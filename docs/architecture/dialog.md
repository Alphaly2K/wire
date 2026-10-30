# 对话框控件与窗口枚举

Wasm 入口在 PE32/AMD64 均接通共享 catalog 的全部 31 项入口。其中十三项提供真实的控件查找、消息包装、整数转换和窗口枚举；其余十八项返回明确的失败和 LastError 120。MessageBoxA/W 没有真实交互，新入口不沿用旧诊断函数固定返回 IDOK 的行为。

GetDlgItem 依次查找直属 WS_CHILD 窗口，以完整父级句柄和 control ID 的低 DWORD 匹配；未找到返回零，不写 LastError。GetDlgCtrlID 从窗口记录返回 ID 的低 DWORD，无效窗口返回零。父子状态、坐标和销毁清理见[窗口生命周期](window-control.md)。数值相同的菜单句柄和 child ID 保持独立。

八项消息包装调用实际 WNDPROC，使用当前线程、同一 JIT/VMM/预算和 USER 回调栈。Get/SetDlgItemText 与 SendDlgItemMessage 保留完整参数、结果和各自 ABI 的返回栈约定；找不到 child 返回 1400，其他线程的控件返回 120，均先于输出访存。跨 A/W 字符串消息暂时返回 50。

GetDlgItemInt 在首个回调 frame 和临时缓冲预检成功后清零 translated，再进入 WM_GETTEXT；回调自身故障时该零值保持可见。最多解析返回的 29 个单位，允许 ASCII 空白、可选符号及十进制前缀；signed/unsigned 范围、负数回绕及失败值由共享 `dialog_values.rs` 计算。SetDlgItemInt 使用同一十进制格式化。新适配按控件注册编码使用 ANSI 或 UTF-16 缓冲，缓冲位于该次客体回调栈，覆盖让出、嵌套调用和 DLL 装载寿命。完成结果在输出故障前冻结，恢复不会重放 WNDPROC；translated 与原返回槽别名也使用预先捕获的调用帧。

EnumWindows/EnumThreadWindows 先拒绝空 callback，再按 HWND 反序建立快照；只选择有线程归属的顶层、非 message-only 窗口，线程零筛选沿用旧全部线程规则。EnumChildWindows 保留原递归快照顺序和去重。调用前重新检查成员是否仍存在，不纳入回调中新创建的窗口。顶层枚举返回最后一次 callback 的 DWORD 结果，FALSE 停止；子枚举停止后仍返回 1，空快照也返回 1。两参数 callback 在调用线程运行，可让出、抛出异常或装载 DLL；下一 frame 故障保留完成结果和游标。每线程最多共用 32 层 USER/其他回调。

DialogBoxParamA/W 和 DialogBoxIndirectParamA/W 没有模态 manager 或模板创建后端，返回完整 INT_PTR -1/错误 120；零属于合法模态结果，不再用于这一失败路径。CreateDialogParamA/W、CreateDialogIndirectParamA/W 和 EndDialog 返回 NULL/FALSE 与错误 120，不创建或结束任何窗口，不访问模板或调用 DLGPROC。MapDialogRect、IsDialogMessageW、CreateIconIndirect、SetWindowPlacement、GetScrollInfo、SetScrollInfo、LockWindowUpdate 仍返回零/120。MessageBoxA/W 也返回零/120；模态模板、导航、图标创建、滚动条与交互对话框仍缺失。

新旧入口共用 `hierarchy.rs` 的 child/快照算法和 `dialog_values.rs` 的十进制规则。`process/window_dialog.rs` 使用当前 USER owner 和两种 ABI 的回调续点，依赖闭包不包含旧 CPU 执行器、IR 或旧内存适配器。
