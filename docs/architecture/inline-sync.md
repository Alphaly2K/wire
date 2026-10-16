# SRW、条件变量与 InitOnce 边界

十二项 export 的 provider、arity、参数宽度与架构策略集中到 `win32/catalog/inline_sync.rs`。PE32 保留 kernel32 下的 ABI-only error 120、stdcall 清栈及输出不变；Win64 接受 kernel32/kernelbase，冷绑定为 `ControlImport::InlineSync(id)`。调用时按 ID 处理，不再解析名称前后缀。

`win32/kernel/srw.rs` 的 `SrwLocks` 私有拥有进程锁状态。它检查 guest token 与内部 owner/readers 是否一致、验证释放归属、检查 reader 溢出，再给出待提交的更新或竞争结果。`win32/x64/inline_sync.rs` 先检查 guest 写权限并读取八字节 token，成功写回新 token 后才提交 owner 更新。调度器通过 `available` 查询就绪，保留 shared 可与现有 readers 并行、exclusive 必须无 owner/readers 的规则；没有引入新公平性策略。

adapter 显式返回值、Try 失败、SRW 阻塞或 InitOnce 阻塞。Runtime64 保留 CPU saved return frame、block/unblock、trace/API count 的提交：阻塞不推进 CPU 或计数；失败 Try 提交零返回并计数，保留原有 blocked 状态；一般完成经公共返回路径解除阻塞。SRW owner 与进程同寿命，不持有 CPU、guest 内存或 host mutex。

条件变量 Initialize 写八字节零；Wake/WakeAll 仅接受零 wait-list token。未新增 SleepConditionVariable 或异步 InitOnce。InitOnceBegin 的顺序仍为 flags 检查、state 读取、pending/context 输出预检，再依状态写入；发布时依次写 state、pending、context。InitOnceComplete 保留非法 flags/context 的短路，之后才读 state；错误经当前线程 LastError 槽写入。输出别名不做去重或事务合并，返回槽在进入语义前保存。

真实 guest 线程的 SRW/InitOnce 重试、PE32 ABI-only、token/归属故障、DWORD flags 和输出别名等独立基线见[验收记录](../reports/refactoring-inline-sync-2026-10-16.md)。critical section、WaitOnAddress 和线程生命周期仍使用各自现有路径。
