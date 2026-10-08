# 原生 Mono M2 共享、线程与真实类库验证（2026-10-09）

M1 和原始 PE→IL→guest callback/roots 原型之后，本轮在独立 worktree 补齐了
共享 scalar 数组、阻塞线程、受检 primitive IL 和原始 Unity Mono 叶 ABI 验证。
**M2 尚未完成全部出口；M3 准入和原生游戏后端保持关闭。**

## 已实测的闭环

| 检查 | 结果 |
| --- | --- |
| 原创 PE Win64 Int32 → ARM64 IL → 原创 x64 callback | checksum 19 |
| 真正 pinned Mono int[] → guest 读写 → IL 读写 → 嵌套 callback | 同一 backing；31→36，另一槽为43；返回79，无数组复制 |
| shared 数组经 GC、readonly/越界/撤销 | 保留值，违规访问失败 |
| pin 最后 owner 在未 attach 的 native thread 释放 | null-safe domain TLS 检测，attach/free/detach，线程恢复未 attach |
| 两 native Mono threads | MonoThread/TLS 不同；Managed WaitOne/host Set；强制 GC；同一 guest owner 上200次 callback；数组结果300 |
| guest register/stack roots | 无 strong handle 仍存活；删除快照 roots 后 weak 清空；避免主线程残留指针假阳性 |
| primitive unsafe IL | 整数1/2/4/8字节、float/double、非对齐、blittable struct、cpblk/initblk通过 |
| unsafe 故障 | unmapped/NOACCESS/readonly/guard/cross-page/overflow/越界/revoke，共14个受检故障，无部分写入 |
| 未适配 helper / 引用写入 | Marshal helper 的 call 与 stind.ref 返回 NotSupportedException，未改写 readonly alias |
| 用户 Mono PE 的叶导出访问真实 native Mono 对象 | header/bounds/length/data/unbox metadata+数值一致；原始 VM从未初始化 |
| 用户真实 mscorlib/System/System.Core | 全部M1 core/interop/threads/GC/layout，以及上述M2闭环通过 |

最后一轮真实类库运行正常 cleanup，独立 noexec guard 为 `checks=65 denied=0`。
所有可执行支持在开发构建阶段生成，测试运行没有 mmap/mprotect/vm_protect/
mach_vm_protect executable 或 MAP_JIT 申请。五个 guard 反向测试均以86拒绝。
重新验证的 M1 全部通过，实际链接清单仍为304对象、668编译依赖、0 unmatched；
没有选入 libatomic_ops GPL stack/malloc 设施。这不等于最终发行许可或iOS签名验收。

## unsafe 适配的具体范围

补丁 `0010-scoped-checked-interpreter-memory.patch` 提供版本1的 prototype API：
VM 初始化前 `wire_mono_prepare_memory_adapter` 禁用 IL inline/cprop/super/bblock
优化；在当前线程上为一个明确的 MonoImage 安装访问 callback。callback 的
read/write/copy/fill 操作调用 wire checked Memory，失败由 Mono 自己的异常边界
转换为 AccessViolationException。Rust callback 只返回状态，不跨 C ABI unwind。

同时使用封闭的 **346/613 Mint opcode** primitive 许可表。对 computed-goto
的每次 successor dispatch 与 switch dispatch 都执行许可检查；不能只在
main_loop 标签检查。未知调用、native helper/intrinsic、数组/引用及未覆盖访存
先返回 NotSupportedException，不让部分 hook 后的原生解引用成为隐式回退。
这些数字是 permit-table 的规模，不是通用 IL/游戏兼容率。

合法的 byref/local 仅在当前及活跃 parent interpreter frame 的精确 allocation
跨度内直接处理；不认可整个宿主栈或宿主页。其他指针都需通过 Memory。
普通 blittable field、ldobj/stobj 和 block 路径已测试。引用 write barrier、
跨 frame/native/guest 的混合 block、localloc、atomic、数组对象/metadata 的全量
映射与通用 helper 仍不支持。

scope 是明确的单线程/单 image 原型，不是所有 Mono 程序的沙箱。未安装 scope
的 M1/线程 fixture 只运行本项目控制的 IL；绝不能据此放行任意游戏 IL。
禁用优化是为了防止安全 hook 被当前 transform 的优化/inline 形式绕过，
不是最终性能方案。生产实现需要逐种低阶访存及 helper 的语义覆盖，再保留优化。

