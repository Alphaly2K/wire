# WaitFor* 调用边界

四项 WaitForSingleObject/Ex、WaitForMultipleObjects/Ex 的 provider、scalar ABI 和 WaitApi ID 集中到 `win32/catalog/waits.rs`。PE32 前三项 typed，MultipleEx 保留 ABI-only error 120 和 20 字节 stdcall；Win64 四项均接受 kernel32/kernelbase。USER 消息等待、SleepEx 和 Winsock 等待继续使用原入口。

Win64 冷绑定为 `ControlImport::Wait(id)`，普通 scalar catalog handler 不接管阻塞或回调。single/multiple、alertable 由 ID 决定，进程框架继续拥有 saved return frame、deadline、block/unblock、APC delivery、trace 与 API count 的原提交顺序。DWORD 参数截断为 32 位，handles 和数组地址保持完整 64 位；multiple count 限于 1–64，先读完受检 handle 数组，再检查权限或 acquire。wait-all 的原原子获取、auto-event 消费、重复对象、thread/pipe/timer 和访问错误语义保持。

PE32 保留 scheduler wake/deadline 与对象 helper；SingleEx 继续忽略 alertable。Multiple 保留 count clamp 到 64 和空数组的历史结果，没有统一 Win64 的严格参数策略。成功 acquire 发生在公共 gateway 读取返回槽之前，返回槽 fault 时已获取的 auto-event 不回滚。multiple 先读取整个 clamp 后的数组，再消费 wake 或获取对象。

WSAWaitForMultipleEvents 仍走独立名称路径，保留 Winsock 初始化检查、10093/10022 映射及原 alertable 控制，不注册为 kernel WaitApi。架构差异与独立 fault/APC 基线见[验收记录](../reports/refactoring-waits-2026-10-16.md)。
