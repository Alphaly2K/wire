# AMD64 / Unity / Mono 启动推进（2026-10-08 接续）

这是接续 `bb74137` 的窗口/图形初始化检查点。普通 `wire run` 仍拒绝 AMD64；
实际启动使用 `wire probe64 --startup`。解释器是主路径，所有高地址访问仍经过 checked guest memory，
没有把 guest 机器码交给宿主 CPU，也没有引入 QEMU GPL 实现。

2026-10-08 接续已接入原生 ARM64 shader / MoltenVK 库及 Win64 DXGI factory4 / adapter1。
全部8个内嵌DXBC转译并通过spirv-val；3组VS/PS与2个CS真实创建pipeline，
Unity内嵌VS/PS执行64×64离屏Draw，4096个RGBA8像素读回逐通道比较0误差。
此Draw是宿主后端验证，不是guest Present。具体许可、COM/资源边界与复跑见
[Win64图形接入设计](d3d11-dxvk-moltenvk-design.md)。

2026-10-08继续补齐MoltenVK缺少原生GS的路线：已实现独立GS→compute IR pass，
11个原创GS、44组输入逐项比较34,013个输出DWORD，覆盖Cut/绕序、adjacency、循环动态索引、
越界归零、条件零输出、GS实例、CBV与超额发射拒绝。
持久Vulkan buffer与Rust Rc所有权/跨设备拒绝也已接入；展开结果经过生成的透传VS和真实DXBC PS
实际DrawIndexed，4096个RGBA8像素符合期望，全部13个GS/VS/PS SPIR-V通过独立验证。
这是宿主后端补齐，不是完整10_0、guest Draw或Present；SO、VS→GS与更多资源/状态仍待实现。

## 当前真实结果

AliceToPapa隔离副本推进到 **598,723,699 条退休指令、600,000,000 work、1,223,023 次已完成API**
（最新完成重放`linked-pass-111.txt`，预算耗尽）。从13条EXE入口开始已实际执行UnityPlayer的TLS/DllMain、EXE入口，
动态加载Mono的TLS/DllMain及合作worker；异常链仍执行真实guest handler与同帧RtlUnwindEx。
此前curl/SSPI、显示器、HKCU、资源图标、类注册、WNDPROC、DC与目录枚举均已越过。
本轮`CreateDXGIFactory2`、真实Apple M4 adapter枚举/Desc/引用、D3D12明确失败后的
D3D11回退及实验性device/context、真实纹理创建/copy/update通过，
COM音频设备缺少factory时真实失败，Unity进入自己的无音频回退；等待定时器线程已实际运行。
暂停线程的真实CPU快照与APC/alertable wait已接通。pass111实际进入MonoManager ReloadAssembly，
托管System.IO因动态导入SetThreadErrorMode缺失报异常；该API与线程错误模式已补齐，正在继续重放。
没有忽略INT3、认证失败或guest对窗口的隐藏状态。

游戏实际写出的 `Player.log` 包含：

```text
Mono path[0] = 'C:/AliceToPapaADV_1-0-0_Data/Managed'
Mono config path = 'C:/MonoBleedingEdge/etc'
Input System module state changed to: Initialized.
```

当前证据还不足以验收托管游戏入口、正式窗口、D3D11/DXGI Present 或持续可玩。
实际 guest 窗口为 HWND=`0x7ffd00630000`、客户区314×211、标题 `AliceToPapaADV_1-0-0`，
但 `visible=false`；这是引擎的隐藏初始化窗口，不是已经显示的游戏界面。
`--window` 已实际验证 AppKit 主线程初始化并到达同一 D3D11 边界（pass89），按真实 visible 标志呈现，
不会把隐藏窗口强制显示来充当正式窗口证据。
启动指令数是兼容进度，不是性能收益。AMD64 仍使用逐指令 decode，没有64位安装期 IR。
既有 PE32 / Metal 路径不受这条诊断路线替换。

