# 事件与信号量

新 Wasm 入口在 PE32 与 Win64 均支持 CreateEventA/W、CreateEventExA/W、OpenEventA/W、SetEvent、ResetEvent、CreateSemaphoreA/W、CreateSemaphoreExA/W、OpenSemaphoreA/W 和 ReleaseSemaphore。参数宽度和 provider 来自 `win32/catalog/sync_objects.rs`；PE32 另接入 PulseEvent，AMD64 保持该项 absent。

事件、信号量、[互斥体](mutexes.md)与[定时器](timers.md)共用有类型的命名空间。同名、同类型创建返回新的句柄别名，保留已有对象状态；跨类型冲突返回错误 6。名称区分大小写，`Local\name` 与 `name` 共用身份，最多接受 260 个 UTF-16 单元。A 名称使用 CP932，W 使用 UTF-16。

创建先检查 LastError 输出，成功时新对象写 0，已有对象写 183；打开与其它成功操作保持 LastError。SECURITY_ATTRIBUTES 接受正确结构长度、空安全描述符及继承标志；非空安全描述符返回错误 50。Ex 创建拒绝未知 flags，访问掩码按对象类型验证。

manual-reset event 在 ResetEvent 前保持信号；auto-reset event 每次成功等待消耗一个信号。SetEvent 不积累重复信号。信号量只接受正数释放量；超过 maximum 返回错误 298，非法数量返回错误 87。ReleaseSemaphore 先检查权限、数量和可选 previous-count 输出，写入成功后才增加计数；输出故障不会增加 credits。对象等待见[混合等待](waits.md)。

每个别名有独立权限、继承和保护关闭标志，支持 CloseHandle、DuplicateHandle、Get/SetHandleInformation。等待需要 SYNCHRONIZE，Set/ResetEvent 和 ReleaseSemaphore 需要 modify-state；权限不足返回错误 5。对象句柄范围为 `0x90000000..0xa0000000`，最多 1024 个活动句柄，身份单调分配而不复用。最后一个句柄关闭后，已挂起等待仍保留对象和名称，直至等待完成或取消。

PulseEvent 要求事件的 modify-state 权限，复用对象身份和普通混合等待的原子获取规则。只检查当前作用域内已经挂起的等待，manual event 完成全部可满足等待，auto event 至多被一次成功等待消费。wait-all 的其余对象或消息不就绪时不消费任何对象；已过期、有待投递 APC 的 alertable wait、被更深回调覆盖的等待不接收脉冲。完成值保存在等待续点中，随后 Set/Reset、关闭句柄、超时或新 APC 均不撤销它；对象引用保留到等待实际完成或取消。事件在 PulseEvent 返回前复位，后来发起的等待不能获得该脉冲。线程间选择顺序不提供公平性保证。

调用帧在资源变更前完整捕获，输出可覆盖原参数或返回槽；成功计一 work，不计客体退休。普通事件和信号量操作不采样时间；PulseEvent 仅在相关等待有有限 deadline、活动 timer 或消息 timer 时取一个单调样本，时间源失败不改变事件或等待。完成值已冻结的等待恢复时不再采样或获取对象。
