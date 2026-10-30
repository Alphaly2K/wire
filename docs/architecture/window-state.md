# 窗口文本与输入所有权

Wasm 入口在 PE32/Win64 通过 user32 接入 `WindowStateApi` 的 17 项接口。ABI 复用 `win32/catalog/window_state.rs`；契约目录保存历史策略，当前 provider 资格由服务实现核对。

Set/GetWindowTextA/W 和 GetWindowTextLengthA/W 在两种 ABI 都调用实际 WNDPROC，分别发送 WM_SETTEXT、WM_GETTEXT 和 WM_GETTEXTLENGTH。先验证完整 HWND 和所属线程，返回使用原 API 的参数数量和返回约定；应用可覆盖文本处理，默认过程使用 CP932/UTF-16。GetWindowText 的 signed capacity 非正时返回零，不访问输出。跨 A/W 字符串转换与跨线程发送尚未接入，返回 50；错误 HWND 返回 1400。调用已接受后的输出故障保留已执行的过程，修复后不重放回调。

Get/SetFocus、Get/SetActiveWindow 和 Get/SetCapture 使用每线程状态，Get/SetForegroundWindow 使用进程的逻辑前台窗口。SetFocus、SetActiveWindow、SetCapture 可用 NULL 清除，SetForegroundWindow 的 NULL 返回 1400。ReleaseCapture 返回 BOOL 1；SetForegroundWindow 成功返回 1，其余 setter 返回原 owner。GetLastActivePopup 当前返回有效的输入 HWND，尚无 owned popup 树；涉及其他线程的文本或所有权操作明确返回 50，跨线程前台切换仍待接入。

AllowSetForegroundWindow 返回 FALSE/错误 50，保持现有前台状态；跨进程前台权限转移尚未实现。

首次通知 frame 可写、目标过程有效后才接受转换。owner 在执行通知前提交：focus 变化发送旧窗口 WM_KILLFOCUS、新窗口 WM_SETFOCUS；capture 变化只给旧窗口发送 WM_CAPTURECHANGED；active/foreground 变化发送旧/新窗口 WM_ACTIVATE。重复设置不发送通知。每条通知执行前重查窗口和当前过程，因此前一条回调中的 subclass 对下一条生效，销毁下一目标会跳过该通知。回调期间的重入转换保留其新结果；故障或非局部转移不回滚已提交 owner。

窗口销毁清除所有指向它的 focus/active/capture/foreground，线程退出清理其状态。完整销毁/退出通知、foreground 限制策略、child focus 和跨线程 sent-message 调度仍待迁移。

宿主 WM_SETFOCUS/WM_KILLFOCUS、WM_ACTIVATE/WM_ACTIVATEAPP 在入队时更新对应线程或进程状态。键盘输入路由到该线程 focus，鼠标输入路由到 capture；capture 转发把 screen 快照换算为接收窗口的 client lParam，滚轮继续使用 screen 坐标。完整 HWND、批次预检、位置快照与事件字段见[宿主输入](input-events.md)。