| 重放 | 退休指令 | 已完成 API | 最后边界 |
| --- | ---: | ---: | --- |
| pass-61 | 4,044,386 | 8,960 | `CVTDQ2PD` |
| pass-62 | 4,275,323 | 9,525 | `GetFileInformationByHandle` |
| pass-63 | 4,275,393 | 9,527 | `SystemTimeToTzSpecificLocalTime` |
| pass-64 | 4,275,857 | 9,536 | `GetTimeZoneInformation` |
| pass-65 | 4,277,163 | 9,548 | `CreateFileMappingW` |
| pass-66 | 16,277,942 | 53,280 | `SetThreadStackGuarantee` |
| pass-67 | 20,680,194 | 65,725 | 线程命名 `RaiseException 0x406d1388` |
| pass-68 | 20,785,687 | 66,075 | 异常链通过；Winsock ordinal 115 / `WSAStartup` |
| pass-69 | 49,882,370 | 113,675 | 50M work 预算，未发现新故障 |
| pass-70 | 54,879,073 | 121,998 | worker `WaitOnAddress` |
| pass-71 | 56,164,509 | 123,356 | `SetDllDirectoryW` |
| pass-72 | 56,176,439 | 123,372 | 未登记隐式 ntdll 导致空模块句柄 |
| pass-73 | 56,176,863 | 123,386 | `GetSystemDirectoryW` |
| pass-74 | 56,177,133 | 123,389 | `LoadLibraryExW` flags=`0x8` |
| pass-75 | 56,182,224 | 123,405 | secur32 缺失，curl 初始化失败后的 INT3 |
| pass-76 | 58,335,816 | 125,058 | SSPI 初始化通过；`EnumDisplayDevicesA` |
| pass-77 | 60,146,353 | 125,789 | 显示设备/模式通过；`RegCreateKeyW` |
| pass-78 | 60,162,898 | 125,927 | 游戏 HKCU 键创建通过；`LoadIconA` |
| pass-79 | 60,164,054 | 125,933 | 图标/类注册通过；`CreateWindowExW` |
| pass-80 | 60,164,114 | 125,934 | 真正进入 WNDPROC；`ImmGetContext` |
| pass-81 | 60,164,568 | 125,943 | 隐藏窗口创建通过；`GetDC` |
| pass-82 | 60,180,964 | 125,970 | DC查询通过；`FindFirstFileExW` |
| pass-83 | 60,211,629 | 126,049 | 目录枚举通过；`CreateDXGIFactory2` |
| pass-84 | 60,211,629 | 126,049 | 开启 AppKit 桥接；同一 DXGI 边界，窗口仍隐藏 |
| pass-85 | 60,211,853 | 126,055 | factory/adapter通过；D3D12 ordinal101 |
| pass-87 | 60,211,853 | 126,055 | D3D12 ordinal分派通过；delay-load provider缺失 |
| pass-88 | 60,249,414 | 126,117 | D3D12明确不支持并回退；D3D11CreateDevice |
| pass-89 | 60,249,414 | 126,117 | AppKit + 原生Vulkan；同一D3D11边界，窗口仍隐藏 |
| pass-107 | 72,344,956 | 129,032 | D3D11资源初始化通过；GetProcAddress(NULL)被错误终止 |
| pass-108 | 72,605,599 | 129,305 | NULL模块按主EXE查找并返回NULL；CreateWaitableTimerExW |
| pass-109 | 219,185,690 | 367,281 | 等待定时器实际运行；Mono暂停线程后GetThreadContext |
| pass-110 | 291,364,669 | 464,830 | 真正读取暂停线程寄存器后继续；QueueUserAPC |
| pass-111 | 598,723,699 | 1,223,023 | APC/alertable wait通过；600M预算；托管System.IO缺SetThreadErrorMode |
| pass-112 | 387,616,539 | 587,668 | 线程错误模式通过；GetConsoleCP |
| pass-113 | 520,943,132 | 717,098 | 控制台代码页通过；GetCPInfoExW |

