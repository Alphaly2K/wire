# WaitFor* 调用边界

四项 WaitForSingleObject/Ex、WaitForMultipleObjects/Ex 的 provider、scalar ABI 和 WaitApi ID 集中到 `win32/catalog/waits.rs`。PE32 前三项 typed，MultipleEx 保留 ABI-only error 120 和 20 字节 stdcall；Win64 四项均接受 kernel32/kernelbase。USER 消息等待、SleepEx 和 Winsock 等待继续使用原入口。

Win64 冷绑定为 `ControlImport::Wait(id)`，普通 scalar catalog handler 不接管阻塞或回调。single/multiple、alertable 由 ID 决定，进程框架继续拥有 saved return frame、deadline、block/unblock、APC delivery、trace 与 API count 的原提交顺序。DWORD 参数截断为 32 位，handles 和数组地址保持完整 64 位；multiple count 限于 1–64，先读完受检 handle 数组，再检查权限或 acquire。wait-all 的原原子获取、auto-event 消费、重复对象、thread/pipe/timer 和访问错误语义保持。

PE32 的对象 readiness、consuming acquire 和 waiter wake 位于 `win32/x86/object_waits.rs`，只借 KernelObjects32 和 Sched；DirectSound 通知也直接借同一对象表发出 signal，不再经过整个 ApiState。SingleEx 继续忽略 alertable。Multiple 保留 count clamp 到 64 和空数组的历史结果，没有统一 Win64 的严格参数策略。成功 acquire 发生在公共 gateway 读取返回槽之前，返回槽 fault 时已获取的 auto-event 不回滚。multiple 先读取整个 clamp 后的数组，再消费 wake 或获取对象。

PE32 single/multiple 与 MsgWaitForMultipleObjects/Ex 编排由 `x86/process/synchronization.rs` 执行。各入口明确借内存、对象表、错误槽和调度资源，消息等待额外借 User32 与起始时间；不借整个 ApiState。原数组读取、wake 消费、对象获取、消息/定时器 readiness 与 deadline 顺序保持，返回帧仍由公共 gateway 提交。

event/semaphore/mutex 的创建、打开、signal/reset/pulse 与 release 位于 `win32/x86/sync_objects.rs`，仅借对象表、调度器、GuestMem 和 LastError。命名对象复用的 LastError=183 在同一适配函数维护，ApiState 的公开 create_named 兼容入口委托到这里。PulseEvent 保留先清 signal 再唤醒 Retest 的旧策略；ReleaseSemaphore 保留先更新 count、再写旧值、最后唤醒的故障提交顺序。CPU、block/deadline、trace 和公共返回帧仍由进程调用入口编排。

WSAWaitForMultipleEvents 仍走独立名称路径，保留 Winsock 初始化检查、10093/10022 映射及原 alertable 控制，不注册为 kernel WaitApi。架构差异与独立 fault/APC 基线见[验收记录](../reports/refactoring-waits-2026-10-16.md)。
