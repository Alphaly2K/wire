# 真实 Unity 注册与 profiler 回调（2026-10-09）

实现位于独立 worktree `native-mono-shared-memory/wire`，默认不开启。
这一阶段继续同一个真实 AliceToPapa UnityPlayer；完整 game Admission 仍 false。
浮点、SSE、翻译缓存、原生 Mono source/patch stack/dylib 均未修改。

## 已执行结果

`tools/mono/probe_startup.py --boundary profiler --skip-etw` 已通过；输出为
`wire-out/native-mono-unity-profiler-probe-04`。原游戏/SDK 只读，使用 APFS clone。

- 收到真实 Unity 的 **10,824** 项 InternalCall；每个名称/Win64 PC 均与原始 Mono
  trace 相同。工具冻结 reference 表及 hash，不以条目数代替逐项匹配。
- 两个实际 CoreModule 方法 `GIDebugVisualisation.get_cycleMode/get_pauseCycleMode`
  经原生 `mono_runtime_invoke`、固定 ARM64 adapter、原始 Win64 target 解释执行返回；
  分别与原始 Win64 对照比较 Boolean 结果。签名必须 static、InternalCall、无参数、
  Boolean、无 byref；CoreModule 也验证精确 hash。
- 其余 **10,822** 项保存在 provider 表内，尚未安装 native 函数指针或准入完整 ABI。
  GI 修改函数在早期引擎状态尚未就绪，实验明确读到未映射地址，故不在此阶段强行调用。
- Unity 实际混用 legacy install 和新 profiler create；NULL context handles 用有界
  token 表保存，不把 native handle 暴露给客体。
- 实际 native BDWGC collect 触发 **8** 次 profiler GC callback，mask **0xfe1**，
  包括 START/END、stop/start-world、locked/unlocked；全部经编译 adapter 解释执行
  原始 Unity callback，而非手工注入事件或跳过 guest 函数。
- native cleanup 再触发 **8** 次 GC callback，以及一次真实 shutdown callback。
  shutdown/resize PC `0x18005fee0` 原始字节是 `c2 00 00`（RET 0），仍逐次解释执行。
- 原始 Mono 无映射/TLS/DllMain；noexec guard **89 checks / 0 denied**。
  显式 skip ETW 的平台差异仍记录，尚未实现 ETW。

## 执行与生命周期

回调使用固定编译 Rust C ABI 入口，Win64 PC 仅存在 provider。专用客体栈预留
Win64 shadow space并满足进入函数时 RSP=8 mod16。每次回调有2M work和8层深度上限；
CPU/SSE/缓存实现不变。Scoped TLS owner 持有 Memory/Runtime 独占借用，FFI 前无活跃
Memory RefCell borrow；scope 先撤销TLS指针，再释放stack/owner。常规返回和unwind均测试。

现阶段只允许同 host owner/guest main thread。外来 Mono worker 进入 adapter 会 fail-stop，
不能并发访问 guest Memory；生产 owner queue 尚未接入此 provider。nested Mono gateway、
loader/scheduler/control transition 仍拒绝，不能把无效回调伪装为成功值。

`Process64` 的 run/error/显式finish/Drop 均在字段销毁前以有效 owner scope cleanup。
清理后不允许恢复客体运行；Mono image继续驻留，避免GC/runtime全局代码指针悬空。
完整 managed metadata/object aliases、guest roots、thread scheduler仍需继续。

## 有限边界与复现

CLI新增 `bindings|profiler`，之前 `root|main-thread|pre-bindings` 保留。
profiler边界停止在UnityTLS安装**之前**，随后验证真实CoreModule/GC并cleanup。
`bindings`停在legacy profiler安装之前。工具默认30M guest work，可用 `--max-instr`调节。
注册表输出 `api.tsv.mono-bindings.tsv`，完整ABI准入列全部0。

```sh
cargo build --release --features native-mono-backend
python3 tools/mono/probe_startup.py \
  --game '/Users/alphaly/Downloads/パパがアリスをママにして♪/AliceToPapaADV_1-0-0.exe' \
  --skip-etw --boundary profiler --output wire-out/native-mono-unity-profiler-fresh
```

这是有限启动诊断，不是完整游戏的速度数据。下一步继续UnityTLS的原生owned interface、
实际managed image/assembly加载与生命周期，以及后续metadata对象ABI。