指令与 API 计数受真实时间/线程调度影响，上表不构成相同路径的性能 AB 对照。

`16dd486`保存了D3D11资源与Mono线程服务检查点，完整release回归281项通过，Clippy无warning。
之后统一Win64 API参数表与kernel32/kernelbase动态导出发现：已实现函数不再依赖此前是否登记静态gateway。
其它DLL的专属函数不会因此出现在kernel32；未知函数仍返回NULL/ERROR_PROC_NOT_FOUND。
GetConsoleCP沿用CP932 profile；GetCPInfoEx A/W输出284/544字节，代码页/默认字符/lead-byte表与A/W名称完整写入，
无效flags及checked输出fault保留原输出。新增两项回归后完整release283项通过，Clippy无warning。
pass114正在沿实际游戏继续，不将该检查点当作正式窗口或首帧验收。

## 距离正式窗口的剩余工作

当前仍处于引擎/运行时初始化；input 初始化日志不证明托管游戏入口已完成。
Win64 USER32类/初始窗口、64位 WNDPROC / WPARAM / LPARAM 回调与可选 AppKit 桥接已经建立；
真实游戏已通过DXGI factory/adapter和实验性D3D11 device/context、初始化资源，尚未请求显示当前隐藏窗口。
接下来仍需补齐运行时实际边界、交换链、guest Draw和Present，以及所需状态/能力的真实实现。
实际请求adapter=`0x40aab5000`、driver type=UNKNOWN、flags=`0x20`（BGRA_SUPPORT）、SDK=7，
feature-level数组为11_1 / 11_0 / 10_1 / 10_0，没有9_x回退。
本机MoltenVK原生geometryShader=false，而10_x及以上需要geometry；
目前guest bridge明确采用实验性10_0兼容子集，已接通真实native device及资源，
并不宣称全套feature-level规范符合性；独立geometry→compute已验证，完整guest GS/SO与Draw仍待接入。
不能将创建成功、独立资源/Draw探针当作完整10_0或实际游戏Present，详见图形设计文档。
既有 PE32 D3D9/Metal 能力可复用，但当前没有接通这条64位图形执行/像素路径。
Win64 MSG、消息等待、宿主输入→guest消息分发和托管入口仍待实现/验收；桥接保留输入事件，
没有忽略后续未知 API 或声称可交互。
目前无法可靠给出剩余小时数或完成百分比；不是只差最后一个 API。

## 已实现的运行时范围

- 实验性D3D11 device/context、owned Vulkan资源、shader编译、状态描述符与真实query；
  生命周期及GPU数据/clear像素已验证，当前不是完整FL10_0或guest Draw/Present。
- 等待定时器接入合作调度，单次/周期、手动/自动复位、命名共享、重设与取消保留真实信号语义；
  timer APC、系统唤醒与非NULL SECURITY_ATTRIBUTES仍不支持。
- GetThreadContext读取暂停guest线程的真实CPU上下文，按请求写控制/整数/浮点组；
  不使用宿主线程寄存器，当前线程/运行线程与extended XSTATE仍拒绝，SetThreadContext尚未实现。
- QueueUserAPC有界FIFO、SleepEx与Ex wait在目标线程alertable时执行真实guest回调，
  返回WAIT_IO_COMPLETION，不提前执行或丢弃回调；逐条预算暂停/恢复和64位参数已验证。
  Set/GetThreadErrorMode保持线程状态隔离，动态导入可发现。
- 可选原生Vulkan设备对应的DXGI factory4 / adapter1，完整64位COM对象/vtable/gateway、
  IUnknown身份与parent引用，最后Release回收owned页；protected readonly页不允许guest
  VirtualFree/HeapFree或VirtualProtect升级。adapter信息/UUID派生process-local LUID来自实际设备，
  DESC/DESC1明确序列化304/312字节，QI与枚举fault不改变引用或留下对象。
  private byte data有界、短buffer语义与window association已实现；输出/通知/交换链和private
  IUnknown回调引用仍明确未实现，不报告D3D能力。没有后端时不会发放假adapter。
