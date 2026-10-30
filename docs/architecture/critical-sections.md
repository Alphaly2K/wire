# Critical section 调用与调度边界

新 Wasm 入口在 PE32/Win64 均支持 InitializeCriticalSection、InitializeCriticalSectionAndSpinCount、InitializeCriticalSectionEx、Enter/TryEnter/Leave/DeleteCriticalSection 和 SetCriticalSectionSpinCount。PE32 provider 为 kernel32，Win64 为 kernel32/kernelbase，签名与 DWORD 截断由共享 catalog 声明。

结构分别为 24/40 字节，保留 DebugInfo、LockCount、RecursionCount、OwningThread、LockSemaphore 和指针宽度的 SpinCount 布局。初始化整体预检并写出未持有状态，DebugInfo 为全 1、LockCount 为 -1。SpinCount 接受 DWORD，SetSpin 返回旧 DWORD；单虚拟 CPU 中不执行宿主自旋。Ex 保留 NO_DEBUG_INFO 和低 24 位忽略策略，其余高位返回 FALSE 和错误 87。

Enter 支持同线程递归；TryEnter 竞争失败返回 FALSE，允许结构只读。阻塞 Enter 保存原参数、返回帧及调用层级，不消耗 work，线程按现有合作调度切换并在进度变化后重试。Leave 仅递减当前 owner 的计数，最终释放清空 owner；中间递归释放仍保持其他线程等待。LockCount 使用计数减一的约定，等待者独立保存在调度器中。

初始化写完整结构，Enter/Leave/Delete 只写所有权相关字段，SetSpin 只写 SpinCount。所有写入先检查整个目标范围，故障保持 CPU、work 和旧字段；后续修复可在原网关重试。参数和返回帧在写入前保存，结构与调用帧重叠不改变已保存的返回位置。成功保留 LastError，完成计一 work、零客体退休，不采样时钟。

归属与递归计数不一致、递归溢出、非 owner Leave 或删除仍持有的结构均明确报告服务不可用。当前子集不保证 FIFO 公平性，不提供孤立关键段的 abandoned 恢复，也未接入这些误用对应的 Windows 结构化异常。

待完成调用按加载深度与回调深度保存。锁等待的访存故障进入异常处理器后，处理器的 Sleep、对象或消息等待拥有独立记录；正常恢复保留外层调用。非局部转移、线程退出及 CPU 编辑释放被放弃的等待引用。相同规则也适用于对象与消息等待。

旧 PE32 的顺序字段写入与旧 Win64 适配器继续作为迁移来源。新入口使用当前 VMM、调用 ABI 与调度器，不调用旧 Process 或执行器。
