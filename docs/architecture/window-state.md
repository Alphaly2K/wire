# 窗口文本与输入所有权

`win32/catalog/window_state.rs` 是十六项 export 的 provider、ABI、参数宽度与执行策略来源。两架构保留各自已有能力：PE32 全部 typed，Win64 的 SetActiveWindow、GetLastActivePopup 和三项 capture 接口仍 absent。Win64 静态 kernel32/kernelbase 别名与动态 user32 范围分别声明。

PE32 的 `user32/window_state.rs` 仅由 User32 准备结果、文本消息或固定大小的所有权通知，不依赖 Cpu、ApiState 或 callback 执行器。它只借当前 tid 与 LastError，先校验目标并提交 focus/capture/active/foreground。`x86/process/window_state.rs` 执行回调；逐条取得当前 WNDPROC，保留重入期间销毁窗口、替换过程和非局部控制转移的顺序。callback 故障不回滚已提交所有权。NULL setter 仍可清除，重复 setter 不发送通知，ReleaseCapture 与 SetForegroundWindow 仍返回 BOOL 1。

PE32 文本操作按 HWND、所属线程顺序验证，然后发送 WM_SETTEXT/WM_GETTEXT/WM_GETTEXTLENGTH。A/W 的文本解释继续由实际 WNDPROC 决定，不能直接读写 `Window.text` 来代替应用程序回调。默认文本处理与尚未迁移的 dialog wrappers 保留原入口和 Wine LGPL 来源说明；旧的普通文本/焦点名称路由与重复 ABI 声明删除。

Win64 的 `x64/runtime/user/window/state.rs` 由 User owner 直接处理，只借内存和当前 LastError 地址，不再依赖 Runtime64；保留已有 scalar title 和 ownership 行为，使用固定 typed ID。getter 在查询 owner 时不查 HWND；setter 先验证 HWND，NULL 返回 1400 并保持原 owner。普通 A/W 文本分别使用 CP932/UTF-16，capacity 保持 DWORD 截断，输入故障前不更改 title，输出故障保留 Err 和既有返回帧提交策略。与窗口查询一起完成 owner 验收，见 `wire-out/refactoring-2026-10-14/r3-user-scalar64-{source-checks,verification}.json`。

独立回归与候选验收状态见[批次记录](../reports/refactoring-window-state-2026-10-18.md)。

Process callback 编排提取基线 `d883866`。迁移前增加独立 guest 回调，分别替换或销毁下一通知窗口，固定逐条重查语义；原所有权转换、文本检查、getter、错误映射与通知循环核对保持。callback frame 和 Process 执行循环未改，普通边界提取不重复性能矩阵。当前证据见 `wire-out/refactoring-2026-10-14/r4-window-state32-{source-checks,verification}.json`。
