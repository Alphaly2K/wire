# COM 基础契约与状态归属

十项基础 export 的 provider、ABI、参数宽度与执行策略集中在 `win32/catalog/com.rs`：CoInitialize/Ex、CoUninitialize、RoInitialize/Uninitialize、CoCreateInstance、CoTaskMemAlloc/Free、OleInitialize/Uninitialize。Win64 八项 typed，接受 kernel32/kernelbase 的既有静态导入，以及 ole32/combase/WinRT apiset；默认动态查询只声明后三个 provider，已注册 gateway 的旧查询 fallback 保留。Ole 两项在 Win64 absent。PE32 仅 ole32：六项 typed、两项 Ole ABI-only/error 120，Ro 两项 absent。模块自身能否动态装载仍由既有 loader 准入决定。

冷绑定 scalar ComApi，执行时不再查 API 名称。Win64 Reserved、指针和 SIZE_T 保持完整 carrier，CoInitializeEx flags、RoInitialize 类型和 CoCreateInstance class context 截断 DWORD；PE32 继续 stdcall。返回帧、CPU、公共 unblock、计数与 trace 由原进程入口提交。普通 init 返回 S_OK/S_FALSE 或原 HRESULT，Win64 不写 LastError，PE32 失败仍同时写 HRESULT 到 LastError。

`win32/com/apartment.rs` 拥有 mode、COM refs 和 WinRT refs 的纯状态。两类引用分别计数，重复同 mode 初始化增加相应引用并返回 S_FALSE，mode 冲突不增加引用；解除初始化饱和减一，只有两类引用都为零才清 mode。溢出继续由各架构映射原错误。Reserved 和 flags 的校验在架构 adapter 内，先于 owner 更新。

Win64 的每个 Thread 私有拥有 Apartment，活动线程只保存 ID；删除 Runtime64 的 mode/引用镜像和切换时的来回复制。新线程使用独立的空状态。PE32 Apartments32 私有拥有 tid 映射，保留原 tid 身份及 CoUninitialize 删空条目的行为。退出资源次序和 tid 生命周期未重新定义。

Win64 TaskMemory 私有登记 task allocation 的成员，并执行 CoTaskMemAlloc/Free，只借 Memory 与 Pages；free 先检查成员、再检查 backing，unmap 成功后才移除两处记录。StringFromCLSID/IID 和 known-folder 输出使用同一个登记 owner，保留各自原通用分配及登记/输出先后，未合并 Local/Global 或 guest Heap。其余 Win64 COM 调用仅借活动线程 Apartment、内存和 Trace；com 模块不再依赖 Runtime64。PE32 task memory 继续共享进程 GuestHeap，free 忽略无效/空指针，超大分配返回零时保留 LastError。

公开 `nls::dispatch` facade 保留原先 task allocation 的精确小写 ole32 provider 范围、返回值与日志，通过契约 ID 委托 COM task adapter；其它 COM 接口仍不进入此 facade。主调用路径直接使用冷绑定 ID。

Win64 CoCreateInstance 保留受限 missing-factory 行为：空必需参数先返回 E_INVALIDARG；随后预检 output、读 CLSID/IID、记录 trace、写空输出，最后按 apartment 未初始化、context 为零或 class 未注册映射 HRESULT。输出 fault 与返回槽别名顺序保持。PE32 仍委托原 WMI locator factory，其支持范围和输出先后不因契约迁移改变。两架构的工厂能力分别表达。

PE32 [WMI factory 与对象调用](wmi.md)直接借私有 registry、heap、BSTR 和 gateway table；普通方法不再借整个 ApiState，原引用计数、枚举游标与故障提交保持。

独立基线包括真实线程在小/大时间片下的 mode/引用隔离、静态/动态 provider、完整 carrier、输出 fault/alias、task allocator 与 GUID/Local/Heap 的归属。原创 balanced init/uninit fixture 消费第二次初始化的 S_FALSE，并核对每轮四次 API 调用。详见[本批验收](../reports/refactoring-com-core-2026-10-17.md)。

Win64 窄调用提取基线 `82a6949`，task 分配/释放和 apartment/factory body 经显式依赖替换核对保持；原 Apartment 状态实现未改。release 完整 Win64 269、fmt/release lib/bin strict Clippy/build 通过，不重复性能或游戏矩阵；证据 `wire-out/refactoring-2026-10-14/r3-com-calls64-{source-checks,verification}.json`。
