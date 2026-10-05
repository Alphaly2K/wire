# 常用 Win32 功能组与主窗口推进

菜单、基础 GDI、常用 USER32、统一 stdcall ABI 已成组实现；后续参考切换为
用户指定的 Hangover，固定提交与许可结论见 [Hangover 路线](hangover-win32-design.md)。
真实游戏已经创建 **TVPMainWindow（HWND=0x40010008）**并输出真实提示页/STAFF像素，
已接入macOS上传呈现；完整可玩M2未验收。最新见[渲染检查点](graphics-progress-2026-10-06.md)。

## 当前实现

- 菜单具有真实句柄、48/44B MENUITEMINFO、mask 更新、复制字符串、子菜单命令查找、
  删除/移除/销毁、窗口菜单归属和循环/深度拒绝。没有菜单交互/UI 绘制。
- GDI 具有 typed DC/bitmap、排他 bitmap 选择和生命周期、COLORREF 像素、
  32bpp BI_RGB DIB section guest 存储、上下行序、GetObject、SRCCOPY BitBlt、
  origin/viewport。768 次逐色探测、overlap/clip/direct guest writes 均验证。
  DIB 受 VMM 内部所有权管理，Guest VirtualFree 不能释放后重用其 storage。
- USER32 常用窗口/控件文本、整数转换、parent、enable、focus/active/capture、
  SetWindowPos/MoveWindow、默认 WM_MOVE/WM_SIZE、坐标/矩形和窗口枚举。
  guest WndProc 控制权转移继续保留，销毁清理输入 owner。
- gateway 扩到 16 DWORD；11 个 Wine spec 统一 typed named stdcall 与明确 ordinal。
  修正原手工表 9 处 ABI，oleaut32 的 #2/#6 可绑定 SysAllocString/SysFreeString。
  ABI 识别仍与实际功能实现分开；未实现服务标记 UNIMPLEMENTED。
- BSTR 真正分配 guest prefix/data，支持 embedded NUL、Len/ByteLen/free/realloc、
  NULL 与自身别名；COM 初始化按线程记录模式/计数，GUID 使用宿主随机源。
  基础 ordinal 字符串比较仅简单大小写映射，不是完整 Windows NLS collation。
- 当前进程句柄/查询、单逻辑 CPU 亲和性和实际 stack 参数检查。
  scheduler 有界扩到128槽位，stack/TEB arena 编译时验证不会碰撞 heap/主 TEB。
  达到容量返回 NULL/ERROR_NOT_ENOUGH_MEMORY，避免 interpreter 内部崩溃。
  exited thread 的槽位循环回收、完整 suspend count 等仍待实现。

## 真实运行证据

全游戏目录38个PE的API导入已成组盘点，诊断存于忽略的 wire-out/common-api-audit。
win32dialog、windowEx、layerExImage、ExtKAGParser、csvParser、kropus、wuvorbis、
extrans、fstat、lineParser 均真正执行 guest DLL 初始化。
TVPMainWindow 注册/创建完成；真实 CreateDIBSection 和相关脚本继续执行。

128线程运行退休 **452,652,969** guest 指令，**1,128,367** API，
IR **404,984,796** / raw fallback **47,668,173**。
游戏仍走错误处理并 ExitProcess(0)：直接触发为 AdjustWindowRectEx 未实现，
后续 MessageBoxW 文本为 Fatal Error / Unknown error。
退出0不能视为启动成功；初始化运行 MIPS 不作为持续性能收益。
亲和性补齐后游戏仍建立100多个线程，旧64槽位限制是独立的资源边界，
不是仅靠亲和性修正就能消除的问题。

日志：wire-out/game-common/{first,affinity,thread-trace,threads128}，不提交游戏资产。
复现：使用 docs/wine-win32-progress-2026-10-06.md 的 run 命令，将预算扩到1.5B，
日志目录改为上述目录。解释器仍为主路径，没有 runtime JIT。

## 接续

检查点验证：完整 **115 项**通过（3 unit、35 CPU differential、5 FPU、
14 IR、1 Wasm、57 Win32），release Clippy all-targets、format、diff检查通过。

补窗口 nonclient/client 几何和 paint 消息、font/DC 对象、DIB blit，
再把真实游戏 surface 接入平台可见呈现。模态 dialog、图标、滚动、
字体资源/字形、完整 monitor/z-order、跨 A/W WndProc thunk 等仍有缺口。
部分函数只提供明确的初始 desktop/input profile，不宣称完整 Win32 已完成。
