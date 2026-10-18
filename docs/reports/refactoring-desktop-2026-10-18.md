# R2/R3 进程桌面设置与 DWM 查询（2026-10-18）

状态：五项独立旧行为基线与原创查询 fixture 已通过，运行时尚未接入，不计入已验收累计数。

六项范围：SystemParametersInfoA/W、GetDoubleClickTime、GetDialogBaseUnits、DwmGetWindowAttribute、DwmIsCompositionEnabled。PE32 前两项 ABI-only、两个固定值 typed、DWM 保留 unknown ABI/零清栈；Win64 两项设置与两项 DWM 已有处理，两个固定值 absent。

Win64 SystemParametersInfo 静态支持 kernel32/kernelbase/user32，默认动态表缺失，保留已注册 gateway fallback；DWM 静态支持上述三个 provider 加 dwmapi，动态 user32/dwmapi。不依据 Windows 名义 DLL 收窄原虚拟 provider 范围。

拟将 screen-reader 标量移入私有进程桌面设置 owner，固定计时/字体单位成为纯 profile；DWM 保留窗口存在性、HRESULT、NULL/尺寸检查、LastError、checked output 与返回帧顺序。已有真实窗口 DWM frame bounds 回归继续保留。

原创可选 `gateway-desktop`：Win64 重复读取 screen-reader 设置并消费 BOOL 与实际输出，每轮预写 sentinel；PE32 重复 GetDoubleClickTime 并累加。资源恒定。旧 GDI DC 冻结双架构 12K 实跑通过：Win64 retired 11n+10/work 12n+10，PE32 retired 4n+7，API 均为 n；PE32 IR/raw 均通过，工具 20 项通过。证据 `r2-desktop-baseline{32,64}-tests.txt`、`r2-desktop-fixture-baseline{32,64}`、`r2-desktop-tool-baseline.txt`。

before 为 `r2-gdi-dc-wire/wire`，SHA256 `b94fa7683a3625b801a703ce34a0abef6b724c3fc010604df35e692c620ebaf9`，5,960,776 bytes。文件预算 +24KiB，证据根 `wire-out/refactoring-2026-10-14/`。前批计时、游戏与本批编译严格串行。
