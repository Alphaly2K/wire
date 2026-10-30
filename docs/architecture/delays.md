# Sleep 与线程让出

Sleep、SleepEx、SwitchToThread 的 provider、DWORD 参数和返回约定集中在 `win32/catalog/delays.rs`，新 Wasm 入口在两种架构使用同一调度协议。

Sleep(0) 先完成保存的返回帧，再请求 round robin 切换。有限正延迟在首次调用时读取单调 10 MHz 时钟并固定 deadline，醒来时正常返回；INFINITE 保持阻塞。SleepEx 的 alertable=TRUE 支持[用户 APC](thread-control.md)：已有队列优先于延迟且不采样时间，投递完成返回 WAIT_IO_COMPLETION；无 APC 时沿用普通延迟协议。

SwitchToThread 在存在另一个可运行线程时返回 TRUE 并请求切换，否则返回 FALSE。新线程只有在加载锁可用时才具备启动资格，已经开始的线程可在其他线程持有加载锁时执行普通代码和 I/O。

阻塞保留调用帧、CPU、LastError、deadline 和预算，成功返回计一 work。零延迟及 SwitchToThread 不采样时钟；正延迟使用与 QPC 相同的来源。时间源失败先交回宿主，后续恢复沿用已建立的等待状态。无可运行线程时 run 返回 Waiting，由后续运行或宿主输入继续推进；run(0) 不改变状态。

[线程等待](waits.md) 与[线程生命周期](thread-control.md) 描述共享的状态保存、预算和唤醒边界。
