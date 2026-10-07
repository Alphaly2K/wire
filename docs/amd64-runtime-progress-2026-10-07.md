# AMD64 / Unity / Mono 启动推进（2026-10-07）

这是接续 `6748118` 的运行时检查点。普通 `wire run` 仍拒绝 AMD64；
实际启动使用 `wire probe64 --startup`。解释器是主路径，所有高地址访问仍经过 checked guest memory，
没有把 guest 机器码交给宿主 CPU，也没有引入 QEMU GPL 实现。

## 当前真实结果

AliceToPapa 隔离副本已从13条 EXE 入口指令推进到 **60,162,898 条退休指令、
60,292,800 work、125,927 次已完成 API 调用**（`linked-pass-78.txt`）。
已经实际执行 UnityPlayer 的 TLS / DllMain、EXE 入口，以及动态加载 Mono 的 TLS / DllMain；
合作调度的 worker 也在执行。线程命名异常已真正执行 guest vectored handler、语言处理器
以及同帧 RtlUnwindEx，继续到了 Winsock 和 Unity input 初始化。
已经越过 altered-search DLL 装载、curl 的 SSPI 服务发现、显示器查询和 HKCU 游戏设置键创建。
当前停止于 `user32.dll!LoadIconA`（实际主 EXE、资源 ID 103）的图标装载边界，正在准备窗口。
没有忽略此前 curl 初始化失败触发的 INT3，也没有把不存在的安全包或系统注册键改成成功。

游戏实际写出的 `Player.log` 包含：

```text
Mono path[0] = 'C:/AliceToPapaADV_1-0-0_Data/Managed'
Mono config path = 'C:/MonoBleedingEdge/etc'
Input System module state changed to: Initialized.
```

当前证据还不足以验收托管游戏入口、正式窗口、D3D11/DXGI Present 或持续可玩。
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

指令与 API 计数受真实时间/线程调度影响，上表不构成相同路径的性能 AB 对照。

## 距离正式窗口的剩余工作

当前仍处于引擎/运行时初始化；input 初始化日志不证明托管游戏入口已完成。
先越过实际模块装载和余下初始化，再成组适配 Win64 USER32 的 class/window/message 布局、
64位 WNDPROC / WPARAM / LPARAM 回调及宿主窗口呈现，才能验收真正创建并显示的窗口。
游戏画面还需要 D3D11/DXGI 的64位 COM/vtable/资源布局、设备/交换链、shader 和实际 Present。
既有 PE32 D3D9/Metal 与 AppKit 能力可复用，但当前代码并没有接通这条64位图形路径。
目前无法可靠给出剩余小时数或完成百分比；不是只差最后一个 API。

## 已实现的运行时范围

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
  --max-instr 50000000 --startup
```

下一阶段继续真实资源图标、Win64 USER32窗口类/窗口及 WNDPROC，再以正式窗口和实际 Present 验收；
不以 LoadLibrary 成功、错误退出0或空窗口当作游戏跑通。
完整 release 回归通过262项：8 unit、37 differential、9 FPU、26 IR、1 Wasm、109 Win32、72 AMD64。
本轮新增6项覆盖 altered/system依赖搜索、SSPI表/安全失败、显示结构/实际枚举回调、注册表状态与fault原子性。
两项 Metal 设备测试本轮按默认 ignored；上一提交已单独通过，本轮没有渲染实现改动。
日志为 `/private/tmp/wire-amd64-services-tests.txt`、`wire-amd64-services-clippy.txt`；
Clippy release all-targets `-D warnings`、format、diff whitespace 检查通过。
`scripts/package-macos.sh` 重建 `wire-out/Wire.app`，bundle 与 release binary 校验一致。
旧文档的13条入口/无 Win64 runtime 是历史状态。

本轮版本比较移植自固定 Wine 10.0 的 LGPL-2.1-or-later `ntdll/version.c`，选择 LGPL 2.1，
保留原始快照、版权、许可证及 SHA-256，范围见 `third_party/wine-10.0/README.md`。
Winsock 仅只读核对公开 ABI/行为，没有复制其 C 实现。没有引入 QEMU GPL 代码。
SSPI 本轮只读核对固定 Wine 10.0 的 sspi.h/secur32.spec 及 curl 8.0.1 的初始化流程，
未复制上游实现；显示器与注册表服务为本地独立实现。
