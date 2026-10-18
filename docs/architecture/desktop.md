# 进程桌面设置与 DWM 查询

`win32/catalog/desktop.rs` 是六项 desktop/DWM API 的 provider、参数与执行策略的权威来源。PE32 只将两个固定 profile 查询接入 typed handler，SystemParametersInfoA/W 保持 ABI-only，DWM 保留 unknown ABI/零清栈；Win64 两项设置与两项 DWM 使用 typed ID，固定查询仍 absent。

`win32/user/desktop.rs::Settings` 私有拥有进程逻辑 screen-reader 设置，由 Win64 User 组合；不访问宿主偏好。默认 false，setter 仅在原支持的 action/flags 验证成功后提交。PE32 固定 double-click 时间与 dialog font units 从同一领域 profile 取值，无复制状态。

Win64 架构协调在 `x64/runtime/user/desktop.rs`：SystemParametersInfo 保留 DWORD 截断、GET 对输出指针的受检写入、SET 忽略 pvParam 的原规则，以及 error50/LastError；DWM composition 明确为关闭，window attribute 仅支持原 frame bounds/no-cloak 子集。DWM 保留 HWND→NULL→attribute/size 的验证顺序及 HRESULT，不修改 LastError。

静态/动态范围有意保留原差异：设置的静态 kernel32/kernelbase/user32 别名可用，默认动态导出表缺口与已有 gateway fallback 保留；DWM 静态还接受 dwmapi，动态接受 user32/dwmapi。PE32 不因 Win64 声明获得 DWM 参数栈信息。

通用调用框架在输出前保存返回帧。独立基线验证跨页写故障不提交 CPU/API、输出覆盖返回槽仍使用已保存 frame、设置被拒绝不变更状态；既有真实窗口回归验证 DWM frame bounds。完整证据见[桌面批次报告](../reports/refactoring-desktop-2026-10-18.md)。