## 对象、线程和原始 ABI 的边界

foreign array fixture 只注册 pinned scalar payload；GC不会写这些元素，且没有
managed 并发 writer。普通 managed references/Monitor header/可变 metadata
不满足同样假设。owner须在VM仍存活时释放；生产owner还必须保留VM寿命。
本轮修复了未 attach 线程 Drop 中调用 mono_thread_current 会断言的问题，
改用 null-safe mono_domain_get 检测，并用新的native thread实际释放pin验证。

driver 不跨 native invoke 保持 Rust mutable Guest reference。原型 typed gateway
仍以 Int32/指针为主，M1的 float/struct/ref/out属于host↔host验证，未宣称全部
Win64↔ARM64 ABI已经实现。两线程通过一个 Mutex 短暂领取 guest owner执行
callback，VM等待时不持有它。它证明所测阻塞/GC闭环，不代表完整Process64/
Runtime64队列、guest SEH、domain unload、finalizer→guest或任意nested callback
调度已接入。

原始用户Mono哈希仍为
`86a5a4de6bcd74a637d63c96d822dc989d4ffaed6ef1c15c0e6e1f528ae6708f`。
探测仅执行header常量、mono_array_length/addr和mono_object_unbox叶函数；
导入绑定到拒绝入口，任何import依赖会使probe失败。不调用DllMain或旧VM/GC。
仅登记真实对象的特定不可变字段、scalar payload与所需metadata字节；不暴露
整个宿主metadata页。候选的全部vtable/class/metadata和Unity生命周期仍未匹配。

## 已确认的下一阶段阻塞

使用游戏真实类库的系统契约探测得到：

```text
platform=Unix
file_error=System.DllNotFoundException: kernel32.dll
platform=4 expected_windows=2 windows_vfs_bound=0 native_game_backend_enabled=0
```

真实Windows类库含kernel32 P/Invoke。原生ARM64 Mono不能把wire的Win64 data
gateway当作ARM64 C函数指针调用，也不能直接用宿主文件/线程语义代替guest语义。
修改Platform枚举或放一个假kernel32成功返回库不能解决这个契约。需要类型化、
预编译 native P/Invoke入口，接到wire的隔离Windows服务，并覆盖LastError、
UTF16/结构体、缓冲区所有权与线程身份；unsafe/native-pointer访问检查必须先闭环。

此外，已有290个查询名中286个出现在候选native exports中，缺少
mono_gc_set_heap_validate_frequency、mono_gc_set_heap_verifier_callback、
mono_profiler_init_etw、mono_profiler_startup。查询不是调用计数，出现同名export
也不是ABI兼容证明；诊断/可选接口不得伪装支持。

继续顺序为：受检 reference/array/helper 与生产 roots/scheduler；typed native
Windows服务与scope覆盖；完整ABI/profile状态机；最后才进入M3的Unity VM/provider
接入。当前结果不支持启用真实游戏后端，不提供整款游戏的加速数字。

## 重跑和证据

M1工具新增只读 `--support-builder`、`--classlib-input` 输入，所有classlib副本和
静态support输出仍只写 ignored wire-out。工具不会修改SDK或游戏。完整原型可用：

```sh
python3 tools/mono/build.py all --output wire-out/native-mono-checked-runtime
python3 tools/mono/probe.py all --output wire-out/native-mono-checked-runtime
python3 tools/mono/audit_build.py --output wire-out/native-mono-checked-runtime
python3 tools/mono/probe_m2.py --runtime-output wire-out/native-mono-checked-runtime \
  --checked-memory
```

游戏类库验证使用 `probe.py --classlib-input <游戏Managed目录>`，以既有源码构建
runtime作为只读输入；`probe_m2.py --sample-mono <原始Mono PE>` 追加叶ABI/平台探测。
源码、fixture、runtime、classlib、原始PE和patch的SHA-256写入results.json。

本checkout的复核输出：
`wire-out/native-mono-checked-runtime/`、`native-mono-game-classlib/`、
`native-mono-m2-game-classlib/results.json` 与 `run-guest-bridge.log`。
默认路径测试与计时见 [共享Memory记录](native-mono-shared-memory-2026-10-09.md)。
本阶段不会合并主checkout的SSE工作；所有阶段提交保留在独立分支，无push。