- D3D12 CreateDevice/GetDebugInterface真实返回UNSUPPORTED/NO_INTERFACE；named、ordinal101/102、
  GetProcAddress与delay-load provider一致。NULL ppDevice探测同样失败，非NULL输出checked后置NULL。
  此服务仅让引擎得到实际能力失败并正常回退，不等于D3D12支持。
- PE32+ 静态及动态依赖、DIR64、64位 IAT、导出、delay import gateway；真实 TLS callback / DllMain。
  重复完整路径 LoadLibrary 增加引用；动态 TLS 模板为所有存活线程单独建立。
  新线程第一次调度才收集 DLL_THREAD_ATTACH，避免漏掉创建后、恢复前装载的 DLL。
  loader owner 防止另一个线程交错进入初始化通知。
- GetModuleHandleEx A/W 支持 FROM_ADDRESS、UNCHANGED_REFCOUNT、PIN；guest 模块额外引用可释放。
  最后一次真正卸载、失败 DllMain 的完整装载回滚仍拒绝，不把 FALSE attach 报成成功。
- Windows x64 整数/指针 gateway、RCX/RDX/R8/R9 和 shadow-space 栈参数、完整64位返回地址；
  guest callback 可以跨逐条预算暂停恢复，不复用32位 stdcall 清栈。
- 稀疏高地址 VMM 的 reserve/commit/decommit/release/protect/query、image/private/mapped 类型与 guard。
  宿主内存始终是数据；跨页检查失败不提交半次写入，guard 消耗保留其架构行为。
- 合作线程的真实执行、挂起/恢复、等待、退出状态、线程句柄别名、独立 TEB/TLS/FLS、
  DLL thread attach/detach、FLS 析构、独立 MXCSR/x87 控制字。
  event/semaphore/critical section/SRW/InitOnce 使用真实对象和所有权，不伪造等待成功。
- confined 文件系统、同步文件 I/O、共享与访问权限、句柄复制/关闭、文件偏移、匿名管道、
  环境变量、命令行、guest 用户目录、系统/时间/拓扑、CP932/UTF-16 和有界 locale 查询/回调。
  COM/WinRT apartment 初始化与引用平衡已实现，不等于完整 COM 或 WinRT 对象支持。
- PSAPI/K32 模块查询使用实际 guest PE 元数据。内置 provider 是逻辑模块；没有 PE 时
  ModuleInformation 返回不支持，逻辑 system32 文件名不声称磁盘上存在 DLL。
- FormatMessage A/W 的有界系统消息 / FROM_STRING 与 LocalFree 所有权；复杂 inserts/语言仍失败。
- ETW provider 注册/注销、disabled-session 查询与写入验证。当前没有 tracing session，
  不发假的 enable callback，也不声称已记录事件。
- `.pdata` 静态函数表、RtlAdd/DeleteFunctionTable、RtlInstallFunctionTableCallback 和
  RtlLookupFunctionEntry。动态查找真正调用 guest callback，检查返回条目和输出 image base。
  已修复 Mono 初始化因 RtlInstallFunctionTableCallback 失败产生的断言，未忽略原来的 RaiseException。
- WSAStartup/WSACleanup 的进程引用、版本协商和完整 Win64 WSADATA 布局（408字节）；
  WSAGet/SetLastError 使用当前线程 TEB 的 LastError。
  ordinal 111/112/115/116 和按名称 GetProcAddress 指向同一服务；socket 网络操作仍明确未实现。
