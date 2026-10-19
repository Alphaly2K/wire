# 消息队列与计时器调用边界

`win32/catalog/messages.rs` 声明 22 项消息、消息计时器及 queue metadata 契约，集中 provider、ABI、DWORD/指针宽度和执行策略。五项 PE32 metadata/queue-status 继续 `LegacyUnimplemented`，六项 Win64 timer/wait/queue-status 继续 absent；声明契约不增加原来的能力。

PE32 `user32/messages.rs` 协调既有队列、消息计时器、窗口过程、键盘转换及调度器，返回 `GuestCallOutcome`。正常返回才提交 API 返回帧，阻塞仍停留在原 stub 等待重新分发；TIMERPROC 和 WNDPROC 的非局部转移保持原处理。Get/Peek/Wait 和对象消息等待在读取参数前继续服务同步发送的消息。对象消息等待暂留 API 的进程编排位置，使用 typed ID 选择参数布局并复用原对象 acquire/deadline 规则。

Win64 冷绑定保存 `ControlImport::Message(id)`。普通消息操作可以返回值或阻塞；DispatchMessage 使用窗口协调器返回的显式 callback/resume 控制结果，保留原两次 frame 检查。旧 USER/message 的 arity、DWORD 和 provider 名单已删除。运行时名称继续用于 trace，不用于选择消息行为。

队列输出与 metadata 的提交顺序保持各架构旧行为：PE32 在输出检查前清除 change bits，输出失败仍保留消息；Win64 先完成受检 MSG 输出，再提交 metadata、移除消息和消费硬件键盘状态。消息过滤、WM_QUIT 绕过范围过滤、同步专用 post 限制、PostQuitMessage trace 返回和 DWORD exit-code 都保持原差异。A/W 提取共享原 MSG 布局；TranslateMessage 继续按实际窗口过程的编码产生字符。

本批只收敛调用契约和过程控制。PE32 消息/计时器仍由 User32 持有，Win64 仍由进程 User 持有；宿主事件通过已有 owned event 边界进入，guest 参数不转成可直接解引用的宿主指针。

PE32 `user32/queues.rs` 随后收敛队列资源操作：计时器刷新、post/host 入队、change bits 与消息等待唤醒只借 User32、LastError、Sched 和原 start 时间值，不借 ApiState 或 CPU。Get/Peek/Wait 与对象消息等待使用同一 prepare_queue，保留 timer→过期 synthetic paint 清理→新增 paint 的顺序及两次时间取样；仍使用原队列，没有额外分配或查表。输出 fault、keyboard 消费与实际阻塞/回调继续由原调用 adapter 处理。

owner 基线 `145a271`。两份原 prepare 主体归一一致，timer/host/put/mask 策略及时间取样对照保持。完整 release Win32 255、fmt/release lib/bin strict Clippy/build 通过，证据 `wire-out/refactoring-2026-10-14/r3-user-queues32-{source-checks,verification}.json`；不跑性能或游戏矩阵。

独立基线与验收见[本批记录](../reports/refactoring-messages-2026-10-18.md)。
