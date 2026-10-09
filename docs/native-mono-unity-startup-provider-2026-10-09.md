# 真实 Unity → 原生 Mono 有限初始化入口（2026-10-09）

本轮把已有原生 Mono 原型接入了 AliceToPapaADV 的真实 UnityPlayer 启动过程，
不再只运行独立 fixture。诊断入口能够完成原生根域、主线程、域配置以及 embedding
hostname/异常策略设置，然后清理并停止；完整游戏后端与 M3 Admission 仍关闭。
这不是游戏已原生化，也不是启动到窗口或游戏帧率的性能比较。

## 单 VM 选择与边界

默认 `Process64::load` 仍使用原始 x64 Mono/解释器。只有 feature
`native-mono-backend` 加显式 `--native-mono-startup-probe DYLIB` 才选择独立诊断 provider。
它不伪造 `Admission` 的完整 ABI、roots、scheduler 或 callback 证明。

选择发生在任何原始 Mono 映射/初始化之前：核对 EXE、UnityPlayer、corelib、预编译
corelib 支持文件、ARM64 dylib 的固定 SHA256；加载原始 DLL 文件时再核对其 fingerprint。
原始 Mono 的模块句柄范围只保留地址空间，不映射任何 PE 页、不执行 imports、TLS、
DllMain 或 thread notifications。GetProcAddress 仍保留原始导出存在性；已存在的名称/ordinal
转为解释器识别的 gateway token，缺失导出继续返回 NULL。

名称与 ordinal 从原始导出表对应，不用“相同机器码地址”猜测不同原生签名：存在多个
名称共用同一原始地址时拒绝替换。未命名 ordinal 的调用明确拒绝，不能绕回原始 VM。
宿主 dylib 在首次 native 调用前再次核对 hash；一宿主进程只允许一次原生 runtime 生命周期。

根域/Profiler/主线程都只向 guest 返回不透明 token，地址不映射为可读对象或 metadata。
每个普通参数先从受检 guest Memory 复制到 owned 数据，再以固定 C 签名调用 native API。
Windows 路径通过同一 GuestFs 转成 confined host 路径；字符串/argv/双 NUL path list 有容量限制。
可写 argv 使用 owned 可写缓冲，guest 回调 PC 和栈指针不会直接传入 ARM64 C API。

## 真实初始化段

当前固定签名处理的 startup boundary 包含：

- 目录/path list、signal/crash chaining、默认优化 query/settings、两个真实 CLI option；
- main args、data dir、coverage、NULL payload profiler 与 NULL coverage filter；
- `mono_jit_init_version("Unity Root Domain", "v4.0.30319")`、默认 config parse；
- current thread、两处 NULL-owner guest 栈 reference store、main-thread 设置；
- root-domain config、embedding hostname `Unity`、legacy unhandled-exception policy。

实际 startup options 是 `--gc-params=incremental=3` 与 `-O=-float32`。其他 option 在传入
Mono 前拒绝，不能让未知 option 改变 interpreter/no-JIT 运行模式。
候选 runtime 使用锁定的 `MONO_AOT_MODE_INTERP`（5），并执行 memory adapter preparation；
`mono_class_get_userdata_offset` 必须实测为原始叶 ABI 的 232 才准入。

`mono_unity_install_memory_callbacks` 的这一个原始样本是 `c2 00 00`，即立即返回，
既不读取 callbacks 表，也不写 RAX。gateway 保留 RAX、只弹出返回地址；不能使用 native
candidate 会读版本字段的 installer 替代，更不能据此放宽其他 runtime 的 allocator 所有权。

主线程是真实 native MonoThread object，由 v2 strong handle 保活到诊断停止；guest 仅
持不透明 identity token。两处原始 `mono_gc_wbarrier_set_field(NULL, guest_stack_slot, thread)`
经过 guest Memory 受检写入。限定 aligned、仍在活跃 main stack 的槽，拒绝真实 object owner、
未登记 token、heap、return/shadow slots、overflow 和不可写页面。没有把 guest slot 传给 native
write barrier。这只证明有限初始化交接，**不是**通用 managed reference store 或完整 guest root
扫描；诊断始终保留整个 exposure 的强引用，停止后先 free handle，再 cleanup root domain。

## 明确的平台差异和未实现项

这个 Windows UnityPlayer 会请求 `mono_profiler_load("etw")`。原始 Windows Mono 的
ETW initializer 有真实代码，不能当成 no-op。ARM64 candidate 没有对应 ETW 实现：默认
诊断诚实拒绝；只有显式 `--native-mono-probe-skip-etw` 才记录平台差异并跳过这个 Windows
profiler。此条不计为已实现的 profiler 契约。