- WaitOnAddress / WakeByAddressSingle / WakeByAddressAll 接入合作调度：checked 1/2/4/8字节比较，
  无匹配时立即返回、真实超时、按登记顺序唤醒、无 waiter 时不积累 wake credit。
  唤醒不要求值已变化，等待可跨逐条预算恢复，LastError 在线程间隔离；没有忙循环伪造成功。
- Set/GetDllDirectory A/W 维护实际进程搜索状态；NULL 恢复默认、空字符串排除 CWD。
  默认支持 EXE directory / 显式 DLL directory / System32 / Windows / CWD 的 confined 搜索，
  也用于真实导入依赖，并补默认 `.dll` 后缀。
  已支持的 SYSTEM32 / DEFAULT_DIRS 标志保持限定搜索；PATH、AddDllDirectory cookies、
  LOAD_WITH_ALTERED_SEARCH_PATH 使用目标 DLL 目录替换依赖搜索首项，并递归传递限定搜索；
  不修改全局 SetDllDirectory。完整路径要求、非法组合和依赖实际代码已验证；
  加载回滚仍不是完整支持。逻辑 provider 只接受规范 System32 完整路径或名称，任意同名路径不能冒充。
- secur32 的 InitSecurityInterface A/W 返回真实224字节 Win64 函数表，版本1；
  pointer slots 是64位 guest gateway，保留槽为空。表为只读 runtime-owned page，不属于 guest heap。
  当前包注册表为空；枚举返回 count=0/info=NULL，Schannel/NTLM 等查询和获取凭证返回
  SEC_E_SECPKG_NOT_FOUND；无有效凭证/上下文的认证、签名、加密调用返回 SEC_E_INVALID_HANDLE。
  这只完成服务发现，不是 TLS 或网络认证。SSPI 错误不污染 Win32 LastError。
- 单1024×768、32bit、60Hz逻辑显示器的设备/monitor 枚举、A/W DEVMODE、MonitorInfo与
  MonitorFromPoint/Rect 查询保持一致；Win64 POINT 的两个32位坐标在同一个64位参数中。
  EnumDisplayMonitors 真正执行 guest callback，传递64位 LPARAM，处理 clip/BOOL结果并释放临时 RECT。
  非零 HDC、CCD display-configuration 路径仍返回不支持，没有声称存在 GPU 或交换链。
- 隔离的进程内注册表支持 HKCU 键/值创建、打开、查询、写入、删除值、关闭与访问权限，
  64位 HKEY 和 sign-extended predefined roots 不截断；键名大小写无关，CP932/UTF16字符串值互转。
  short-buffer 返回 ERROR_MORE_DATA、报告字节尺寸并保留数据；fault 输出不创建半个键或句柄。
  系统 hive 只读、缺少的安装信息真实返回找不到；尚无持久化、32位 registry view、ACL或枚举。
- PE32+图标资源树使用32位 RVA / 64位 guest地址，有界目录/名称/语言/数据校验；
  实际读取 RT_GROUP_ICON / RT_ICON，选择32px附近的合法32bit BI_RGB DIB，并保留位图/mask payload。
  LoadIcon A/W共享生命周期、LoadImage的非共享对象、stock cursor/icon身份及错误返回已接入。
  PNG、其它DIB格式、指定尺寸缩放、guest cursor资源和native icon绘制仍不支持。
- WNDCLASS / WNDCLASSEX 使用72 / 80字节 Win64布局；注册保留完整 instance/procedure。
  初始 top-level CreateWindowEx 使用80字节 CREATESTRUCT，执行真实 guest
  WM_GETMINMAXINFO / WM_NCCREATE / WM_NCCALCSIZE / WM_CREATE；回调可跨逐条预算继续。
  WM_NCCREATE返回FALSE、WM_CREATE返回−1时执行适当销毁消息、撤销窗口并返回NULL；
  临时结构在所有回调完成后释放。不是只分配 HWND 后跳过 WNDPROC。
