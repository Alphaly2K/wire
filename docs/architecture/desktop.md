# 桌面设置与固定输入时间

新 Wasm 入口在 PE32/AMD64 提供 user32 的 GetDoubleClickTime、GetCaretBlinkTime、GetDialogBaseUnits 和 SystemParametersInfoA/W。固定逻辑 profile 返回双击间隔 500 ms、caret 偏好 530 ms，以及横向 8、纵向 16 的 dialog base units；不读取宿主偏好。成功计一 work、零客体退休，不采样时钟，保持 LastError 和其余 CPU 状态。

SystemParametersInfoA/W 复用现有 USER Settings。SPI_GETSCREENREADER（0x46）向完整客体指针写入四字节 BOOL；SPI_SETSCREENREADER（0x47）在 fWinIni 为零时按 uiParam 设置该进程的状态，不访问 pvParam。两种字符入口共享状态，进程之间独立。其余 action 或不支持的设置标志返回 FALSE/错误 50，不修改宿主或发布设置变更。

统一调用帧先保存参数和返回值，输出可与返回槽重叠。完整输出或错误槽故障保持 CPU 与进程设置，允许修复后在原网关重试；返回帧不可读时不执行设置。DWORD 参数按 32 位处理，指针保持客体位宽。新入口只接受 user32 provider。

DwmIsCompositionEnabled 和 DwmGetWindowAttribute 在两种 ABI 仅由 dwmapi 提供。前者向四字节 BOOL 写零；后者校验窗口，支持 DWMWA_EXTENDED_FRAME_BOUNDS（9，16 字节 RECT）与 DWMWA_CLOAKED（14，四字节零值）。无 compositor 的虚拟窗口使用现有窗口矩形，未支持属性/错误尺寸返回 E_INVALIDARG，无效 HWND 返回 E_HANDLE；HRESULT 失败保留 LastError。

两种 ABI 的 GetProcessWindowStation、GetThreadDesktop 和 GetUserObjectInformationA/W 共用无后端失败协议，均返回零和错误 120，不访问查询输出及 required-length 指针。当前没有 station/desktop owner 或对象查询能力，不发布 synthetic handle。错误槽故障保持 CPU/work，修复后可重试。这四项仅接受 user32。GetCaretBlinkTime 只提供虚拟设置值，实际 caret 创建、绘制和计时仍缺失。

显示器和模式查询见[虚拟桌面](display.md)，窗口边框与矩形见[几何接口](geometry.md)。实际宿主展示及 compositor 能力单独验收。