plugin finder 只登记原始 guest PC 并安装预编译的拒绝入口，未建立 typed guest reentry。
若 native 初始化真的激活它，入口输出 `NATIVE_MONO_STARTUP_REJECT` 并终止该独立进程，
不会返回假 NULL/成功，也不会让 Mono 继续尝试宿主 library fallback。这些成功初始化运行
没有激活 plugin callback；不能推断后续加载/游戏运行不需要它。

每个 boundary、异常、非 budget 终止均关闭 probe，并在原 attached owner thread 清理。
成功后不能 resume；dylib 保持 resident，防止残留 Mono/GC code pointer 指向被卸载镜像。
pre-root 失败可能保留 native 全局配置/profiler allocation 到进程退出，不支持同进程重试。

三种有限 stop 位置是 `root`、`main-thread`、`pre-bindings`。最后一种在 exception policy
设置后停止，尚未向 native Mono 注册 Unity InternalCall，也未运行游戏 Managed DLL。
不要把上轮原始 VM 捕获的 10,824 个 InternalCall 名称/target 匹配当作本轮 typed native gateway。

## 复现与证据

工具 `tools/mono/probe_startup.py` 创建全新 APFS game clone，只修改 clone；复制已经在
开发阶段预编译的 `mscorlib.dll.dylib`。运行时不编译 support、不修改原游戏/SDK。输出保存
frozen wire、命令/环境、输入 hash、startup log、aggregate API trace 和 results。

```sh
cargo build --release --features native-mono-backend
python3 tools/mono/probe_startup.py \
  --game '/Users/alphaly/Downloads/パパがアリスをママにして♪/AliceToPapaADV_1-0-0.exe' \
  --skip-etw --boundary pre-bindings \
  --output wire-out/native-mono-unity-startup-final-pre-bindings
```

固定 native runtime SHA256 为
`2cfdc2552a60ac42729bf73ef3eeb7e6b2f25031835f6a8e639ec3fa61ad11e7`；
固定 corelib support SHA256 为
`b10bb3e59af90e92e240424ac88b1f96eb27906c3fa099958713d93801b4b078`。
其他固定 hash 在代码与各运行的 inputs.json 中。

三阶段均需同时满足：进程 exit 0、相应 ready marker、无原始 Mono attach stage、
独立 noexec guard checks > 0/denied = 0。未显式 skip ETW 的运行应 exit 1，root 未建立。
headless 有限段约 3.31M guest work；不会创建正式渲染窗口，耗时不能与先前完整启动比较。

回归覆盖参数 faults/bounds、allocator 的无 table 读取且保留 RAX、导出/ordinal 的 token
映射与歧义拒绝、无 root 或非合法 stack store 的拒绝、关闭后调用拒绝。完整 default 与
feature Rust tests 和 feature Clippy 日志位于 `wire-out/native-mono-startup-*.log`。

## 本轮最终验证

最终 frozen 构建的证据目录与结果：

| ignored 输出目录 | stop/结果 | noexec guard |
| --- | --- | --- |
| native-mono-unity-startup-final-root | root ready，exit 0 | 85 checks / 0 denied |
| native-mono-unity-startup-final-main-thread | root + main thread ready，exit 0 | 85 / 0 |
| native-mono-unity-startup-final-pre-bindings | root + main thread + pre-bindings ready，exit 0 | 85 / 0 |
| native-mono-unity-startup-final-etw-rejection | ETW 未显式关闭，root 未建立，预期 exit 1 | 60 / 0 |

四次运行均 `original_mono_attach_observed=false`、`full_game_backend=false`。
最终完整 Rust 回归：default **365 passed / 7 ignored**，feature **389 passed / 7 ignored**；
feature all-target Clippy `-D warnings` 通过。本轮新增 4 个必要的边界/生命周期回归。
原生 Mono source/19-patch stack/dylib 未修改或重建。

## 接下来

把真实 10,824 个注册 entry 与 managed signature inventory 连接为 typed registry；
先实现实际启动优先的固定 ARM64 adapters/owner-thread guest reentry，逐项处理 reference、
Injected struct/ref/out 与返回值。未经准入的 guest targets 不得传入 native Mono。
profiler GC callbacks、UnityTLS 和 managed assembly/image loading 是注册之后的下一批真实边界。
完整生产 roots/safepoints、services/scheduler 仍须单独验证；上述有限入口不能代替它们。
