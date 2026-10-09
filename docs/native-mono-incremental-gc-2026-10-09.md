# 原生 Mono 增量 GC 静态写屏障修复

锁定 Unity Mono `75711b4363fa58f05f953b3f95400d4d61c0ca35` 的 no-JIT ARM64
解释器在 `--gc-params=incremental=3` 下损坏真实 corelib 的 collation 对象图。
旧 20-patch runtime 的独立 `NativeProbe.Collation()`：普通 GC 通过，Unity 增量配置
抛出 `System.TypeInitializationException`。真实 Unity 的 Console 初始化还出现
“Collation tailoring table is broken for culture127”和数组越界，另一次运行 SIGSEGV。
managed trace 发现仍被 static 引用的 TailoringInfo 地址被新 char[] 重用。

patch `0021-interpreter-static-write-barriers.patch` 保留增量 GC：

- `MINT_STSFLD_O` 和 `MINT_STTSFLD_O` 改用 generic reference store barrier。
- `MINT_STSFLD_VT` / `MINT_STSSFLD_VT` 使用 range-copy barrier；结构体可能含引用，
  不能仅 memcpy。普通 static 的 vtable 初始化顺序保持不变。
- 已检查 instance/reference array/indirect object store 和带引用的 instance/value
  store：这些路径已有相应 barrier。没有修改数值运算、CPU/SSE、IR 或翻译缓存。

修复后真实 collation（invariant/en-US/ja-JP Compare/IsPrefix，反复强制 GC）通过。
新增 StaticRoots 压力测试覆盖 static object array、线程 static reference、普通 static
结构体、泛型 static 结构体，32 轮分配压力及强制 GC 后逐个验证内容。
core/interop/threads/gc/layout/remoting/collation/static-roots 八套，在普通配置和
Unity 增量配置下全部通过。五条 noexec 反向路由均以 86 拒绝可执行内存操作。
`tools/mono/test_build.py` 的 5 项测试通过。

新运行时的源码、依赖和构建目录为实体 APFS 副本；依赖 symlink 指向新 output 内部。
旧 runtime 没有修改。新制品位于 `wire-out/native-mono-incremental-runtime`：

- runtime SHA256 `c89ab8c9cc00a045e998cb9bc528f2959f478729136390a0d07e54b8e8b69bda`
- matching corelib support SHA256 `edd2730c016afafc55e3dbad617fcaff6332474a9fde6808205de8edb7e53c8e`

失败证据为 `wire-out/native-mono-collation-fixture/probe/run-collation-incremental.log`；
修复后证据为新 output 的 `probe/run-*-incremental.log`、`run-*.log` 及配置 manifest。
`probe.py --incremental` 在 init 之前设置原始 Unity GC 参数，结果文件和 case 日志与
普通配置分开保存。全量八套的日志都保留；results.json 记录最后一次选择的 case 集合。
这证明当前回归中的故障已修复，不是完整游戏/所有 GC 边界的准入声明。

实际 Unity 复测 `wire-out/native-mono-unity-logging-incremental-01` 完成
InitAssemblyRedirections 和 ClassLibraryInitializer.Init → UnityLogWriter.Init，
exit 0，guard 179/0，原始 Mono 未 attach。尚未接日志输出 adapter 的该次运行不应
被称为完整日志链路验证；后续日志/异常/同步上下文推进由独立文档记录。
