# 消息队列与计时器调用边界

`win32/catalog/messages.rs` 声明 22 项消息、消息计时器及 queue metadata 契约，集中 provider、ABI、DWORD/指针宽度和执行策略。五项 PE32 metadata/queue-status 继续 `LegacyUnimplemented`，六项 Win64 timer/wait/queue-status 继续 absent；声明契约不增加原来的能力。

PE32 `win32/user/pe32/messages.rs` 由 User32 处理既有队列、消息计时器和键盘转换，只借内存、LastError、scheduler 和 start 时间，返回值、等待期限或 dispatch 请求；不借 CPU、ApiState 或 guest 执行器。`x86/process/messages.rs` 结合 Presenter 的 16ms 轮询上限阻塞当前线程，或执行 TIMERPROC/WNDPROC。正常返回才提交 API 返回帧，阻塞仍停留在原 stub 等待重新分发，非局部转移保持原处理。Get/Peek/Wait 和对象消息等待在读取参数前继续服务同步发送的消息。对象消息等待暂留 API 的进程编排位置，使用 typed ID 选择参数布局并复用原对象 acquire/deadline 规则。

跨线程同步发送的接收编排位于 `x86/process/messages.rs`：User32 的 sent owner 选择请求并先标记 processing，Process 执行 WNDPROC，正常返回后再由 owner 检查发送线程退出、提交 result 并唤醒 sender。sender 已退出时沿用先删请求的策略；Transferred/Exited 或 callback fault 保留原提前离开状态。选择/完成方法只借 User32 和 scheduler，不借 CPU 或 ApiState，也不新增队列分配。

Win64 冷绑定保存 `ControlImport::Message(id)`。普通消息操作可以返回值或阻塞；DispatchMessage 使用窗口协调器返回的显式 callback/resume 控制结果，保留原两次 frame 检查。旧 USER/message 的 arity、DWORD 和 provider 名单已删除。运行时名称继续用于 trace，不用于选择消息行为。

Win64 普通消息由 User 准备值、错误或等待请求，只借内存、只读 Threads 和原 start 时间；Runtime64 协调器在错误时写 LastError，在等待时阻塞。错误槽仍按需读取，纯 metadata 查询不提前访问 TEB；调度器通过 User 查询队列是否就绪。过滤、受检输出、metadata、移除及硬件键盘消费保持原顺序。

队列输出与 metadata 的提交顺序保持各架构旧行为：PE32 在输出检查前清除 change bits，输出失败仍保留消息；Win64 先完成受检 MSG 输出，再提交 metadata、移除消息和消费硬件键盘状态。消息过滤、WM_QUIT 绕过范围过滤、同步专用 post 限制、PostQuitMessage trace 返回和 DWORD exit-code 都保持原差异。A/W 提取共享原 MSG 布局；TranslateMessage 继续按实际窗口过程的编码产生字符。

本批只收敛调用契约和过程控制。PE32 消息/计时器仍由 User32 持有，Win64 仍由进程 User 持有；宿主事件通过已有 owned event 边界进入，guest 参数不转成可直接解引用的宿主指针。

2026-10-20 新增的[键鼠事件](input-events.md)在 PE32 消息中也保存入队时的 MSG.pt，不再固定输出零坐标；posted、timer、paint 与 translated 消息使用各自产生时的逻辑游标快照。Win64 继续保留消息位置快照。两端输出检查与移除/键盘消费顺序保持，PE32 五项 metadata/queue-status 的 ABI-only 策略未扩展。

PE32 `win32/user/pe32/queues.rs` 收敛队列资源操作：计时器刷新、post/host 入队、change bits 与消息等待唤醒只借 User32、LastError、Sched 和原 start 时间值，不借 ApiState 或 CPU。Get/Peek/Wait 与对象消息等待使用同一 prepare_queue，保留 timer→过期 synthetic paint 清理→新增 paint 的顺序及两次时间取样；仍使用原队列，没有额外分配或查表。输出 fault 和 keyboard 消费归 USER，实际阻塞/回调归 Process。

owner 基线 `145a271`。两份原 prepare 主体归一一致，timer/host/put/mask 策略及时间取样对照保持。完整 release Win32 255、fmt/release lib/bin strict Clippy/build 通过，证据 `wire-out/refactoring-2026-10-14/r3-user-queues32-{source-checks,verification}.json`；不跑性能或游戏矩阵。

独立基线与验收见[本批记录](../reports/refactoring-messages-2026-10-18.md)。

sent 请求提取基线 `683f025`，selection/processing 和 completion/wake 的原主体核对保持；完整 Win32/IR 与构建结果见 `wire-out/refactoring-2026-10-14/r4-sent-monitor32-{source-checks,verification}.json`。普通 callback 编排提取不重复性能矩阵。

消息请求提取基线 `d541e63`，原 timer/post/filter/output/keyboard、deadline、dispatch 输入与 sent loop 对照保持。连续重构阶段整合通过 release 全目标 692 passed / 7 ignored（21 suites）、all-targets/all-features strict Clippy、build 和 fmt。未改执行循环/frame，不跑性能或游戏矩阵，证据 `wire-out/refactoring-2026-10-14/r4-messages32-{source-checks,verification}.json`。

Win64 owner 基线 `c551314`，消息处理、host 入队和窗口协调器对照保持；首次测试发现错误槽读取提前，改为显式错误结果后，release 全目标 694 passed / 7 ignored（21 suites）、all-targets/all-features strict Clippy、build 与 fmt 通过。证据 `wire-out/refactoring-2026-10-14/r3-user-messages64-{source-checks,verification}.json`。
