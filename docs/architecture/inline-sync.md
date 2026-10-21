# SRW、条件变量与 InitOnce 边界

十三项 export 的 provider、arity、参数宽度与架构策略集中到 `win32/catalog/inline_sync.rs`。PE32 保留 kernel32 下的 ABI-only error 120、stdcall 清栈及输出不变；Win64 接受 kernel32/kernelbase，冷绑定为 `ControlImport::InlineSync(id)`。调用时按 ID 处理，不再解析名称前后缀。2026-10-20 的 Alice 接续在原十二项之上新增 `SleepConditionVariableSRW`，属于功能补齐。

`win32/kernel/srw.rs` 的 `SrwLocks` 私有拥有进程锁状态。它检查 guest token 与内部 owner/readers 是否一致、验证释放归属、检查 reader 溢出，再给出待提交的更新或竞争结果。`win32/x64/inline_sync.rs` 先检查 guest 写权限并读取八字节 token，成功写回新 token 后才提交 owner 更新。调度器通过 `available` 查询就绪，保留 shared 可与现有 readers 并行、exclusive 必须无 owner/readers 的规则；没有引入新公平性策略。

adapter 显式返回值、Try 失败、SRW 阻塞或 InitOnce 阻塞。Runtime64 保留 CPU saved return frame、block/unblock、trace/API count 的提交：阻塞不推进 CPU 或计数；失败 Try 提交零返回并计数，保留原有 blocked 状态；一般完成经公共返回路径解除阻塞。SRW owner 与进程同寿命，不持有 CPU、guest 内存或 host mutex。

条件变量 Initialize 写八字节零，waiter 队列归 `x64/runtime/threads/condition.rs`，不向 guest 发布宿主指针；非零外来 wait-list token 仍拒绝。SleepConditionVariableSRW 验证 DWORD flags、condition 与 SRW token 及释放权限，然后在同一合作调度步骤释放锁并登记 waiter。阻塞不消费返回帧或 API count；唤醒或超时后，必须先按原共享/独占模式重新取得锁才返回。成功保留 LastError，超时返回 FALSE/1460，非法 flags 返回 FALSE/87；有限超时保留第一次登记的 deadline。零超时验证释放归属后立即返回超时，调用内没有可观察的线程切换。

Wake/WakeAll 只标记当前未超时且未通知的匹配 waiter，不积累信号；single 按登记序号选择一个，all 标记全部，等待者仍可因锁被占用而继续阻塞。先收到通知的 waiter 不会因随后超过 deadline 改成超时；已经超时的 waiter 不消耗后来的 wake。调度器只在唤醒/超时且锁可取时恢复调用，idle deadline 排除已到期或已通知但仍在等锁的 condition，避免空转。该实现未增加 Windows 的公平性保证、SleepConditionVariableCS 或异步 InitOnce。

InitOnceBegin 的顺序仍为 flags 检查、state 读取、pending/context 输出预检，再依状态写入；发布时依次写 state、pending、context。InitOnceComplete 保留非法 flags/context 的短路，之后才读 state；错误经当前线程 LastError 槽写入。输出别名不做去重或事务合并，返回槽在进入语义前保存。

真实 guest 线程的 SRW/InitOnce 重试、PE32 ABI-only、token/归属故障、DWORD flags 和输出别名等独立基线见[验收记录](../reports/refactoring-inline-sync-2026-10-16.md)。critical section、WaitOnAddress 和线程生命周期仍使用各自现有路径。

条件变量新增两项队列回归与两项 Win64 行为回归，覆盖先 wake 不积累、不同 condition 隔离、single 登记顺序、all/shared 重取锁、超时后锁仍被占用、迟到 wake、通知后 deadline 到期、非法 flags/DWORD 截断、只读锁及阻塞重试身份。真实 guest 线程分别在 1-work 和 100,000-work 切片执行 exclusive/shared 唤醒和持锁超时三种场景；PE32 另验证原 ABI-only 的 16 字节清栈与输出保持。
