# Wire 文档

当前目标、进度与验收条件只在 [GOAL.md](../GOAL.md) 维护。完整[接口迁移清单](api-migration.json)记录两种 ABI 的契约、绑定、实现位置和能力缺口，可运行 `python3 tools/probes/audit_api_migration.py` 重新生成。工具以版本化旧接口基线保留原有全集，并查询当前编译后的契约与实际 Wasm 绑定；旧声明丢失或旧策略变化会阻止覆盖台账。`legacy_sources` 是历史出处，`implementation_sources` 和 `wasm_adapter` 指向现行实现。执行与复用边界见[Wasm 运行时](architecture/wasm-runtime.md)。

以下文档描述已有兼容组件的契约及限制，供迁移时核对；新运行时的接入状态以 GOAL.md 为准。

| 兼容组件契约 |
| --- |
| [PE32 ABI 来源](architecture/abi.md) · [地址等待与线程状态边界](architecture/address-waits.md) · [基础字符串、指针与进程查询边界](architecture/basic-helpers.md) |
| [BCrypt RNG](architecture/bcrypt.md) · [CryptoAPI 上下文与哈希](architecture/crypto.md) · [BSTR 契约与资源归属](architecture/bstr.md) · [日历与宿主计时调用边界](architecture/calendar.md) |
| [Wasm 回调执行边界](architecture/callbacks32.md) · [窗口类注册与元数据](architecture/classes.md) · [时钟与平台采样](architecture/clocks.md) |
| [COM、任务内存与 GUID](architecture/com-core.md) · [进程身份与 LastError 调用边界](architecture/context.md) · [Critical section 调用与调度边界](architecture/critical-sections.md) |
| [PE32 CRT 调用与复用契约](architecture/crt32.md) · [游标状态与架构边界](architecture/cursor.md) · [PE32 D3D9 调用与资源契约](architecture/d3d9.md) |
| [Sleep 与线程让出边界](architecture/delays.md) · [桌面设置与固定输入时间](architecture/desktop.md) · [Win64 设备发现与通知](architecture/devices.md) |
| [对话框控件与窗口枚举](architecture/dialog.md) · [PE32 DirectSound 与 PCM 输出](architecture/directsound.md) · [虚拟桌面与显示器](architecture/display.md) |
| [环境变量与环境块](architecture/environment.md) · [客体异常投递与 CONTEXT](architecture/exceptions.md) · [文件状态与标准句柄](architecture/files.md) |
| [GDI DC 与设备能力](architecture/gdi-dc.md) · [GDI 对象、像素与位图](architecture/gdi-operations.md) · [PE32 GDI+ 状态与调用边界](architecture/gdiplus.md) |
| [矩形与窗口边框](architecture/geometry.md) · [图形创建与 COM 契约](architecture/graphics-contracts.md) · [Heap 调用与资源边界](architecture/heaps.md) |
| [图标与光标资源边界](architecture/images.md) · [SRW、条件变量与 InitOnce 边界](architecture/inline-sync.md) · [宿主键鼠事件](architecture/input-events.md) |
| [Raw input 与 IME 边界](architecture/input-profile.md) · [Interlocked 与 SLIST 边界](architecture/interlocked.md) · [PE32 kernel 对象所有权](architecture/kernel-objects32.md) |
| [键盘状态与布局边界](architecture/keyboard.md) · [Local/Global 内存调用边界](architecture/local-memory.md) · [TLS/FLS 调用边界](architecture/locals.md) |
| [PE32 菜单契约与资源归属](architecture/menus.md) · [消息队列与计时器调用边界](architecture/messages.md) · [模块身份与生命周期](architecture/modules.md) |
| [多媒体计时器调用与资源边界](architecture/multimedia.md) · [Mutex 调用边界](architecture/mutexes.md) · [NLS / locale 边界](architecture/nls.md) |
| [绘制区域与 paint DC](architecture/painting.md) · [路径辅助与通用句柄](architecture/paths-handles.md) · [进程句柄与查询](architecture/process-control.md) |
| [进程错误模式与启动属性](architecture/process-options.md) · [命令行、启动信息与处理器能力](architecture/process-startup.md) · [进程内注册表](architecture/registry.md) |
| [PE32 资源查询与字符串输出](architecture/resources.md) · [精确标量 SSE 边界](architecture/scalar-simd.md) · [Shell 目录、命令行与错误消息](architecture/shell-and-messages.md) |
| [SID 调用与字节布局](architecture/sids.md) · [Win64 Toolhelp 快照资源边界](architecture/snapshots.md) · [SSPI 服务发现](architecture/sspi.md) |
| [事件与信号量调用边界](architecture/sync-objects.md) · [系统与临时目录查询](architecture/system-paths.md) · [系统信息与内存状态调用边界](architecture/system-profile.md) |
| [线程执行与生命周期](architecture/thread-control.md) · [线程句柄与属性](architecture/thread-properties.md) · [Waitable timer 调用边界](architecture/timers.md) |
| [查询 token 与进程身份](architecture/tokens.md) · [ETW provider](architecture/tracing.md) · [可选系统后端的不可用协议](architecture/unavailable-endpoints.md) |
| [USER 注册名称与窗口属性](architecture/user-objects.md) · [Variant 调用与资源边界](architecture/variants.md) · [SAFEARRAY 与元素所有权](architecture/safe-arrays.md) · [文件版本资源边界](architecture/version-resources.md) |
| [Windows version 边界](architecture/version.md) · [虚拟内存调用边界](architecture/virtual-memory.md) · [线程对象等待](architecture/waits.md) |
| [Direct3D → WebGPU 上游复用评估](architecture/webgpu-upstream-reuse.md) · [窗口生命周期与过程控制](architecture/window-control.md) · [窗口查询、字段与坐标](architecture/window-queries.md) |
| [窗口文本与输入所有权](architecture/window-state.md) · [Windows printf 契约与 PE32 变参](architecture/windows-printf.md) · [Win64 WinHTTP 适配器](architecture/winhttp.md) |
| [WinRT 字符串与激活边界](architecture/winrt.md) · [Winsock 启动、地址值与事件](architecture/winsock.md) · [PE32 WMI COM 资源边界](architecture/wmi.md) |
