# Hangover 参考路线（2026-10-06）

按用户指定，后续 Win32 以 Hangover 为主要参考。固定主仓库提交为
`56e385b7ff490edd281b288f420677093563560b`，Wine 子模块提交为
`3ea3a14941b6a122b892c84141ce9662c008081e`，避免浮动 master。
已验证的 Wine 10.0 菜单/矩阵/窗口逻辑继续保留，不整体换版本。

## 结构与许可核查

当前 Hangover 主仓库主要管理 Wine/FEX/Box64 子模块、构建与分发；
Win32 实现位于它的 Wine 分支，32 位兼容转换主要在 wow64/wow64win。
应用的 x86 代码经模拟器运行，在 NtUser/NtGdi/NT 系统调用或 Wine Unix call
边界退出模拟，调用原生 Wine 后端。它不是一套可单独复制的纯 Win32 shim。

主仓库 LICENSE 是 LGPL 2.1。实际审阅的 wow64win/user.c、gdi.c、
wow64win_private.h、wow64/syscall.c、oleaut32/oleaut.c、win32u/defwnd.c
均明确允许 LGPL-2.1-or-later；选择 LGPL 2.1 与 wire 的 LGPL-2.1-only 兼容。
这一结论限于这些文件，不推及 FEX、Box64 或其他未来移植文件。
实际移植的 BSTR 和默认窗口消息保留版权、原函数摘录、固定提交、许可和 checksum，
见 [来源登记](../third_party/hangover/README.md)。

## 对 wire 的适配

| Hangover/Wine 规则 | wire 实施 |
| --- | --- |
| get_ulong 取 DWORD，get_handle 符号扩展，get_ptr 零扩展 | scalar/handle/pointer 分开识别；伪句柄显式处理；guest 指针经 GuestMem，不能直接转换为宿主地址 |
| MSG32 / CREATESTRUCT32 / WINDOWPOS32 等显式转换 | guest 布局按 DWORD 序列解析；输出先检查完整范围；窗口回调继续接入 guest CPU |
| addr_32to64 + put_addr 处理 pointer-to-pointer | DIB/BSTR 返回真正的 guest 地址；不截断返回 host pointer；失败不伪造对象 |
| 原生 Wine backend 执行服务 | CALL_NATIVE/gateway 进入 wire 的真实对象/内存/文件/栅格后端；不在 x86 解释器中执行整个 Wine |
| 系统服务仍依赖 Wine server、驱动和消息回调 | 平台无关逻辑可逐文件适配；平台调用由 wire 实现，macOS/iOS 生命周期另行接入 |

已有 gateway 缓存由 12 扩到 16 DWORD；known stdcall 只读取实际参数数量，
检查缺页，准确恢复栈。全游戏目录 38 个 PE 的 imports 已按 DLL 汇总，
用固定 Wine spec 统一 named stdcall 和明确数字 ordinal，避免逐插件补导出。
**ABI 已知不代表功能完成**；未实现的对象/图形服务必须明确失败并记录日志。

解释执行仍为主路径；这里不引入 FEX/Box64 JIT 或运行时机器码。
保持 FLAGS 保守分析、fault/预算/线程/回调边界和 x87 精度。
下一步成组补窗口 paint、字体/DC、DIB blit 和真实游戏主窗口呈现。
