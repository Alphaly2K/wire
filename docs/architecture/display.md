# 虚拟桌面与显示器

新 Wasm 入口在两种 ABI 接通 user32 的十六项显示入口，包括设备/模式枚举、MonitorFromPoint/Rect/Window、GetMonitorInfoA/W、GetSystemMetrics、EnumDisplayMonitors 和三项 CCD 查询。新入口不接受旧适配器的 kernel32/kernelbase 别名。客体使用单个逻辑桌面：1024×768、32-bit、60 Hz，工作区等于显示器边界；查询不读取宿主桌面或创建宿主呈现窗口。

EnumDisplayDevicesA/W 枚举一个 adapter 和一个 monitor child。默认设备名称为 `\\.\DISPLAY1`，child 位于该名称下；adapter/monitor 的状态位分别为 5/3。输出 cb 必须为 424/840，索引超出范围或名称不匹配返回 FALSE 并保持输出与 LastError。EDD_GET_DEVICE_INTERFACE_NAME 当前返回错误 50，不提供虚构的设备接口路径；其他非法 flags 返回 87。

EnumDisplaySettingsA/W 和 Ex 接受索引 0、ENUM_CURRENT_SETTINGS、ENUM_REGISTRY_SETTINGS，名称可为 NULL/空或默认设备名称。DEVMODE 的 dmSize 接受 ANSI 124..156、Wide 188..220 字节；完整输出仅写该长度，模式字段发布 32bpp、1024×768、60 Hz 和零旋转。Ex 的合法标志为 EDS_RAWMODE（2）和 EDS_ROTATEDMODE（4）及其组合；其他标志返回错误 87。当前没有模式切换或热插拔。

HMONITOR 是进程内稳定的完整指针宽度身份，不能当作客体地址访问。MonitorFromPoint 的 POINT 为按值八字节结构；PE32 占两个栈 word，AMD64 使用一个 packed 参数。命中规则使用 signed LONG 和非空交集；越界时按 DEFAULTTONULL/DEFAULTTOPRIMARY/DEFAULTTONEAREST 返回零或唯一显示器，非法 flags 返回错误 87。MonitorFromWindow 验证完整 HWND，错误 1400；NULL HWND 按未命中处理。GetMonitorInfo 接受 MONITORINFO（40 字节）及对应 A/W 扩展（72/104 字节），无效显示器身份返回 1461，非法 cbSize 返回 87。

EnumDisplayMonitors 当前支持 NULL HDC，非零 HDC 返回 FALSE/错误 50。可选 RECT 与显示器求交，空交集成功且不调用回调；NULL 回调返回错误 87。一次有效枚举通过同一 Wasm JIT 调用 MONITORENUMPROC，传递完整 HMONITOR、NULL HDC、临时裁剪 RECT 指针及完整 lParam。RECT 位于当前客体栈，仅在回调期间有效。回调的 BOOL 决定枚举结果，AMD64 仅检查低 DWORD；原调用上下文在返回后恢复，保留累计 work/退休和内存变化。回调可嵌套、让出、加载 DLL、在 DllMain 中调用或退出线程，异常搜索与展开沿每线程的统一续点处理。最多嵌套 32 层；首个 RECT 和调用帧均可写后才进入回调。

GetDisplayConfigBufferSizes、QueryDisplayConfig 和 DisplayConfigGetDeviceInfo 当前直接返回 ERROR_NOT_SUPPORTED（50），不读取输出指针、不写结果、不修改 LastError。CCD 路径/模式尚未实现，绑定这些入口只提供明确的失败协议。

其余成功查询保持 LastError；普通参数错误通过同一 TEB 错误槽报告。参数与返回帧在输出前保存，跨页输出故障不留下已写前缀；修复后可重试。完成服务计一 work、零客体退休，回调指令另外按实际执行计数。所有显示服务均不采样时钟。

纯 profile 与 Windows 记录编码复用 `src/win32/user/display.rs`，其原许可证及署名保留。浏览器软件窗口与 DXGI/D3D11 WebGPU 提交已接入；原生桌面 compositor、DC 相关显示器枚举、设备接口与 CCD 路径仍待接入。
