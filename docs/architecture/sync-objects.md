# 事件与信号量调用边界

16 个 export 的 provider、scalar ABI 和执行策略集中在 `win32/catalog/sync_objects.rs`，装载或动态查询时绑定 SyncObjectApi。PE32 适配器留在原 API facade，Win64 的 typed adapter 位于 `x64/runtime/sync_objects.rs`；执行时按 ID 选择操作及 A/W、Ex、Open 变体，不再解析 export 字符串。命名对象、句柄、等待调度和错误提交的 owner 保持。

| export | PE32 | Win64 |
| --- | --- | --- |
| CreateEventA/W、OpenEventA/W、SetEvent、ResetEvent | typed；kernel32 | typed；kernel32/kernelbase |
| CreateEventExA/W | ABI-only；error 120、原 stdcall 清栈 | typed；kernel32/kernelbase |
| PulseEvent | typed；kernel32 | absent |
| CreateSemaphoreA/W、ReleaseSemaphore | typed；kernel32 | typed；kernel32/kernelbase |
| CreateSemaphoreExA/W、OpenSemaphoreA/W | ABI-only；error 120、原 stdcall 清栈 | typed；kernel32/kernelbase |

参数中的 handle、name、SECURITY_ATTRIBUTES 和 previous-count 指针保留 guest 指针宽度；BOOL、flags、access 和 LONG count 使用 32 位 carrier，Win64 先按契约截断。声明中的 signed LONG 不改变原位模式。Wine spec 仍作为未迁移家族的 ABI 来源，ABI-only 项不因此获得实现。

PE32 沿用共享 handle/name map：同名对象跨 event/mutex/semaphore 类型复用同一 handle；OpenEvent 也保留原名称查找策略。Create 使用旧 error 183 行为，成功的 Set/Reset/Pulse 不额外清错。Set/Pulse 的 manual/auto waiter 数量和 wake 提交保持；Pulse 清除信号。ReleaseSemaphore 先按旧规则相加并 clamp 到 maximum，再写 previous-count，最后唤醒 waiter；输出 fault 时 count 已提交。公共 gateway 最后读取返回地址，返回槽 fault 也可能发生在资源变更之后。

Win64 的 Events owner 保留对象与每个 alias 的 access，命名对象跨类型冲突报 error 6。Create/Open 的错误、Local 前缀、长度及 access 检查、非零 SECURITY_ATTRIBUTES 拒绝都按原顺序执行。ReleaseSemaphore 先验证 count/权限并获得 previous，再写 guest 输出，最后提交 count；输出 fault 不增加 count。公共 gateway 仍预先保存 return frame，CPU/API count/trace 的最终提交顺序保持。

Win64 普通调用现直接由 Events 执行，只借 Memory 和只读 Threads；文本读取直接使用受检 codec，Threads 在实际写错误时读取当前 TEB。Runtime64 入口只组合资源；对象、alias 和等待算法保持。与 timer 一起通过完整 Win64 269、library 61 passed / 7 ignored 及严格构建，见 `r3-sync-services64` 证据。

本批不迁移 mutex、timer、WaitFor* 或合作线程调度，也不统一两架构不同的命名、权限、count 和 fault 策略。独立旧行为基线与迁移验收见[本批记录](../reports/refactoring-sync-objects-2026-10-16.md)。
