# Critical section 调用与调度边界

八项 export 集中声明于 `win32/catalog/critical_sections.rs`。PE32 接受 kernel32，七项 typed，SetCriticalSectionSpinCount 保留 ABI-only error 120。Win64 八项接受 kernel32/kernelbase，按冷绑定的 `ControlImport::CriticalSection` 调用；DWORD spin/flags 的截断也由契约提供。

两种架构的 guest 结构与故障顺序分别实现。PE32 adapter 只借 GuestMem 和当前 tid，依次写 24 字节结构中的六个 DWORD，保留部分写入故障、忽略 Ex flags、Leave 饱和及 Delete 不检查 ownership 的历史策略。Enter/TryEnter 先写 owner，再写 recursion 和简化 LockCount；不在搬迁中改变算术或引入全结构预检。一般返回仍在公共 PE32 gateway 读取返回槽之前完成内存操作。

PE32 adapter 输出返回值、包含原 owner 的阻塞请求或最终释放后的唤醒请求。进程层消费旧 Wake::Retest、记录原阻塞 note、保存/切换 CPU，并沿原线程顺序只唤醒第一个匹配等待者；中间递归释放不唤醒。adapter 不持有 CPU、ApiState 或 scheduler。

该编排位于 `x86/process/synchronization.rs`，只借 CPU、内存、日志和 scheduler。公共 API facade 消费 Returned/Blocked 结果，仍在原位置提交 EAX、返回槽和 LastError。

Win64 adapter 只借受检 Memory 和当前 tid，按 40 字节结构读取/写回。Enter 的竞争先检查全结构写权限，再请求阻塞；Try 的竞争直接返回失败，允许只读结构，并保留原不解除 blocked 状态的提交。递归溢出、非 owner Leave、删除 held 结构继续拒绝。SetSpin 保留全结构读检查、spin 槽写检查，以及旧值的完整 64 位返回 carrier。Runtime64 在语义前保存返回帧，负责最终 CPU、block/unblock 和 trace/API count。

这是既有合作调度子集的迁移，没有新增 host mutex 或公平性规则。独立 partial fault、返回槽别名、真实线程最终释放重试与性能证据见[验收记录](../reports/refactoring-critical-sections-2026-10-16.md)。
