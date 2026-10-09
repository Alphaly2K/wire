# Unity classlib 初始化与 remoting IL 修复（2026-10-09）

`wire-out/native-mono-unity-classlib-probe-02` 实际执行锁定 CoreModule 的
`UnityEngine.ClassLibraryInitializer.InitAssemblyRedirections()`（token `0x060022ff`）。
其 IL 建立真实 managed ResolveEventHandler 并订阅 AppDomain.AssemblyResolve。
17,546 metadata queries / 1,926 handles、82 image cycles、GetType、String/RuntimeType
GC 身份验证和 shutdown 均完成；guard **176 checks / 0 denied**，exit 0。
原始 x64 Mono 未 attach，完整 game Admission/native backend 仍 false。

首次运行定位到 no-JIT Mono 的 remoting field-address wrapper 没有 IL：
`remoting.c` 把 ldfld/ldflda/stfld wrapper 的 IL emitter 放在 `#ifndef DISABLE_JIT` 内。
patch `0020-interpreter-remoting-field-wrapper-il.patch` 只将四处 guard 改为
`!defined(DISABLE_JIT) || !defined(DISABLE_INTERPRETER)`；保留 proxy/domain/context
检查，不启用宿主机器码生成。未改浮点、CPU/SSE、IR 或翻译缓存。

修复 runtime 单独位于 `wire-out/native-mono-remoting-runtime`，source/build 为实体 APFS
副本。旧 19-patch runtime dylib 未变；第一次复制发现 source symlink 后已恢复旧 source、
19-patch marker 和依赖链接，并逐项核对 marker 记录的文件 hash。build.py 现在在 prepare/
runtime 之前拒绝 writable source/build 指向 output root 外，新增两个隔离回归。

provider 按 runtime/support fingerprint 成对选择，load 前重检 runtime hash，classlib
阶段要求新 pair，避免错配。runtime SHA256：
`17f7859533bdeb5e636fc4e19a3aa1ac55502dbf089c6ede7c2b506ecdd4dcda`；support SHA256：
`a916d6f9c9199b524a65ebd93bdea328e31b248c415051a2d2a8609664580d6c`。

原创 native fixture 验证 MarshalByRefObject 字段读写、object/int ref CAS 成功与失败、
event 双订阅/解绑/GC、AppDomain.AssemblyResolve 的实际 private delegate field。
新 runtime core/interop/threads/gc/layout/remoting 六套通过；mmap/jit/mprotect/
vm_protect/mach_vm_protect 五种 executable-memory 反向路由均以 86 拒绝。
feature 完整 Rust **397 passed / 7 ignored**，feature all-target Clippy -D warnings
通过，build 工具 **5 tests passed**。证据均来自新 20-patch build。

调用前核对 class/image 身份、token/name、static void()、非 generic/inflated，native invoke
不接受 guest pointer，exception 用 native local，guest exception 输出权限先检查。
`--managed-trace` 仅用于诊断，不用于性能比较。CLI boundary `classlib` 目前执行一次
InitAssemblyRedirections，随后停在 `ClassLibraryInitializer.Init()`（token `0x060022fe`）
之前。它调用 UnityLogWriter.Init；下一步继续日志初始化和真实日志 InternalCall ABI。
