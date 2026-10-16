# 线程查询、设置与状态 owner

九项 export 的 provider、参数宽度、返回约定与 ID 集中在 `win32/catalog/thread_properties.rs`：Get/SetThreadErrorMode、Resume/SuspendThread、GetThreadId、Get/SetThreadPriority、GetExitCodeThread、SetThreadAffinityMask。Win64 接受 kernel32/kernelbase；PE32 仅 kernel32，其中 GetThreadId 与两项 error mode 保持 ABI-only/error 120，其余六项使用 typed adapter。错 DLL 的历史未知 ABI/拒绝策略保持。

Win64 冷绑定普通 scalar ID，由 Runtime64 保存返回帧后交给 `Threads::property_call`。owner 只借 Memory 和当前 LastError 地址，内部 error mode 字段私有。CPU、公共 unblock、trace 和 API 次数仍由进程提交。Create/ExitThread、APC、CONTEXT 和 description 分配保留专用路径。

Win64 先解析完整 64 位 thread handle，失败写 error 6，再返回各 API 的原 sentinel；窄化的 pseudo handle 不成立。优先级参数截断为 DWORD，接受 −15、−2…2、15，读取负优先级仍零扩展低 32 位。affinity 使用完整 carrier 且只接受 1。挂起计数上限 127，溢出返回 DWORD MAX/error 87，恢复饱和减一；两个 API 都返回操作前计数。exit code 输出为 DWORD，活动线程为 259。

error mode 校验先于输出访问；有效 mask 为 0x8007。非空输出先预检、再写旧值，最后更新 owner。输出 fault 不改变 mode，CPU/API/trace 不提交；已保存的返回帧允许输出覆盖原返回槽。成功保留原 LastError，输出与 LastError 别名则按原写入顺序生效。

PE32 adapter 只借 GuestMem、Sched 和 LastError。Sched 拥有 exit 状态读取和二态挂起切换；已有 Waiting/Exited 状态保持，重复挂起不累加，挂起/恢复成功都返回零；这两项不接受当前线程 pseudo handle。优先级设置保持空操作成功、查询固定零。affinity 接受 pseudo handle 或既有调度 handle，随后检查 mask=1。未知 exit handle 仍成功写零，活动线程写 259，已退出线程写其 code。输出先于返回地址读取，覆盖返回槽会改变返回目标；保持旧日志文本与 fault/清栈顺序。

本批保持上述兼容行为，未增加 Windows 线程功能，详见[独立基线与验收](../reports/refactoring-thread-properties-2026-10-17.md)。
