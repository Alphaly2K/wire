# 窗口生命周期与过程控制

`win32/catalog/window_control.rs` 是十七项创建、销毁、默认过程、过程转发、显示/更新、启用、父窗口、位置和同步发送接口的唯一 provider、参数宽度及架构策略声明。PE32 的 ABI 清栈和 Win64 的 DWORD 截断由契约生成；Win64 静态兼容 kernel32/kernelbase/user32，动态查询保留仅 user32 的范围。

PE32 `user32/control.rs` 仍协调创建、销毁和同步发送，其余几何/更新已拆为请求：`user32/window_geometry.rs` 只借 USER、内存、scratch 与 LastError，`x86/process/window_geometry.rs` 执行启用、位置和绘制通知。只有正常返回才由 API 层完成返回帧，阻塞或非局部转移继续交给进程编排。创建保持 WM_NCCREATE/WM_CREATE 和原清理顺序，后续通知重新读取窗口过程；ShowWindow 和 DestroyWindow 在回调前提交原状态，回调失败时保留该状态。MoveWindow/SetWindowPos 的 guest WINDOWPOS 可在回调中修改，之后才提交位置并发送 WM_WINDOWPOSCHANGED，继续使用操作开始时的过程快照。同步发送保留跨线程排队、唤醒及重新分发。

默认窗口过程复用原文本、非客户区、背景和绘制实现。UpdateWindow 在 PE32 上按原 update region 决定是否执行 WM_PAINT；RedrawWindow 的同步更新也经过同一协调器。窗口记录仍由既有 User32 资源持有，本批收敛控制入口，不重排资源生命周期。

默认 WM_WINDOWPOSCHANGED 由 USER 准备 WM_MOVE，然后 Process 执行；执行后重新向 USER 取得当前窗口、过程、style 和 client size，再发送 WM_SIZE。初始 flags 继续取自回调前的 WINDOWPOS。`user32_common.rs` 只保留文本与内存 codec，不依赖 CPU/ApiState，闲置的私有 client_origin 转发入口删除。

`user32/lifecycle.rs` 的 User32::remove_window/exit_thread 统一管理窗口与线程退休，只借 Graphics、Menus 和 Sched；清理不借 ApiState、guest memory 或 CPU。原 keyboard→待处理 send 完成/唤醒→graphics→focus/active/foreground/capture→window/menu→timer/queue 清理，以及退出后的 cursor/keyboard/thread queue 清理次序保持。创建失败、DestroyWindow 和 Process 线程退出直接调用同一 owner 方法。基线 `5af8a4b`；完整 release Win32 255、fmt/release lib/bin strict Clippy/build 通过，覆盖跨线程 send 在窗口销毁或 owner 退出时唤醒，证据 `wire-out/refactoring-2026-10-14/r3-user-retirement32-{source-checks,verification}.json`。本批不跑性能或游戏矩阵。

Win64 在冷绑定时把需要过程执行器的操作保存为 `ControlImport::Window(id)`，执行时返回显式 `WindowResult`，由进程处理 callback/resume。先验证执行器及 callback frame，再检查 HWND、参数和过程，保持故障顺序。UpdateWindow 保留原 scalar 行为，不新增 frame 检查；DefDlgProcA/W、EnableWindow、SetParent 保持 absent。DispatchMessage 暂留消息领域，进入 typed SendMessage 协调器时继续执行原有的第二次 frame 检查。

两架构的现有差异继续保留：PE32 ShowWindow 将任意非零模式视为显示，且不要求 owner；Win64 只接受既有模式。PE32 支持原 GDI update region 与同步跨线程发送；Win64 保留原错误及能力范围。后续能力扩展需独立测试和验收，不借本次重构改变行为。

验证与性能证据见[本批记录](../reports/refactoring-window-control-2026-10-18.md)。

几何请求基线 `c8c735e`，新增独立 guest fixture 在迁移前固定 WM_MOVE 替换过程并最小化、WM_SIZE 使用新过程/状态。原 parent、enable precommit、position prepare/commit、update、text/codec 与 frame 核对保持，release 完整 Win32 258、IR 31、fmt/release lib/bin strict Clippy/build 通过。删除闲置 facade 后重跑必要构建，首次 dead-code Clippy 日志保留；不跑性能或游戏矩阵。证据 `wire-out/refactoring-2026-10-14/r4-window-geometry32-{source-checks,verification}.json`。
