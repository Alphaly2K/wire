# PE32 WMI COM 契约

PE32 在 COM 初始化后通过 `CoCreateInstance` 创建本地 WMI locator；需要 CLSID_WbemLocator、IUnknown 或 IWbemLocator，以及进程内或本地 server context。聚合返回 CLASS_E_NOAGGREGATION，未注册类返回 REGDB_E_CLASSNOTREG，未初始化线程返回 CO_E_NOTINITIALIZED。AMD64 当前仍返回未注册类失败。

四种接口共用同一个进程 owner，拥有对象种类、引用计数、枚举游标和各自的 vtable；对象可由同进程线程调用，VM 之间独立。QueryInterface 支持 IUnknown 与当前接口，Release 在引用归零时释放进程堆对象。活动对象最多 4096 个。内部方法仅由 vtable 注册，不能通过公开 DLL 导出名称任意绑定。

IWbemLocator::ConnectServer 接受 `root`、`root\cimv2` 及其本机前缀，拒绝远程命名空间、凭据、authority 和 context。IWbemServices::ExecQuery 仅支持 WQL 的 `SELECT * FROM Win32_OperatingSystem`，允许空白和大小写差异，flags 限 0、0x10、0x20、0x30。其余 provider、查询语言和通知查询返回明确 WMI 错误，接口输出置 NULL。

IEnumWbemClassObject 提供单行快照的 Next、Reset、Skip 和 Clone；克隆拥有独立游标，耗尽或返回不足请求条数时返回 WBEM_S_FALSE。IWbemClassObject::Get 提供 `__CLASS`、Caption、CSDVersion、OSArchitecture、Version、BuildNumber 六项固定虚拟系统字符串，以 VT_BSTR VARIANT 返回。BSTR 与公开 automation 接口共用进程堆和成员记录，可由 VariantClear 或 SysFreeString 释放；返回的是虚拟 profile，不查询宿主机器。

四套 vtable 保留全部 65 个声明与 stdcall 宽度，其中 45 项当前返回 E_NOTIMPL；属性写入、属性枚举、异步、远程与方法调用均未实现。通知查询没有事件 provider。声明、调用入口与实际能力分别记录在 [接口台账](../api-migration.json)。

参数、首返回帧与全部实际输出先验证，之后才发布对象、增加引用或推进游标。非 NULL 访存故障交还 JIT，修复后可重试；NULL 必需指针返回 E_POINTER。输出可以覆盖输入或原返回槽，调用仍使用已保存的参数与返回位置。HRESULT 不改写 LastError，不采样宿主时间。服务成功完成计一个 work，不增加客体退休数；静态启动拒绝、进程退出及 VM 销毁清理 WMI owner。

新旧入口共用 `src/win32/com/wmi.rs` 的处理体。新适配器仅提供当前 VMM、进程堆、BSTR 和逻辑网关，不引用旧 CPU 或执行 IR。`node crates/wire-wasm/tests/wmi.mjs` 使用稳定项目输入验证实际 JIT 调用、四套 vtable、查询/枚举、释放和故障恢复，不依赖游戏或本机文件。
