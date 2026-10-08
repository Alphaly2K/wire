# 独立 Unity Mono 原型

此目录不接入 Cargo 或游戏后端。源码候选和归档 SHA-256 固定在
`source-lock.json`；尚未证明精确对应 AliceToPapa 的 Unity 6000.3.11f1 DLL。
构建和测试只写入忽略目录 `wire-out/native-mono-m1`，Unity SDK 只读。

macOS ARM64 重建：

```sh
python3 tools/mono/build.py all
python3 tools/mono/probe.py all
python3 tools/mono/audit_build.py
```

可用 `--output` 指定另一个 `wire-out` 子目录；`probe.py --sdk` 指定本地 Unity
MonoBleedingEdge。首次准备需要网络下载；后续验证本地归档哈希。
不下载或提交游戏 DLL、程序集和资产。

`aot-tool` 是开发阶段编译工具，允许生成 ARM64 静态解释器支持文件；
不得用于运行游戏。`runtime` 关闭 JIT，并编入拒绝 executable/JIT mapping
的补丁。`--aot=interp` 只为本地 class library 生成支持 wrappers/trampolines；
原创 fixture 的普通方法编译数量为 `0/0`，方法体通过 IL interpreter 执行。
这些支持文件是开发时构建的 Mach-O，未来分发还需要打包、签名和版本匹配。

构建用 `aarch64-apple-darwin20` 选择上游 macOS ARM64 分支，防止新 Darwin
版本被旧 configure 误识别为 iOS；实际编译 SDK、宿主系统和日志另存于输出。
GC 使用锁定的 Unity BDWGC，禁用线程自动发现与静态库全局区自动扫描；
Mono 显式注册 roots 和线程。未使用 SGen，也未禁用 GC。

`noexec_guard` 是独立 macOS 测试保护层，拦截 mmap、mprotect、vm_protect 和
mach_vm_protect。五个拒绝测试必须返回 86，然后所有正确性测试必须正常退出、
给出 PASS 和保护层统计。它不是通用的系统沙箱，也不替代 iOS 真机验收；
dyld 加载构建好的静态支持库与运行时生成代码是不同的操作。

补丁说明及实测边界见 `docs/native-mono-m1-2026-10-09.md`。
`audit_build.py` 从最终静态归档和编译依赖记录实际选入对象，检查未混入
libatomic_ops 的 GPL stack/malloc 设施；该清单不是最终发行许可审计。
上游 notices 位于 `licenses/`，固定的完整源码归档及逐文件版权仍须保留。
