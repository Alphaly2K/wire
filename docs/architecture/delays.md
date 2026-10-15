# Sleep 与线程让出边界

Sleep、SleepEx、SwitchToThread 的 provider、DWORD 参数和 typed ID 集中在 `win32/catalog/delays.rs`。PE32 接受 kernel32，Win64 接受 kernel32/kernelbase；调用阶段按 DelayApi 分发，不再维护重复 ABI/arity/DWORD 或解析这些名称。

Win64 冷绑定为 `ControlImport::Delay(id)`，保留进程框架的控制结果。SleepEx 在 alertable 时先尝试交付 APC，保留已有 deadline、INFINITE 和 Blocked 路径，正常完成使用公共 Returned 提交。Sleep 保留原 Sleep block、unblock 与 Yielded；SwitchToThread 保留 SwitchThread 请求，由 Process 决定最终调度结果。后两项在发出请求前提交 saved return frame/CPU/API count，原 trace 分工保持。返回槽在执行 handler 前受检，fault 时 CPU、counter 与线程控制均未提交。

PE32 保留三个原 body：Sleep/SleepEx 先消费 wake，零延时在完成返回帧后才 round-robin，非零延时 arm deadline 后阻塞；SleepEx 继续忽略 alertable。SwitchToThread 先 poll timeout，再根据其它 runnable 线程设置 BOOL 和 yield。公共 gateway 先写 EAX，再读返回槽，之后才允许线程让出；故障时保留已写 EAX，未执行切换。LastError 不额外清除。

两架构的原 deadline、INFINITE、返回/trace/调度差异保持，没有合并 sleep 与 wait 状态，也未新增回调能力。独立故障基线、实际线程切换/APC/timeout 与原创零延时循环见[验收记录](../reports/refactoring-delays-2026-10-16.md)。