- 窗口矩形/客户区、宽窄字符串标题、pointer-width WindowLongPtr与有界extra bytes、
  同线程 SendMessage / CallWindowProc、ShowWindow / DestroyWindow 回调已建立。
  MonitorFromWindow 查询实际对象；AdjustWindowRect复用已保留LGPL来源的共用geometry。
  子窗口/menu、跨线程发送、reentrant self-destroy、完整默认窗口行为、DPI非96和消息循环仍有边界。
  focus/foreground当前只有进程内基本状态，没有完整activation/focus消息协议。
- 无已安装IME的guest profile：有效窗口 ImmGetContext返回NULL，不伪造HIMC或composition；
  DC对象按窗口/线程维护，ReleaseDC/DeleteDC 区分来源与所有权，GetDeviceCaps描述逻辑显示器。
  尚无Win64 GDI像素绘制或selected pixel format；D3D11实验性资源路径另行接入。
- FindFirstFile / Ex、FindNextFile A/W和FindClose使用真实confined目录、64位search handle、
  320/592字节输出、FILETIME和64位文件尺寸；checked fault不消费下一个条目。
  外逃symlink不能进入枚举，基础大小写无关wildcard复用既有PE32实现；无DOS 8.3别名、
  NT全部wildcard规则、case-sensitive/filter-directory/其它Ex选项或完整reparse服务。
- probe64新增可选`--window`，native UI ID通过表映射到完整64位HWND，不截断guest句柄。
  只呈现已完成创建且由guest设为visible的窗口；主线程AppKit轮询保留native事件到有界guest事件队列。
  目前事件尚未转成Win64 MSG，也没有64位Present像素上传。
- ntdll 是隐式常驻 provider。版本查询/判断使用明确的 Windows 10.0.19045 workstation profile，
  RtlGetVersion / RtlVerifyVersionInfo、GetVersion / GetVersionEx A/W、VerifyVersionInfoW 和
  VerSetConditionMask 保留 NTSTATUS/Win32 错误区别与层级比较，而不是总返回满足。
  GetSystemDirectory / GetWindowsDirectory / GetSystemWindowsDirectory A/W 返回 confined guest 路径；
  不声称逻辑 system32 provider 在宿主磁盘存在对应 Windows DLL。

## 已实现的软件异常链及边界

RaiseException 构造实际 AMD64 EXCEPTION_RECORD（152字节）、EXCEPTION_POINTERS、
16字节对齐 CONTEXT（1232字节），异常参数保持64位。
按登记顺序执行真实 guest vectored handler；继续执行时读取修改后的 checked CONTEXT，
恢复 GPR/RIP/FLAGS/MXCSR/XMM0–15 和既有 FP80 x87 状态，回调自身 clobber 不覆盖保存上下文。
noncontinuable 不允许继续，完成后释放 owned exception buffers。

当前帧的语言处理器、HandlerData 和 establisher frame 从实际 `.pdata/.xdata` 读取，
DISPATCHER_CONTEXT 使用80字节 AMD64 布局；ScopeIndex 由真实 guest handler 读写。
同一 establisher frame、同一函数内的 RtlUnwindEx 转移使用实际 TargetIP 和完整64位 ReturnValue；
EH-only 与有 termination handler 的元数据分别处理，后者实际调用 guest unwind handler。
真实 Mono 线程命名异常因此进入其 C_specific handler / RtlUnwindEx，未硬编码异常码或 catch PC。

这不是完整 SEH/unwind：跨帧/chain/versioned metadata、collided unwind、动态 table callback 异常查找、
CPU fault→SEH 与 stack-overflow guarantee 仍明确失败。
CONTEXT 仅支持 CONTROL/INTEGER/FLOAT；高于32位的既有 x87 ip/dp 恢复仍拒绝，不能截断。

## CPU 与浮点边界

AMD64 整数宽度/窄寄存器/REX/RIP-relative、FLAGS、乘除、位操作、条件移动、锁定操作、
CMPXCHG8B/16B、预算可暂停的 REP 与 CPUID 单 guest CPU profile 已覆盖当前启动需要。
所有 AMD64 FLAGS 仍完整计算，不沿用未经验证的32位 FLAGS 丢弃优化。

