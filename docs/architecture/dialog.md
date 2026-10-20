# 对话框控件与窗口枚举

`win32/catalog/dialog.rs` 为 24 项 PE32 export 提供唯一的 DLL、参数、清栈和 typed ID 声明，Win64 全部 absent。原 22 项控件/枚举已随日志批次及累计复核接入，见[批次报告](../reports/refactoring-dialog-2026-10-19.md)与[当前状态](../status.md)。随后两个 MessageBox 入口仅做普通契约迁移，不重复性能矩阵。

MessageBoxA/W 由 `win32/x86/dialog.rs` 仅借 GuestMem 读取原文本和 caption，返回 IDOK 并记录 `SHOWN` 诊断；忽略 flags、NULL 和读故障的原文本策略保持，没有新增 host 对话框。原公开 user32::dispatch facade 继续排除这两个入口，普通 gateway 使用冷绑定 ID。

`win32/user/pe32/dialog.rs` 仅借 User32、内存、scratch、当前 tid 和 LastError，准备控件消息或窗口快照，不依赖 CPU、ApiState 或 callback 执行器。`x86/process/dialog.rs` 消费请求并执行应用 WNDPROC/枚举回调，保留 Returned、阻塞及非局部控制转移；API 入口处理返回 frame 与 trace。ApiState 的小型组合方法只拆分资源借用，scratch 游标仍由原 owner 私有持有。GetDlgItemInt 在调用 WM_GETTEXT 前清零 translated 输出，回调正常返回后才由原纯内存解析写入最终 translated 标志。child 或线程错误先于输出访存。

顶层枚举先拒绝空 callback，反序快照只选顶层、非 message-only 窗口，调用前再次排除已销毁窗口，callback FALSE 直接作为返回值。子窗口枚举继续使用原递归快照顺序和去重，FALSE 停止后返回 carrier 仍为 1；空快照也继续成功。这些历史差异分别固定，迁移时保持。

DialogBoxIndirectParamW、CreateDialogIndirectParamW、MapDialogRect、IsDialogMessageW、CreateIconIndirect、SetWindowPlacement、GetScrollInfo、SetScrollInfo、LockWindowUpdate 仍明确不可用。它们由 typed 领域拒绝，以保留错误 120、原 note 和 unimplemented trace；不借此新增 UI 能力。原公开 `user32::dispatch` 可保留冷路径兼容委托，正常 API 网关直接使用绑定好的领域 ID。

Process 请求提取基线 `3217b11`。child 查找、消息准备、数字转换、两个快照算法和两种 callback 循环分别核对保持，原 frame/run_loop 未改。正确性沿用独立 fixture 的 NULL/返回 carrier、部分输出、快照销毁与 callback 退出覆盖，不重复性能或游戏矩阵；证据 `wire-out/refactoring-2026-10-14/r4-dialog32-{source-checks,verification}.json`。
