# COM、任务内存与 GUID

新 Wasm JIT 运行时在 PE32 与 AMD64 接通五项 COM/WinRT 初始化、三项任务内存和六项 GUID 服务。CoCreateInstance 在 PE32 提供本地 WMI 工厂，在 AMD64 保留未注册工厂的失败协议。所有 HRESULT、指针与 SIZE_T 按对应 ABI 传递，DWORD 和有符号字符容量保持各自宽度；成功和 HRESULT 失败均不改写 LastError。

| 接口 | 逻辑 DLL |
| --- | --- |
| CoInitialize | ole32。 |
| CoInitializeEx、CoUninitialize、CoTaskMemAlloc/Realloc/Free、CoCreateInstance | ole32、combase。 |
| RoInitialize、RoUninitialize | combase、api-ms-win-core-winrt-l1-1-0。 |
| StringFromGUID2、CLSIDFromString、IIDFromString、CoCreateGuid、StringFromCLSID、StringFromIID | ole32、combase。 |
| PropVariantCopy/Clear、CoCreateFreeThreadedMarshaler、CoSetProxyBlanket、RoGetAgileReference | ole32、combase；另外经公开 COFF 声明接入对应 API-set。 |

静态导入与 GetProcAddress 共用资格，kernel32/kernelbase 的旧私有别名明确拒绝。装载器同时查询新运行时的 provider 声明，使旧 catalog 在 PE32 未声明的 WinRT API-set 可以装载；服务实现资格继续独立检查。

每个线程直接拥有独立 apartment 状态，新线程从未初始化状态开始，线程终止后清理。CoInitialize 选择 STA，CoInitializeEx 接受 mode 与 0xe 范围内的已知标志，reserved 必须为 NULL；RoInitialize 的 0/1 分别选择 STA/MTA。首次初始化返回 S_OK，同模式重复调用增加引用并返回 S_FALSE，模式冲突返回 RPC_E_CHANGED_MODE，非法参数返回 E_INVALIDARG。引用溢出返回 E_OUTOFMEMORY。

COM 和 WinRT 共用初始化引用；CoUninitialize 和 RoUninitialize 均解除一份引用，引用为零时释放模式，多余解除为空操作。源码复用纯 `win32/com/apartment.rs` 的计数核心，新适配器统一使用同一引用类别；旧适配器的分开计数政策不进入新路径。普通调度不复制或镜像 apartment 状态。

CoTaskMemAlloc 使用同一 VMM 上的私有任务分配器，复用新堆的空闲跨度、配额与页归属算法；它与公开 Heap/Local/Global 的分配成员分开。返回地址按 16 字节对齐，零长度也占一个最小块。单块上限 256 MiB，与所有公开堆共享 64 MiB 元数据计费额度，每个活动块保守计 256 字节，并受 VMM 总容量限制；超大或耗尽返回 NULL，LastError 保持。CoTaskMemFree(NULL) 为空操作，正确指针可由进程内任意线程释放；重复释放、外来指针或 backing 不符报告接口错误，不提交 CPU 或 work。

CoTaskMemRealloc 复用同一任务堆的 realloc。NULL 按 CoTaskMemAlloc 分配（含零长度），非 NULL/零长度释放并返回 NULL；扩缩容保留共同范围的数据，分配失败保留原块。外来 owner、首帧或内存故障沿同一事务拒绝，不改写 LastError。

StringFromCLSID/IID 的输出使用同一任务 owner，须由 CoTaskMemFree 释放。公开输出先验证，内部实际数据范围可写后才提交分配成员，再写字符串并发布指针。空闲页被客体改为只读时，分配文本保持可重试且不消耗跨度。启动拒绝解除全部任务 slab；进程退出后保留最终内存快照至 VM 销毁。

GUID 的前 DWORD 和两个 WORD 按 Windows 小端布局，文本为大写的 38 个带括号字符和 NUL。StringFromGUID2 返回 39；NULL GUID 或小于 39 的有符号容量返回零，保持输出。CLSIDFromString/IIDFromString 接受标准形式及小写十六进制，NULL 输入生成 GUID_NULL。IID 的长度错误返回 E_INVALIDARG，标准长度但格式错误返回 CO_E_IIDSTRING，保持输出。CLSID 没有可解析的 ProgID 时返回 CO_E_CLASSSTRING；非括号形式清零输出，括号开头的无效文本保持原值。输入上限为 32768 个 UTF-16 单元，不引入安装记录或宿主注册表查询。

CoCreateGuid 从宿主取得 16 个安全随机字节，设置 v4 版本及 RFC variant 位；没有可用随机源时返回 HRESULT 0x800706b9，保持输出。核心不读取宿主时间、账户或机器标识来合成 GUID。随机源只在参数、保存返回帧和完整输出范围都可用时请求；观察不消耗 guard，实际故障交还客体，修复后再请求。

WasmMachine 的可选 `random(length)` 必须同步返回所需长度的 Uint8Array，或用 `null` 表示资源不可用；默认使用 Web Crypto。提供者抛错或返回错误类型时保留原 CPU、work 和输出，修复提供者后可以重试。底层 `wire_service_source` 的低八位保留时钟分类，0x100 表示需要 16 字节随机数据；它一次返回所需平台资源，普通系统调用不增加 JS/Wasm 往返。`wire_service_random(owner, low, high, available)` 按小端接收两个 64 位字，`wire_service_clock` 保留原接口。

PE32 的 CoCreateInstance 支持本地 WMI locator，查询、枚举和 BSTR 输出见 [WMI 契约](wmi.md)。AMD64 和其余 class factory 继续返回 REGDB_E_CLASSNOTREG；未初始化或无效 context 返回 CO_E_NOTINITIALIZED、E_INVALIDARG，失败时清零接口输出，NULL 输出返回 E_POINTER。远程/异步 WMI、代理/free-threaded marshaler、OLE/拖放/剪贴板初始化、更多 COM 服务与 WinRT 激活仍未接通。

`node crates/wire-wasm/tests/com.mjs` 和 `node crates/wire-wasm/tests/guid.mjs` 使用稳定项目输入，检查引用、权限/内存事务、ABI、编码与注入随机源。显式 `node tools/probes/check_wasm_pe_entry.mjs --com-guid-only` 需要 MinGW 两种交叉编译器（含 runtimeobject 导入库）及支持 Wasm/Web Crypto 的 Node，验证 O0/O2、真实 C 调用约定、线程共享和缺失工厂，不依赖游戏文件。

PropVariantCopy/Clear 的类型、独立复制和整图清理契约见[属性值](variants.md#propvariant)。这两个入口使用现有任务/BSTR/数组 owner，不创建另一个分配器。

CoCreateFreeThreadedMarshaler 没有聚合或跨 apartment marshaler，返回 E_NOTIMPL 并清零完整接口输出，NULL 输出返回 E_POINTER；不解引用或持有 outer。CoSetProxyBlanket 没有 RPC IClientSecurity 后端，NULL proxy 返回 E_INVALIDARG，其余请求返回 E_NOTIMPL，不调用 QueryInterface 或修改认证状态。RoGetAgileReference 同样没有 agile 对象或 marshaling，返回 E_NOTIMPL 并清零输出；无效 options 或 NULL IID/object 返回 E_INVALIDARG，NULL 结果槽返回 E_POINTER。失败输出经过完整预检，别名返回槽仍使用原调用帧，HRESULT 保持 LastError。