SSE 的 moves/shuffles、基础 packed/scalar arithmetic、compare、sqrt、reciprocal estimate、
整数/浮点互转采用 checked operand width 与 MXCSR。APFloat 处理舍入、DAZ/FTZ、
NaN/indefinite 和异常；多个 lane 聚合异常，unmasked #XM 时只提交 MXCSR 状态，不提前提交目标寄存器。
新增六种 packed integer/float 转换使用类型对应的 lane 规则；legacy m128 要求16字节对齐，
CVTDQ2PD 的 m64 只读8字节、允许不对齐。
Unicorn 对部分 float→integer 转换不更新 MXCSR，因此寄存器结果做差分，异常状态另用独立架构向量检查。

x87 当前只补 FNSTCW / FLDCW / FNSTSW / FNINIT / FNCLEX / WAIT，
状态保留既有 FP80 payload，不降成 F64。**尚无完整 AMD64 x87 算术、栈或环境指令**。
既有 Fpu ip/dp 为32位；控制指令不修改它们，后续完整64位 FPU 指针必须单独扩展。

## 文件元数据、时区与映射

GetFileInformationByHandle、GetFileTime 和常用 GetFileInformationByHandleEx 的
Basic / Standard / AttributeTag / Id 查询来自实际文件 descriptor。
文件大小保持64位，volume/inode/hard-link identity 不用随机句柄冒充；重命名后仍查询打开的文件。
多个可空 FILETIME 输出在写入前全部 preflight。

默认时区转换使用 libc 的日期对应 tzdata；显式 TIME_ZONE_INFORMATION 支持固定偏移、
相对/绝对转换日期和南半球规则，毫秒与跨年保留。反向转换拒绝春季 gap，显式 overlap 选标准时间；
默认时区反向 overlap 目前采用宿主 mktime 的选择。
GetTimeZoneInformation 从宿主当年偏移/夏令时转换生成规则；无法用一个 Windows TZI 表达的
基准偏移变化/多次夏令时区间返回不支持，不声称支持 Windows registry 时区库或动态历史 TZI。
固定时区及纽约/悉尼的独立子进程验证已通过。

文件映射目前支持**未命名、file-backed PAGE_READONLY / FILE_MAP_READ**，包括 MapViewOfFileEx、
64位最大尺寸/偏移、精确视图范围、Flush/Unmap、句柄复制与保护关闭。
Section 保留真实 descriptor，视图以独立引用延续生命周期；guest WriteFile 按 volume/inode
更新全部重叠视图，不改变 guest 页的 READ / NOACCESS / guard 权限。
VirtualQuery 输出 MEM_MAPPED，VirtualFree 不可释放视图，VirtualProtect 不可升级成可写/可执行。
活跃 section/view 阻止 guest truncate，避免旧范围失效。单 view 限制256MiB、总页容量512MiB。
宿主外部进程修改文件尚不实时同步，当前样本使用稳定的隔离资产；这不是完整共享 section 服务。
可写/COPY/EXECUTE、命名 section、分页文件 backing 和 SEC_IMAGE 仍明确返回不支持。

SetThreadStackGuarantee 的零请求真实查询当前保证值0；非零请求返回 ERROR_NOT_SUPPORTED，
保留输入大小。目前没有 overflow-handler reserve 或 stack-overflow SEH，不能报告保证成功。

## Mono 原生 hook 的判断

