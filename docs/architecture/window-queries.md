# 窗口查询、字段与坐标

`win32/catalog/window_queries.rs` 集中声明二十五项 export 的 provider、参数宽度和执行策略。PE32 四项 LongPtr 保留 absent、未知 ABI 与零清栈；GetWindowPlacement 使用明确 typed 错误处理以保留 error120、implemented=false 和原 trace。Win64 原缺失的 FindWindowA/W、GetDesktopWindow、IsZoomed、IsWindowEnabled、MapWindowPoints 继续 absent；其余十九项保留静态 kernel32/kernelbase/user32 与动态 user32 的区别。

PE32 `user32/queries.rs` 的 scalar handler 只借用 User32、受检 guest memory 和 LastError。首次桌面创建需要 module/class 注册，由同模块的小型 coordinator 完成；重复查询直接使用现有窗口。父级坐标计算由 User32 提供，`user32_geometry::client_origin` 兼容 facade 委托该方法，保留环检测、wrapping 加法与原 Hangover LGPL 来源标注。

Win64 的适配位于 `x64/runtime/user/window/queries.rs`。创建、销毁、消息回调、焦点和文本操作继续由原窗口 coordinator 处理。Long 的第三参数仍为 DWORD，Ptr 使用完整 u64；extra 存储允许原 4/8 字节重叠访问。SetWindowLongPtr 的 WNDPROC 校验先于 HWND 校验，PE32 保留先查 HWND 再拒绝空 WNDPROC 的顺序。

| 旧行为 | PE32 | Win64 |
| --- | --- | --- |
| 无效 HWND 的布尔查询 | IsWindow/Visible/Unicode/Iconic/Zoomed/Enabled 保留 LastError | IsWindow 保留；Visible/Unicode/Iconic 设置 1400 |
| GetWindowThreadProcessId 的 PID | 4242 | 1 |
| RECT 输出故障 | 逐 DWORD 提交，保留已写前缀 | 先检查完整 16 字节 |
| 字段宽度 | Long 为 u32，extra 4 字节 | Long 只处理原 style/exstyle 与 4 字节 extra；Ptr 为 u64 与 8 字节 extra |
| 坐标访存故障 | Client/Screen 返回 0、error87；MapWindowPoints 保留 Err | Client/Screen 保留 Err |

这些差异按冻结基线保留。MapWindowPoints 先限制 count≤4096，再计算两端坐标，最后整块读写；GetWindowPlacement 的 Win64 顺序仍为 HWND、长度、完整写权限、min/max 能力检查、输出。独立旧行为测试与候选验收状态见[批次报告](../reports/refactoring-window-queries-2026-10-18.md)。
