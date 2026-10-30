# 游标与鼠标指针

Wasm 入口在两种 ABI 的 user32 接通 ShowCursor、Get/SetCursor、Get/SetCursorPos、ClipCursor、GetClipCursor、TrackMouseEvent、EnableMouseInPointer、IsMouseInPointerEnabled、GetPointerType、GetPointerInfo 和 GetPointerDevices。其它 DLL 的历史私有别名不在新入口导出范围内。

游标句柄、位置和每线程显示计数复用 `win32/user/cursor.rs`，计数采用有符号饱和运算，线程退出清理本线程记录。位置与消息使用同一份状态。SetCursor 接受 NULL；图像加载服务迁移后才会发布可用的非空游标句柄，当前非空值返回错误 6。

未设置裁剪时保留宿主输入和位置 setter 的有符号坐标。非空 ClipCursor 与逻辑桌面取交集，空交集返回错误 87 并保持旧状态；成功后立即约束当前位置，后续宿主鼠标事件及 setter 都按右/下边界不包含的规则裁剪。NULL 解除裁剪；GetClipCursor 在无裁剪时返回逻辑桌面范围。裁剪后的客体消息坐标按目标窗口转换，捕获重定向共用同一位置。这里只改变客体输入，不移动宿主桌面指针。

鼠标指针提升直接复用 `win32/user/mouse_pointer.rs`。EnableMouseInPointer 在进程中只配置一次，重复调用返回错误 5；启用后宿主鼠标事件提升为 WM_POINTER，并进入 QS_POINTER 类别。MSG 输出成功后保存当前线程的指针快照，移除消息时按原鼠标输入更新按键状态。GetPointerType/Info 只接受已取出快照的 ID 1；窗口销毁及线程退出移除相关快照。POINTER_INFO 分别写出 PE32 的 88 字节和 AMD64 的 96 字节结构，保留实际坐标、时间、按钮状态和完整 HWND。

TrackMouseEvent 读取完整的 16/24 字节结构，支持查询和取消空 tracking 状态；实际 hover/leave 注册返回错误 50。GetPointerDevices 返回空 digitizer 列表，没有触摸、笔或触摸板设备。所有输出受统一 VMM 检查，输出故障保持原返回帧、计数和状态；这些 API 不采样时钟或随机源。

检查入口为 `node crates/wire-wasm/tests/cursor.mjs`，只使用项目生成 PE 和显式宿主输入，覆盖两种 ABI、裁剪与消息坐标、指针布局、QS_POINTER、状态隔离、寿命及跨页故障。消息、绘制和旧入口游标回归分别验证共享输入行为与原有适配器。