可以把 Mono C API 接到宿主构建的 Mono 解释器，减少模拟 x64 Mono/GC/JIT 基础设施的成本。
接入点应是 guest 导入/GetProcAddress 的显式 bridge，而不是修改宿主机器码或把 Windows DLL 改名装载。
必须成组处理 domain/image/method/object handles、GC root/pinning、UTF-16 和托管异常；
Unity 注册的 internal calls 是 guest x64 函数，需要从宿主托管解释器重入 guest 调度器；
P/Invoke、反向回调、TLS/线程附着、GC safe points 同样需要 ABI 和所有权桥接。
Unity fork 与对应 class libraries 的版本也需要匹配；仅替换 mono_jit_init / mono_runtime_invoke 不够。

这条路线尚未实现或作为当前成功证据。本轮没有安装或链接宿主 Mono。
只读参考的 Unity Mono 分支文件声明 runtime MIT、部分组件 BSD，构建工具另有 GPL；
没有复制这些源码入库。未来引入宿主 Mono 时须核对具体版本和实际组件的许可及 notices。
no-JIT 与使用数据 IR 也不自动证明 App Store 审核一定通过。

## 复现与接续

原样本只读；运行目录是 `wire-out/games64/alice/runtime/`。
EXE / Unity / Mono 的 SHA-256 仍与 [恢复样本记录](x86-64-sample-2026-10-06.md) 一致。
每次重放的完整输出保存为 `wire-out/games64/alice/linked-pass-N.txt`，不提交游戏资产。

```sh
cargo build --release
cd wire-out/games64/alice/runtime
../../../../target/release/wire probe64 AliceToPapaADV_1-0-0.exe \
  --max-instr 200000000 --startup --window \
  --graphics-backend /Users/alphaly/Projects/wire/wire-out/libwire_graphics.dylib \
  --moltenvk /private/tmp/wire-moltenvk-1.4.2/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib
```

下一阶段从真实 `D3D11CreateDevice` 开始接设备/资源/能力补齐及消息分发，再以正式窗口和实际 Present 验收；
不以 LoadLibrary 成功、错误退出0或空窗口当作游戏跑通。
当前完整release回归通过271项：14 unit、37 differential、9 FPU、26 IR、1 Wasm、109 Win32、75 AMD64。
本轮新增4项覆盖COM身份/引用/parent、Win64 Desc布局/高指针、readonly保护、故障原子性、
private data短buffer、D3D12 named/ordinal/动态查找与真实失败。GS新增两个unit测试检查strip/绕序与坏记录。
原生GPU ignored测试另行通过2项，包含Rust shader/buffer/context/library生命周期、
跨context拒绝、CBV更新/绑定及实际Draw/readback；两项既有Metal测试
本轮保持默认ignored，未修改PE32 Metal实现。
日志：`/private/tmp/wire-native-graphics-all-tests.txt`、`wire-native-graphics-rust-tests.txt`、
`wire-native-graphics-clippy.txt`。Clippy release all-targets `-D warnings`、format、diff whitespace检查通过。
本次日志改为`/private/tmp/wire-geometry-{all-tests,native-rust-tests,clippy}.txt`；
GS证据在`wire-out/geometry-probe-2026-10-08/`。
默认cargo build / Wire.app没有捆绑第三方GPU二进制；backend构建与验证详见图形设计文档。
旧文档的13条入口/无 Win64 runtime 是历史状态。

本轮版本比较移植自固定 Wine 10.0 的 LGPL-2.1-or-later `ntdll/version.c`，选择 LGPL 2.1，
保留原始快照、版权、许可证及 SHA-256，范围见 `third_party/wine-10.0/README.md`。
Winsock 仅只读核对公开 ABI/行为，没有复制其 C 实现。没有引入 QEMU GPL 代码。
SSPI 本轮只读核对固定 Wine 10.0 的 sspi.h/secur32.spec 及 curl 8.0.1 的初始化流程，
未复制上游实现；显示器与注册表服务为本地独立实现。
本轮资源/Win64 USER32与目录查询是本地实现；共用geometry仍沿用既有Hangover Wine LGPL来源，
出处见其文件头和 `third_party/hangover/README.md`。未引入新的Wine/QEMU renderer或原生Mono。
