# 面向 Wasm 的二进制转译运行时

## 目标与持续约束

以 Wasm 作为唯一客体执行目标，构建覆盖 x86/AMD64 与既有 Windows 用户态兼容功能的二进制转译运行时。转译结果须适合继续被浏览器编译为原生代码；代码密度与编译成本摊销共同作为架构目标。保留可复用的 Win32、D3D、指令语义与验证资产；原生解释器、旧执行 IR、段执行器及其缓存不进入新运行时。复杂指令使用已编译进核心 Wasm 的具体数值函数，不恢复解释循环。

根 Cargo 工作区默认构建 `crates/wire-wasm`，标准测试只发现新核心。共享 Win32、图形和数值代码已从旧执行目录迁出；原生解释器、旧执行 IR、旧实验 crate、旧 CLI 与其专用工具已移除。旧处理体保存在忽略的迁移快照中，版本化接口基线保留旧全集和出处。旧兼容层的批量迁移已按冻结台账完成入口核对；目录外依赖与未实现行为继续逐项维护。完整静态审计已将主静态图和两个动态原生运行库候选的未绑定入口清零；最近一组 39 项已整组接通，其中 19 项提供虚拟状态/共享实现，20 项保留后端缺口和明确失败协议。动态查询与已绑定失败分支已有完整运行汇总；当前已在真实前台 WebGPU 环境完成四十亿条退休指令对照和七次 Present，下一阶段审计编译成本及代码寿命。创建 crate、编译通过和减少旧代码数量均不作为整体完成条件。

先完成既有 Win32 的全量接口盘点、可复用能力迁移与项目输入验证，再冻结实现进行少量完整真实游戏对照；不按游戏停点、地址或测试源码调优。性能验收至少覆盖五亿条真实退休指令，分别报告冷启动、后段和整体；兼容阻塞导致未达到该长度时，不作最终性能结论。比较须记录 WebContent 物理内存峰值、进程 CPU 时间与温控状态，考虑 WebKit 多核编译、设备降频和运行顺序；关键收益交换顺序复核。macOS 资源记录与 iOS 设备的 jetsam 验收分别维护。重写方向、内存约束与减少昂贵试验的要求持续有效。

## 转译架构调整

代码密度和实时编译策略持续共同推进。发射器应减少预算/恢复控制流、重复状态计算和可在编译期消除的操作，同时保持短预算、部分故障、FLAGS、权限和源码撤销的准确状态。Wasm 静态字节数、动态路径和浏览器原生工作量分别评估，不以 locals 声明数推算寄存器溢出或固定 MIPS 上限。

冷代码采用低成本的按需 Wasm 编译，首次进入不展开未执行的分支和跨函数调用图；热度来自实际进入的代码，热点区域仅合并有执行证据的块，再投入较昂贵的优化。冷/热代码共享精确语义、执行租约和内存计费，继续只执行生成的 Wasm；编译期 CFG 在发射后释放。

静态且可验证的映像可在运行前生成可复用 Wasm 转译缓存，装载时重新验证映像、装载地址和源码 authority；运行时新生成及修改的代码继续按需处理。预生成 Wasm 只前移本层解码与发射，浏览器原生编译成本仍须纳入整账，不能依赖浏览器提供可导出的原生代码缓存。当前尚无可交付的 AOT 缓存或热度分层，完成前保持明确的阶段状态。

下一实现阶段先分离冷编译和区域编译，接入有界热度及热点升级，再推进区域内发射密度；稳定后建立静态映像预转译缓存。冷编译时间、后台编译 CPU、生成字节、重编译量、冷/后段/整体吞吐和物理峰值同时参与验收。不得通过扩大未验证代码的常驻规模换取表面命中率。

## 架构审计依据

旧实验 JIT 在相同约 40 亿 work 轨迹中均值约 433–515 秒，同核解释器约 233 秒，40 个 CPU 检查点和图形契约一致。共享冷代码将生成 Wasm 输入减少约 95%，仍未解决性能倒挂。helper 状态事务、一次性热选择和源码层容量驱逐构成已确认的架构缺陷；停止继续打磨该实验后端。

Win32、DXGI/D3D11、D3D9、CPU 状态、SSE/x87 与 VMM 的复用边界已审查，见[运行时架构](docs/architecture/wasm-runtime.md)。D3D9 已分离旧 PE32 调用/内存及原生光栅后端依赖，当前入口复用软件绘制与资源语义。

## 当前实现

新运行时已能装载并执行项目控制程序的 PE32/PE32+ 编译产物，已接通时钟、进程/线程身份、LastError、线程创建/等待/退出、同步对象/混合等待、软件与已识别硬件异常投递、模块查询/装卸、VFS 搜索/目录及普通文件/句柄服务；两种 ABI 的类注册、顶层窗口生命周期、同线程过程调用、基本查询、文本/输入所有权、位置通知及键盘状态/US 布局已接通，两种 ABI 均接通 child/parent、启用/默认对话过程、控件消息/整数转换及窗口枚举；消息队列、DispatchMessage/WaitMessage、消息与对象混合等待、消息计时器、宿主批量输入与两种 ABI 的 RawInput 已接通；矩形绘制、UpdateWindow、DC/兼容位图/DIB 创建读回缩放/像素操作与宿主像素快照已接通；PE32 D3D9 工厂及全部 COM 槽已接通共享软件绘制与真实窗口像素 Present，AMD64 DXGI/D3D11 已接通共享 shader 编译/阶段连接、真实 WebGPU Draw、资源读回和 canvas 呈现，子进程图形/音频按需建立独立后端；各进程 USER/GDI 软件窗口已接入浏览器画布与物理输入，通用 Windows 应用与游戏兼容仍未完成，根 Cargo 默认构建已切到新核心。已实现部分如下；具体执行契约集中在架构文档和源码。

| 部分 | 当前能力 |
| --- | --- |
| 架构状态与复用组件 | 独立 GPR/FLAGS、XMM/MXCSR、FP80 状态；原有 Win32 契约目录、便携图形描述/WebGPU 资源适配器、SSE 与 x87 数值核心已独立编译。生产浏览器启动已接通；外部大文件按需供给已接通；默认构建已切换，旧执行源码已移除，完整应用仍需补齐行为缺口。 |
| VMM | 单份稀疏 backing，Windows reserve/commit/decommit、guard、映射所有权、只读文件视图与 PE32/Win64 查询布局；跨页写入事务、源码证明撤销与版本回绕防护。64 项 TLB 由共享驻留 Wasm 函数检查完整 tag、权限和映射 epoch，命中后的数据访问直接发射。VirtualAlloc/Free/Protect/Query 及 Ex 的客体 API 已接通，FlushInstructionCache 复用源码写入失效；两种 ABI 保存调用帧、完整 SIZE_T、28/48 字节查询输出和错误槽故障恢复。 |
| 文件与句柄 | 复用同一有界 VFS、共享 payload、文件 cursor/权限、查找所有权及元数据布局；外部不可变源、64 位高偏移、按需读缓存和私有修改已接通；普通文件、枚举、标准 I/O 和句柄服务已接入两种 ABI，Win64 另接入 NtQueryDirectoryFile。文件/目录 ID 由共享卷分配，跨进程视图、改名/删除及输入快照保持一致；ReplaceFileA/W 与无进度回调的 CopyFileExA/W 已接通。同步 I/O、属性查询/受限修改、时间设置、文件复制/临时文件、目录枚举、路径变更、匿名只读文件映射及别名/标志已接通；输出成功后提交游标/引用，文件与 DLL 共用共享约束。通用控制台、分段阻塞管道写、其余映射类型及其余元数据 setter 尚未迁移。 |
| 直接编译 | iced-x86 → 编译期 CFG → Wasm；编译后释放指令与 CFG，不保留运行时指令流。GPR 和延迟 FLAGS 保持 locals，支持直接自然循环、嵌套与间接控制流、块内恢复、精确预算与故障 PC。 |
| 整数、控制与栈 | 常规整数/别名/进位/移位/旋转/宽乘除、位操作、部分 FLAGS、XADD/CMPXCHG 与宽比较交换、合法 LOCK、LOOP；PUSH/POP/CALL/RET/LEAVE、FLAGS 栈、PUSHA/POPA、ENTER；五类字符串操作及 REP 的预算、故障与源码写入恢复。 |
| CPU 特征与时间 | 固定 CPUID 发布 SSE 至 SSE4.2、POPCNT、CX8/CX16、RDTSCP 等已接入能力与 FCS/FDS 弃用位。RDTSC/RDTSCP 使用单调 10 MHz 平台时钟；错误保留精确进度后交还宿主并允许重试。POPCNT 直接发射，CRC32 使用固定宽度按值入口；F3 位扫描保留无 BMI1/LZCNT 时的旧式语义。PREFETCHW/NTA/T0/T1/T2 提示、三种 fence 和 MOVNTI 已接入。 |
| Windows 执行边界与服务 | 服务、主入口返回、回调返回和线程返回已形成精确出口；逻辑入口不进入编译期 CFG，跨入口指令报告执行保护故障。统一整数调用帧支持 Win64 寄存器/栈参数、PE32 stdcall/cdecl 与宽参数，先保存参数和返回再写输出。已绑定六种时钟、七种身份/错误服务及 GetModuleHandleA/W/ExA/W、GetModuleFileNameA/W、GetProcAddress、LoadLibraryA/W/ExA/W、FreeLibrary、DisableThreadLibraryCalls、Get/SetDllDirectoryA/W 与 Get/SetCurrentDirectoryA/W；成功计一 work、不计客体退休。查询复用已发布导出图，provider 身份独立于可调用服务；已接入同 JIT 的线程调度、线程 TLS/DLL 通知与线程/事件/信号量/互斥体/定时器的单个及混合等待；软件异常、已识别 CPU/访存异常、VEH 与 PE32 注册链已接入每线程回调续点；AMD64 跨帧语言搜索、函数表查询及 handler 内的目标展开已接入；exit/collided 等展开与其余枚举调用者仍待迁移。 |
| USER 窗口基础 | 复用类身份、便携窗口记录和非客户区几何；两种 ABI 的 A/W 类注册/注销、顶层窗口创建/拒绝清理/销毁、默认与转发过程、同线程 SendMessage、显示和基本查询/字段已接入。WNDPROC 使用同一 JIT 的每线程续点，支持 subclass、同步重入、异常、让出、DLL 装载和退出清理；消息队列、DispatchMessage/WaitMessage/MsgWait、MSG metadata/queue status、消息计时器/TIMERPROC、宿主窗口枚举/批量输入和文本翻译已接通；文本、每线程 focus/active/capture、进程前台状态、MoveWindow/SetWindowPos 与键盘状态/US 布局已接通；绘制、UpdateWindow 与基本 GDI 已接通；两种 ABI 的父子坐标、SetParent/EnableWindow/DefDlgProc、控件消息/整数及窗口快照枚举已接通，完整后代销毁通知/模态 manager 及其余 USER/GDI 尚未迁移；根与子进程软件窗口已接入浏览器展示。 |
| 虚拟显示与几何 | 两种 ABI 共用单显示器 1024×768/32bpp/60 Hz profile、设备/模式/monitor 记录、矩形与 96-DPI 边框；显示器枚举使用同一 JIT 的每线程回调，支持嵌套/装载/退出。三项 CCD 入口直接返回不支持；浏览器软件窗口展示已接入，原生桌面 compositor、设备接口与 DC 枚举仍待接通。 |
| Winsock | 两种 ABI 共用 78 项 ws2_32 命名/经典序数入口，复用启动、TEB 错误、事件与地址值。新增 BSD IPv4、每线程文本/hostent、虚拟主机/localhost/数值 IPv4/IPv6 与数字端口解析、fd_set 查询、普通/Ex owned 结果及释放；线程退出回收临时块，结果最多 1024 项。网络传输、外部 DNS/数据库、IDN/接口探测、异步和阻塞回调仍明确失败，不创建可通信 socket。 |
| 注册表 | 两种 ABI 的 34 项声明共用进程内键值、完整句柄和有界存储；原有创建/打开/值操作及关闭之外，接通直接子键/值枚举、容量元数据、叶子和递归删键及 SHDeleteKey。名称保留首次拼写，删除后的句柄不指向重建节点，全部输出先预检。HKCU 可写，其他支持根只读；Flush、时间戳/安全元数据请求返回 50，WOW32、class、持久化与跨进程共享仍缺失。 |
| token 与 SID | 两种 ABI 共用便携句柄、每线程模拟级别和 SID 布局；token 查询/复制/模拟/恢复、关闭后身份保持、SID 分配释放、TRUSTEE 与本地账户查询已接通。ACL、安全描述符、认证启动、其他账户和 token 修改仍缺失；输出故障保持事务。 |
| 随机数与哈希 | 两种 ABI 的 bcrypt RNG 复用共享 provider 状态，支持安全宿主样本、16 MiB 上限与失败重试；CryptoAPI 九项接口支持临时 provider、MD5/SHA1/SHA2 增量哈希、独立复制、状态/长度查询及完整输出事务。没有密钥、签名、加解密、证书或容器持久化。 |
| COM 与 GUID | 两种 ABI 接通 COM/WinRT 共享线程初始化、独立任务内存（含 realloc）及六项 GUID 服务；安全随机数据只在停止的服务边界按需提供。HSTRING 的九项字符串服务、WinRT 两项未注册激活失败协议和 SSPI 三十四项空安全包服务已接通；PE32 CoCreateInstance 已复用本地 WMI 工厂、四套接口/65 个方法、单行系统查询和共享 BSTR；AMD64 仍提供未注册类失败协议。PropVariantCopy/Clear 共用已有 task/BSTR/数组 owner，支持数值、字符串/BLOB/GUID/CLIPDATA、vector 和嵌套属性。非 NULL 接口引用、marshaller/代理/agile、远程 WMI、OLE 和实际 WinRT 激活仍待迁移。 |
| MCI 与 wave | 两种 ABI 共用四项 MCI 和完整三十九项 wave 声明；枚举零设备、open 的 QUERY/完整输出规则、无设备/句柄失败及 A/W 英文错误描述已接通。没有 wave 播放、录音、mapper 或宿主后端，DirectSound 独立。 |
| PE32 CRT | 六十六项旧函数和十二项原始数据已绑定；字节/解析/x87/排序复用原算法，启动数据共用参数解析与运行时页，初始化/qsort/退出使用同一 JIT 续点，内部递归锁接入线程等待。FILE、调用者 on-exit 表、部分非零 callback/new-handler 和通配符展开仍明确失败，完整契约见 CRT 文档。 |
| 可选图形后端 | 两种 ABI 接通十八项 WGL、像素格式、DXGI debug、D3D11-on-12 和 DWM 设置查询/失败协议；原 GetPixelFormat 直接复用。当前 GL context/DC 为空，NULL context 释放成功；其余操作按真实无后端状态报错，HRESULT 输出统一预检并清零，不创建 GPU 或图形对象。GL 渲染/GLSL、像素格式表、D3D12 互操作、debug 对象和 compositor 仍缺失。 |
| PE32 D3D9 | 工厂及 234 个 COM 槽接当前 VMM/堆/网关/USER，复用对象、状态块、固定管线、VS2/PS2 和软件光栅；Present 写入 GDI 共用窗口像素。169 槽进入原处理分支，65 槽保留原不支持返回；根与子进程软件窗口可通过 BrowserPresenter 显示；GPU 加速和完整 shader/格式尚缺。 |
| GDI 与绘制 | 两种 ABI 共用 DC 身份与 raster 状态，接入 Get/ReleaseDC、兼容 DC/位图、stock brush、对象查询、颜色/原点、像素和 SRCCOPY；窗口和普通位图使用有界 owned COLORREF；匿名 32bpp DIB 使用同一 VMM 的客体 BGRA8 backing，接入 CreateDIBSection/GetDIBits/StretchDIBits、DIBSECTION 与选中 DIB 时的兼容创建。矩形失效/验证、Begin/EndPaint、GetUpdateRect、UpdateWindow、默认擦除及合成 WM_PAINT 使用同一 JIT 续点，保留重入区域和输出故障完成结果，展开/退出归还 DC。宿主可取 owned RGBA8 快照；GetWindowDC、其余 DIB 格式/section backing、字体 realization/metrics 和复杂 region 尚未接通；软件像素已接浏览器画布。 |
| PE 映像与初始线程 | PE32/PE32+ 的有界校验、节映射/权限合并、HIGHLOW/DIR64、IAT 与重定位已接入。客户 DLL 支持静态依赖、名称/ordinal 导入、数据导出、转发及循环依赖；整个图绑定后按依赖顺序执行各模块 TLS 与 DllMain，共享依赖只初始化一次。临时 VMM 和绑定表成功后一次发布；TEB/PEB、命令行、栈与独立 TLS 块已接通。PE32 保留 opt-in DEP；Win64 始终启用 DEP。动态装卸支持重入初始化、外部引用、循环依赖回收及 TLS 释放；静态启动失败会逆序通知并释放映像/TLS，清理后保持拒绝；延迟导入、未装载转发目标与 VFS 搜索已接入同一装载状态机，显式完整路径区分同名 DLL。 |
| 整数 SIMD 与位搬运 | XMM 驻留组、对齐/非对齐及窄/宽搬运、打包算术/比较/饱和/乘积/乘加、水平与符号运算、SAD/MPSADBW、最小值及位置、混合/隐式 XMM0、排列/解包、扩展/插入/提取、PTEST、LDDQU/MOVNT、PCMPESTRI/PCMPESTRM、PCMPISTRI/PCMPISTRM。直接发射 Wasm SIMD，窄访存不扩大宽度。 |
| 浮点 SSE | 标量/打包加减乘除、min/max、平方根、谓词与 COMI/UCOMI、格式与整数转换、SSE3 水平/交替加减、ROUND、DPPS/DPPD、LDMXCSR/STMXCSR。数值入口只传值，完整指令一次调用，CPU backing 在外层出口提交。ROUND 普通输入使用 Wasm 原生舍入；点积保留逐乘、逐加的舍入与异常边界。 |
| 近似浮点 | RCPPS/SS、RSQRTPS/SS 直接发射 Wasm，显式处理数值类别，不读写 MXCSR，不调用数值 helper。按 ISA 相对误差界和允许的下溢范围验证，不要求复现特定处理器的近似尾数。 |
| MMX 与 x87 共享状态 | MMX 整数/饱和/乘加/水平/符号/排列/移位、GPR/内存搬运、MMX↔XMM 和 EMMS 已直接发射；六种 MMX 浮点转换复用按值数值函数。八个物理 x87 payload 与 CW/SW/tags 驻留；#MF 先于访存，#XM 只提交 MXCSR，失败保留目的和 x87 状态。CVTPI2PD 内存形式保持 x87 状态独立。 |
| 掩码存储 | MASKMOVQ/DQU 支持隐式地址宽度与段覆盖；命中时直接写所选字节，慢路径统一预检所选字节，保留故障事务与源码撤销。未选中字节及空掩码不访问内存，源与指针保持只读。 |
| x87 控制与上下文 | WAIT、FLDCW、FNSTCW/FNSTSW、FNCLEX、FNINIT、FNOP、FINCSTP/FDECSTP、FFREE/FFREEP 已直接发射；IP/DP/opcode 驻留。FLDENV/FNSTENV、FRSTOR/FNSAVE 支持两种旧式布局，FXSAVE/FXRSTOR 支持三种 FX 布局；物理标签与逻辑槽映射、标签分类重建、CW 保留位/ES/B、MXCSR 检查和精确访存已验证。显式镜像使用 VM 独立缓冲区，普通边不提交 CPU backing。 |
| x87 数值指令 | FLD/FILD、FST/FSTP、FIST/FISTP/FISTTP、七种常量、FXCH/FABS/FCHS、四则及反向/弹栈/整数内存形式、FSQRT、FRNDINT、FPREM/FPREM1、FSCALE、FXTRACT、F2XM1、FYL2X/FYL2XP1、FSIN/FCOS/FSINCOS、FPTAN/FPATAN、FBLD/FBSTP、FCOM/FUCOM 和 FCOMI/FUCOMI 的弹栈/非弹栈形式、FTST、FXAM、八种 FCMOV 已接入。栈/TOP/标签及元数据直接发射；数值入口按值接收并返回结果，内存算术及双结果操作整条指令一次调用。条件搬运读取延迟 FLAGS，分类与搬运不调用数值入口；普通边不执行 CPU backing 事务。 |
| 驻留执行与缓存 | 共享表连接已安装模块，普通命中边不提交 CPU backing、不返回 JS；完整 PC、租约和代码/映射/源码 authority 检查。构建新区域遇到有效缓存覆盖即形成外部边，避免重复发射已有代码。生成模块只保留 chunk 和入口解析，外层状态与向量提交统一到共享驻留入口。默认缓存额度 64 MiB，模块身份、入口别名、pin 和 JS 安装实例随源码失效及回收协调。 |

## 接口迁移与宿主状态

完整接口台账为 [api-migration.json](docs/api-migration.json)，由 `python3 tools/probes/audit_api_migration.py` 根据编译后的契约、实际 Wasm 绑定、现行处理体及版本化旧接口基线生成，不读取游戏输入。包含 1222 项契约、89 个接口组、94 个目录外命名调用分支（全部已绑定）、617 项 GDI+ ABI 声明和 631 个 D3D9/DirectSound/DXGI/D3D11/WMI COM 方法，并分别给出历史出处、当前实现和 ABI 声明位置。当前 PE32/AMD64 分别有 1169/1162 项可绑定；旧目录声明为 typed 而新入口尚无绑定的均为零。现有服务另按公开导入声明接入 82 个 DLL 身份、578 组别名导出，其中 PE32/AMD64 分别有 571/573 组 ABI 声明；实际可调用范围由规范 provider 的原资格决定，缺失操作不会因别名而获得实现。逻辑 provider 容量由 64 扩为 128，可容纳当前 116 个已声明身份，保留区域与服务/延迟入口分离。各组直接列出两种 ABI 的缺失名称及复用/适配边界，旧失败 shim、虚构成功和真实 owner 分开核对；此前没有定位到处理代码的四组已补齐来源。typed 范围仍按原处理函数的实际子集核对，目录外分支及 COM 声明单独审查实现和返回协议；绑定、ABI 声明和完整迁移分别记录。

D3D9 的 234 个槽已有共享适配，其中 169 项进入原处理分支，65 项仍返回 D3DERR_NOTAVAILABLE。DirectSound 的 14 项公开声明/ordinal 与 48 个 COM 方法已接入；两个 Create 提供真实对象，其余 12 项公开声明和六项 COM 槽返回 E_NOTIMPL。GDI+ 的 617 项声明均可绑定，25 项复用原处理分支，592 项返回 NotImplemented(6)。PE32 CRT 的 66 项旧函数和 12 项数据已接入，FILE、调用者 on-exit 表注册及非零 callback/new-handler 等限制见对应契约。上述状态及逐项名称由台账生成器维护。

DXGI/D3D11 的 284 项方法现均附带实际能力说明。源码核对确认 85 项在旧共享分派器中没有操作分支并返回方法错误，23 项明确返回不支持/未找到/无接口，另有 CreateGeometryShader 的共享设备处理体因 WebGPU 不支持 stage 8 而失败；完整方法名分别列在台账的 `graphics_capability_gaps`，包括各阶段状态 getter、Dispatch、间接绘制、命令列表、UAV、mip 生成和专用 factory/output 操作。其余方法仍按各自参数、格式和状态子集记录，不按绑定数推算完整 D3D 支持。

AMD64 DXGI/D3D11 已复用 shader 编译、阶段连接、Draw、资源和 canvas Present。typed pending 保留调用帧与线程，输出故障不重放已完成 GPU 操作；子进程首次图形调用前按需建立独立后端，配置失败保留重试，父退出不关闭后代后端，shader 编译器由调用者管理。真实 WKWebView/WebGPU 检查完成 20 个 shader、14 个阶段连接/管线、296 次 Draw、54 次读回和 64 个 canvas 像素；客体资源最终为零，9388 字节可复用绘制缓存在 VM 关闭后归零，编译器独立关闭后堆/arena 归零。

PE32 DirectSound 共用原 PCM、混音、重采样、通知和锁定语义；宿主消费者及 context/工厂按进程拥有或借用，子进程首次 Create/8 前按需初始化，父退出不关闭后代输出。真实 WKWebView/OfflineAudioContext 检查完成 220 个混音帧、2048 个匹配样本及客体资源归零。实际扬声器、iOS 和完整游戏音频尚未验收。

各进程宿主接口已按 PID 接通窗口枚举、RGBA8 像素、client screen 原点、批量输入及 stdin/stdout/stderr，省略 PID 保持原行为；未知 PID 与整个进程组运行中的访问明确拒绝。BrowserPresenter 以 PID/HWND 区分窗口，使用独立软件/GPU canvas，接入标题、可见性、topmost、关闭、物理键盘与 UTF-16 文本、缩放鼠标、精确滚轮、pointer capture 和失焦释放；DOM 回调只积累数据，不执行客体。输入分批不拆键盘/文本配对，失败保留未接受部分。四种父子 ABI 的宿主隔离与浏览器展示检查通过；真实 WKWebView 中两个 ABI 各 192 个 GDI 像素与 DOM 物理按键送入客体的检查通过，关闭后 DOM 窗口为空，一次 setup/最终报告且普通执行宿主请求为零。

生产浏览器启动入口已接通：`tools/run_wasm.py` 预检完整安装目录、文件/路径配额和 Windows 名称，保留空目录，并通过 loopback HTTP/ESM 装载独立核心。`WasmApplication` 统一文件供给、按需输出、工作片和正常入口返回清理，画面同步独立于小工作片；根和子进程按需初始化图形/音频，stdout/stderr 以原始字节传回启动器。163 项启动/目录/TLS/FLS/detach/四种父子 ABI 输出/失败重试/外部清单、时区及范围响应检查、11 项标准库安装及 HTTP/stdio/CPU 样本/时区选择检查通过。正式 HTTP 页面在真实浏览器中执行项目生成的 PE32/AMD64 静态 DLL/TLS/FLS 输入，页面与启动器均返回 37；窗口输入另完成 stdin 回显、真实 DOM KeyZ、退出码 90 及 DOM 窗口清理。启动器使用独立浏览器连接的断开检测，WebView 直接销毁时仍自动返回 130，不依赖卸载事件。外部大文件已按需供给；默认构建和源码清理已完成，当前真实游戏执行状态见性能证据。

CreateProcessW 使用独立 Machine/VMM/线程/JIT 缓存，共享 VFS 节点与文件预约，当前目录独立；STARTUPINFO/EX 句柄列表、挂起、Unicode 环境及标准句柄重定向已接通。普通文件、管道、标准端点、进程与远程初始线程句柄可继承，失败不发布身份和输出。同步对象/token/快照/本地线程继承、跨进程命名同步/注册表、远程内存及跨进程 DuplicateHandle 仍缺失；存在活动 section/view 时仍拒绝创建子进程。

GetSystemTimes 接两种 ABI，cpuTimes/ServiceSample 提供真实 aggregate idle/kernel/user 100ns 累计时间，kernel 包含 idle。生产启动器在 macOS/Linux 已通过独立 `wire-host` 采样库复用既有原生实现；仅在预检成功的服务边界经 HTTP 异步取样，十进制字符串保留完整 u64，宿主 DWORD 错误原样返回。等待期间保留客体状态，拒绝 CPU 修改与进程组服务重入；提供者异常和 VM 销毁不提交结果。352 项稳定样本检查通过；真实宿主→HTTP→同一平台解码→WasmApplication 的 PE32/AMD64 输出逐值一致、各一次采样并正常退出；真实浏览器的正式入口也已验证 GetSystemTimes 成功。原始 VM 未配置提供者及不支持的宿主仍返回 FALSE/50。默认时区配置已接通；原始 VM 省略配置为 UTC，生产启动器默认复用宿主原生时区，可显式选择 UTC。

## 性能证据与真实输入

当前完整安装、正式导入与真实前台 WKWebView/WebGPU 路径已完成 **4000000000 条退休指令、4007413031 work**。最新两版重放的四十个完整 CPU/FLAGS/SIMD/x87 检查点及最终进程、文件、平台样本、窗口、输出和 GPU 行为一致；真实 WebGPU 均完成 932 次调用、122 次提交、7 次 Present。时间从 PE 装载后开始，包含冷 JIT、执行、服务、文件供给及 GPU 操作；核心/着色器准备另计。没有当前同轨迹解释器参照，整体性能目标未完成。

历史诊断宿主没有显示窗口，也没有声明持续交互活动，旧耗时和原生线程归因均属于隐藏视图。相同核心与二十亿条重放在隐藏宿主为 198.602/241.365 秒，可见且声明交互活动时为 72.532 秒，状态与最终行为一致；该变化不计为 VM 源码加速。公共宿主 `tools/probes/webkit_probe.m` 的 `WIRE_MEASURE_FOREGROUND` 选项显示可关闭窗口、保持交互活动并记录页面可见性和遮挡，默认仍为隐藏正确性探针。三个宿主配置的严格 clang 和项目自有 HTML 短检查已通过，私有 PID getter 仍仅用于独立诊断配置。

已撤回的源码证明复用候选按地址空间身份、mapping epoch 和 source version 保存成功验证，饱和时逐页核验；工作区 90 项、宿主/wasm32 严格 Clippy、核心/驻留、线程、异常、动态装卸、启动失败和进程退出回归均通过。为防止跨 owner 和饱和 epoch 下误入旧代码新增的两项测试依赖稳定自有输入。候选和专用测试已撤回，生产恢复到此前保留核心；原始源码、核心、测试与对照保存在 `target/wasm-proof-validation-audit/`。

| 四十亿条前台重放 | 保留基线 | 源码验证复用候选 |
| --- | ---: | ---: |
| 墙钟 | 220.531 秒 | 167.454 秒 |
| WebContent CPU | 555.030 秒 | 430.812 秒 |
| 硬件指令 | 4.20208 万亿 | 4.15198 万亿 |
| 硬件周期 | 1.52458 万亿 | 1.49231 万亿 |
| cycles/CPU-time | 2.7468 GHz | 3.4639 GHz |
| WebContent 物理峰值 | 1752.24 MiB | 1641.41 MiB |

该组顺序为候选、基线；页面持续 visible、窗口未遮挡，系统忙碌比例均约 69.6%，平均分别使用约 2.57/2.52 核。候选出现 nominal/fair，基线全程 fair，cycles/CPU-time 相差约 26%。硬件周期仅少约 2.1%、指令仅少约 1.2%，不采纳 1.317× 墙钟比或峰值差作为稳定源码收益，不追加同轮长测。候选新增模块元数据纳入原预算计费后，累计生成模块为 88537/88410 个、输入字节为 1461957395/1459611289；这些累计输入不是同时活跃的工作集。最终计费均约 64 MiB、实例约 2576 个；总物理内存仍高，iOS 真机和 jetsam 尚未验收。

两版使用同一个可见宿主和冻结平台轨迹。为降低长轨迹诊断的额外驻留，2366931 条平台样本转换为 37870896 字节固定记录与 96 条旁路记录，逐项往返一致；原始与紧凑身份均保留。先前较短范围的 JSON 探针峰值不与本组直接比较。原生资源以 4 Hz 采集，并在执行起止采样 CPU、硬件指令/周期；CPU tick 换算已独立校准。

当前保留的缓存同步修复使用固定 64 项回收通知，替代 JS 按记录逐次调用 wire_live；溢出或 epoch 饱和时全量核验，源码 authority 与执行租约保持，新增通知存储固定约 0.5 KiB。历史隐藏宿主十亿条 ABBA 顺序的墙钟为控制 80.386、候选 70.276、候选 68.991、控制 86.294 秒，完整状态和最终行为一致；该修复保留，前台收益仍待有效复核。一次前台补测受到温控和系统负载差异影响，未采纳其 1.179× 比例。证据在 `target/wasm-retirement-audit/` 与 `target/wasm-visible-gpr-audit/retirement-visible-comparison.json`。

已撤回的其他候选包括调度/服务合并、块延迟计数、普通计数合并、GPR globals ABI 和扩大自然循环覆盖。调度合并没有五亿条收益；普通计数合并的二十亿条收益不足；GPR ABI 前台二十亿条为 74.334 秒，基线 72.532 秒，硬件指令仅少约 0.23%；扩大循环覆盖没有稳定收益。CALL/RET 与非最低地址入口禁用自然循环的限制经源码确认，解除限制的已测实现不保留。冻结证据分别在 `target/wasm-cost-audit/`、`target/wasm-block-accounting-audit/`、`target/wasm-counter-audit/`、`target/wasm-visible-gpr-audit/` 和 `target/wasm-structured-abi-audit/`。

两层缓存 64/4096 项的十亿条原生计数诊断中，TLB 命中率由 97.0739% 增至 99.8217%，Pages HashMap 查询由 51783574 降至 1001243；原生 chunk 均调用 47263969 次，平均每次 21.16 条退休指令。未插桩对照环境不足且包含已撤回计数候选，没有采纳缓存扩容。共享 lookup 主体没有在每条访存处重复发射；原生 chunk 次数、resident.run 次数和服务尝试数分别统计，不能混算 vmexit 或生成代码大小。证据在 `target/wasm-memory-audit/`。

前台四十亿条原生成本归因已完成，四十个完整检查点和最终行为与冻结基线一致。WebContent 为 459.244 CPU 秒，Wasm Worklist Helper 为 292.793 秒（63.755%），主执行及其他线程为 166.438 秒；逐线程与进程累计 CPU 差约 0.0031%。三秒原生栈观察到 OMG/B3/Air 编译及寄存器分配。同步构造时间未覆盖这些后台优化，编译 CPU 份额不直接视为墙钟占比。温控出现 nominal/fair，诊断耗时 184.690 秒，物理峰值 1751.63 MiB，不作为无插桩产品计时。证据在 `target/wasm-foreground-cost-audit/`，旧隐藏记录在 `target/wasm-native-profile-audit/`。

同次低频缓存计数区分出 88409 次模块插入、52165 次容量淘汰、33667 次源码撤销，没有 clear；插入减移除恰等于最终 2577 个模块。累计译码入口 7993950 个、Wasm 输入 1459611227 字节，平均 90.42 个入口/模块、182.59 字节/译码入口，含模块头和 resolver。49678 个不同安装入口 PC 共安装 88409 次，重复安装占 43.81%；重复 PC 未区分容量淘汰和源码变化。最终 Wasm capacity 44.15 MiB，入口别名保守计费 16.33 MiB；这些账目不包含浏览器引擎的全部代码和堆。

首次前台诊断在约 22.95 亿条处收到 SIGTERM（-15），保留的三秒原生栈被复用；补测完成四十亿条。诊断现按检查点落盘 CPU/FP 和缓存计数、按原生采样落盘线程/资源，SIGTERM 写出中断报告后立即退出。项目自有 HTML 已覆盖正常结束和中断保留记录，生产源码未保留诊断计数或私有采样接口。

代码膨胀审计仍须区分静态与动态指令、Wasm locals 与宿主活跃寄存器、基本块入口与私有逐指令恢复入口。当前 resolver 发布全部译码位置，声明 locals 数量不能推出全部溢出；单一模块的静态 Wasm 指令数不能直接乘以真实退休量或按固定周期换算。预算外提仍需精确短预算及故障恢复，TLB 命中必须保留完整地址、范围、权限、epoch 和源码写保护。

模块内 lookup 候选复用相同检查体，每个模块一份、改为本模块直接调用，PE32/AMD64 样例均增加约 125 字节；88 项工作区、严格 Clippy、454 个整数模块及访存/MASKMOV/SIMD/x87/线程/异常回归通过，未新增单元测试。相同诊断配置四十亿条的完整状态和最终行为一致，候选硬件指令增加约 0.87%、周期增加约 4.18%，没有收益，已从四处源码撤回。基线/候选 CPU 为 459.244/509.267 秒、cycles/CPU-time 为 3.0443/2.8600 GHz，物理峰值为 1751.63/1611.88 MiB；不将该峰值差认定为稳定内存收益。冻结源码、两版、形状和回归在 `target/wasm-local-lookup-audit/`。

下一阶段优先审计冷区域跨函数探索、模块寿命及源码撤销范围。已确认页级源码 authority 会撤销整页相关代码，但本次计数尚未区分实际译码字节写入与同页其他写入；应先批量核实撤销来源，再决定是否细化观察范围。不得直接通过降低别名计费或扩大代码缓存来增长未计费的引擎内存。

调度器的通用 Retry 只比较全局 epoch，无关服务或量子轮转都会唤起重试。已有五亿条剖析中 WaitOnAddress 的 810254 次尝试只有 2 次完成、810252 次继续阻塞；这是已确认的粗粒度调度问题，正确唤醒和实际收益仍待实现与验证，尝试数不视为完成调用或宿主挂起时间。

生产普通跨模块边留在 Wasm，有效缓存覆盖不会被新区块重复编译；外层 run 和向量边界由共享驻留入口提供。移除逐模块外层包装的自有 454 项输入对照使生成字节由 573427 降至 363225，入口解析函数逐字节一致；该形状变化尚无独立真实游戏加速比。证据在 `target/wasm-entry-audit/report.json`。

更长的冻结控制轨迹在 4652671660 retired、4660618893 work 处因着色器解析不支持 `DemoteToHelperInvocation` 停止，已完成 7 次 Present。五十亿条及完整游戏尚未通过，着色器能力协商作为独立兼容缺口维护；完整平台记录在 `target/wasm-block-accounting-audit/`。

旧冻结输入曾遗漏八个大文件约 1.12 GB；新生产入口按完整安装目录声明文件与空目录，外部基础长度不占常驻输入额度，主程序保持 256 MiB 上限。外部文件支持按需读写、只读映射、版本资源及静态/动态/延迟 DLL 和子进程装载，元数据与未使用资产不下载。实际完整安装目录已通过生产清单预检：196 个文件、17 个目录、1212987595 字节，最大文件 683129888 字节，全部文件均已登记。

完整目录静态扫描复用原 PE 元数据解析器，检查了 160 个 PE（其中 153 个托管映像）的普通导入、延迟导入和 P/Invoke，并向当前 binder 批量查询 2338 组 DLL/导出。主静态候选图的两个映像与两个动态原生运行库候选均没有未绑定静态入口。全安装仍有 297 处原生导入和两个 ABI 合计 1872 处 P/Invoke 声明缺少入口，按 DLL/名称/进程 ABI 去重为 1922 项。最近一组减少 42 处原生导入、12 处 P/Invoke 和 51 项去重绑定缺口；安装清单、文件元数据、全部 PE 哈希/导入/导出/P/Invoke 请求一致，没有新增缺口。剩余一项同名服务因 API-set 版本导出声明不匹配而缺失；局部 DLL 导出匹配保留候选状态，生产装载器决定搜索及转发，运行时数据导出另行验证。实际能力缺口逐项保留在接口台账，无后端失败协议不计为对应功能实现。

主静态候选图的未绑定入口已消除；NLS 语言排序、磁盘容量、Shell verb/批量操作和 console 后端仍缺失。GetOverlappedResult/Ex 已支持已完成状态和字节数查询；pending 的等待没有接入异步 I/O。

正式入口使用完整安装清单、零诊断导入陷阱和冻结核心运行，以 500000000 条退休指令为目标，实际在 3350851 条退休指令、3360145 work 时退出，退出码 1，逻辑窗口数零。一次正式采样和一次 API 返回链诊断的退休/work/退出码完全一致；日志与 MessageBox 表明 Mono 运行库装载失败。该次运行的原生运行库有 39 项未绑定静态导入，当前已整组接线并静态复核清零；当时只以主静态图零缺口作为门槛，遗漏了动态运行库的已知依赖，后续门槛包含完整运行库组和实际能力审查。两次运行均未达到五亿条目标，不纳入性能验收。控制核心、完整静态差异、运行记录和下一批完整接口清单位于 `target/wasm-runtime-dependencies-audit/`。

动态运行库的 39 项入口均已通过两种 ABI 绑定、正式 PE 导入与稳定输入验证。19 项接入虚拟身份/状态和已有 owner：SID 分配释放、TRUSTEE、本地账户查询、token 复制/模拟/恢复、处理器/卷/语言查询、进程优先级与枚举、SignalObjectAndWait、COM task realloc 和窗口关系；FreeConsole 在未附着状态成功，FlushProcessWriteBuffers 对当前单 agent 内存为空操作。另接入相关 A/W 与 EnumProcesses 声明，复用现有契约的 OpenFileMappingA/W 准入；冻结旧接口基线保持不变。

该组仍缺少的二十项实际能力如下；逐接口的权限、输出和错误约定维护在台账 `wasm_capability`：

| 缺失后端 | 入口与当前返回 |
| --- | --- |
| ACL、安全描述符、认证启动 | GetEffectiveRightsFromAclW、GetNamedSecurityInfoW、SetNamedSecurityInfoW、SetEntriesInAclW 返回 DWORD 50；CreateProcessWithLogonW 返回 FALSE/50。 |
| 同步 I/O 取消与字节锁 | CancelSynchronousIo 验证线程权限后 FALSE/50；LockFile/UnlockFile 验证普通文件句柄后 FALSE/50。 |
| 命名 section | OpenFileMappingW：非空名称 NULL/2，NULL/空名称 87；匿名映射使用已有 owner。 |
| Console 输入与 mode | GetNumberOfConsoleInputEvents、PeekConsoleInputA、ReadConsoleInputW、SetConsoleMode 返回 FALSE/6，输出保持。 |
| 每进程时间与工作集策略 | GetProcessTimes、GetProcessWorkingSetSize、SetProcessWorkingSetSize 验证权限后 FALSE/50，输出/进程状态保持。 |
| Shell association、GUI 启动空闲 | ShellExecuteExW 返回 FALSE/1155；WaitForInputIdle 验证同步权限后 WAIT_FAILED/50。 |
| 多媒体事件调度 | timeSetEvent 返回零且不注册回调，timeKillEvent 返回 97。 |

运行库稳定集成现为 1782 项，SignalObjectAndWait 的 288 项检查通过，覆盖双 ABI、内存 owner、token 级别/线程隔离、关闭后模拟保持、输出事务、单次 signal、APC 故障重试和整组正式导入/实际 JIT 调用。相关旧 token、SID、COM、APC、线程、进程查询、系统 profile、窗口关系/查询、mapping、多媒体及别名回归全部通过。工作区 87 项、宿主/wasm32 严格 Clippy、release、格式及差异空白检查通过；核心保持零 imports/230 exports。控制核心、整组行为审查与全安装静态差异保存在 `target/wasm-managed-runtime-audit/`，旧基线和五个冻结 JIT 文件哈希不变，该兼容组的控制构建未运行游戏；当前长程结果见性能证据。

该组之外的动态查询已有完整聚合记录：五亿条轨迹观察到 451 次 GetProcAddress，其中 130 次返回 NULL；包含 AppPolicy/XState 等可选探测，以及 InitializeCriticalSectionEx、NLS、线程错误模式、文件属性和 console 名称在部分 provider 下的缺口。原 canonical 服务资格继续与 DLL 导出资格分别维护。所有原生映像的离线候选审查记录 680 个直接查询调用点，642 个恢复了常量名称；38 个通过参数/共享 helper 的调用点由真实运行汇总补充。完整静态报告另保留其他本地原生映像和全部 P/Invoke 缺口。

SystemFunction036 已复用 BCrypt 的安全样本与输出事务，BOOLEAN/DWORD 两参数 ABI 及零长度/熵源失败行为已接通；VerLanguageNameA/W 增加公开 version.dll 导出资格。当前台账包含 1222 项契约，PE32/AMD64 可绑定 1169/1162 项，旧 typed 漏接仍为零。公开 COFF 声明为 82 个 DLL、578 个导出对、571/573 个 ABI 声明；冻结旧基线不变。没有新增依赖。

进程堆、私有堆和 COM task 现共享 64 MiB 元数据计费，每个活动块保守计 256 字节，释放/销毁归还额度，移动 realloc 借用原记录到替换完成。单堆不再受 65536 活动块门槛限制，实际数据页继续受 VMM 与 HeapCreate maximum 约束。两项稳定单元回归覆盖两个 ABI 的 70000 个小块及跨 owner 额度/移动调整/销毁；工作区合计 87 项通过。堆 74992 项、COM 605 项、运行库 1782 项、BSTR 330 项、VARIANT 870 项、SAFEARRAY 18674 项、别名 314 项及安全随机数 523 项通过；宿主/wasm32 严格 Clippy、release、格式及空白检查通过。

## 剩余工作与实施顺序

1. 默认构建、时区配置、旧执行源码清理和完整生产目录预检已完成。优先按静态依赖清单批量处理必需的 DLL provider、原有 PE32 行为的 AMD64 适配和目录外入口，明确记录未实现操作的失败协议；实际装载通过后核对辅助程序供给。外部对照保留独立显式入口。
2. 按接口台账核对行为子集，补齐必要的 NLS 排序/字符数据、消息插入/模块消息表、fiber/动态 TLS、可移动 Local/Global、堆扩展、其余线程/Win32 服务、分段阻塞 I/O、控制台、映射类型及元数据 setter。语言排序须有权重、组合/扩展、语言例外和版本标识；公开权重输入的分发授权仍待核对，当前未引入新数据或依赖。
3. 补齐跨进程句柄类型、命名对象/注册表、section 一致性与远程内存。保持创建失败不发布、输出事务、父子各 owner 的独立寿命与文件预约约束。
4. 补齐 USER/GDI 行为缺口：GetWindowDC、其余 DIB/section、字体 realization/metrics/字形、复杂 region/完整重绘，AMD64 类元数据，专用控件、完整销毁通知、相对 z-order、跨 A/W 字符串转换、跨线程输入与通用回调。浏览器 IME/dead key/完整布局及原生桌面 compositor 仍缺失。
5. 按已列 109 项 DXGI/D3D11 缺口推进所需专用方法、格式与 feature-level 子集，继续评估 D3D9 可选 GPU；保持 ABI、COM、资源/视图、状态和 shader 契约，不以默认成功代替缺失操作。
6. 持续审查 ISA 与异常：TF/AC、客体 INT3、#GP 操作数地址、AMD64 exit/collided/LONGJUMP/consolidation、history cache、PE32 nested/collided 展开、跨装载 catch/退出展开、顶层 filter EXECUTE_HANDLER、延迟装载 hook/失败异常。当前未发布 AVX/XSAVE/OSXSAVE、BMI1、LZCNT、AES 和 CLFLUSH；未知指令仍精确停止。
7. 以项目生成输入完成状态/服务/图形集成验证，再冻结真实游戏做少量长轨迹对照，至少五亿条实际退休指令；分别报告冷启动、后段和整体，包含可复核的新旧实现与解释器参照。未达条件不作最终性能结论。

## 验证入口与当前结果

以下工具只使用项目生成输入，不依赖游戏、专有资料、本机测试二进制或原生解释器。测试覆盖状态值、故障提交、预算与恢复、源码失效和实例寿命；数值参照使用独立 BigInt 算法。

| 显式命令 | 当前结果与重点 |
| --- | --- |
| `python3 tools/probes/audit_wasm_installation.py installation/app.exe --root installation` | 显式完整目录静态审计；不执行客体。复用 PE 普通/延迟导入、导出和 CLR 声明解析，批量查询实际 binder/provider，报告本地导出候选和缺失 callable。5 个项目生成输入检查覆盖两个 ABI、名称/ordinal/延迟导入、builtin provider 对本地同名 DLL 的优先级及 CLR 元数据位宽与 P/Invoke 进程 ABI 的分离；报告在显式输出目录中，不纳入常规测试发现。 |
| `node crates/wire-wasm/tests/jit.mjs` | 454 个模块、222116 项检查：整数/FLAGS、控制流、别名与精确边界。 |
| `node crates/wire-wasm/tests/core.mjs` | 核心内存 ABI、内联命中、RMW/栈/宽比较交换、故障与源码撤销。 |
| `node crates/wire-wasm/tests/application.mjs` | 163 项检查：两个 ABI 的主入口正常返回、空目录、静态 DLL/TLS/FLS 与恰好一次 detach、完整清单/时区字节及空槽下载前拒绝、输入变更/构造失败回收、四种父子 ABI 的按需输出/失败重试/父先退出与借用编译器；外部静态 DLL、未使用大文件不下载、错误 Range 响应在读取 body 前拒绝。 |
| `python3 -m unittest discover -s tools -p test_run_wasm.py` | 11 项标准库检查：完整安装目录与空目录、Windows 名称/命令、稀疏大文件元数据与主程序额度、精确 HTTP 路由/MIME/Origin/Host、206/Content-Range/非法范围/传输额度及同大小文件变更拒绝、原始 stdio 与 DWORD 终态、浏览器连接断开回收，以及 CPU 样本完整字符串/宿主错误/拒绝来源不采样；只使用临时输入及 loopback。 |
| `node crates/wire-wasm/tests/file-backing.mjs` | 470 项检查：两种 ABI、1 TiB 逻辑源及高偏移、按需读写/COW/截断零洞/映射、静态与动态 DLL/版本资源/延迟导入和转发、四种父子 ABI 的外部子程序与共享源、输入失败/调用帧/预检优先级/重入锁及销毁。 |
| `python3 tools/probes/check_wasm_file_input.py` | 显式 Node/loopback/支持稀疏文件的文件系统与 release 核心检查：PE32/AMD64 的完整生产清单、1 TiB 高偏移读写/映射及未使用资产；AMD64 另验证 65 MiB 单次传输、消费后驱逐和缓存归限，原宿主文件不变。临时输入自动清理，不进入常规测试发现。 |
| `node crates/wire-wasm/tests/process-host.mjs` | 94 项检查：四种父子 ABI 的进程窗口、同号 HWND 隔离、软件像素、原点、输入唤醒、stdio、运行中拒绝与退出回收。 |
| `node crates/wire-wasm/tests/browser-presenter.mjs` | 47 项检查：浏览器软件/GPU canvas、PID 窗口身份、可见性/标题/topmost、扫描码/UTF-16、缩放鼠标/滚轮、失焦释放、输入失败和配对批次边界、DOM 回收。 |
| `python3 tools/probes/check_wasm_desktop.py --out target/wasm-desktop-check` | 显式 macOS SDK/clang/Node/WKWebView 检查；项目生成的完整 PE32/AMD64 输入验证实际软件画布的各 192 个像素、DOM 键盘投递和关闭，独立于常规测试，不采集性能。 |
| `node crates/wire-wasm/tests/graphics-probes.mjs` | 896 项检查：两种 ABI 的十八项可选入口及原 GetPixelFormat、实际 GDI/USER 句柄、空 context 释放、错误码/LastError/CPU、HRESULT 指针与 DWORD 宽度、输出/IID/返回帧/错误槽故障事务、别名和正式 PE 导入/真实 JIT 分支；无外部输入。 |
| `node crates/wire-wasm/tests/graphics.mjs` | 523 项检查：工厂/adapter/output/设备/COM、private data、JIT 方法、访存/返回帧、typed pending、shader/Draw 一次完成、量子边界线程归属、swapchain/清屏/present、XMM float、执行后配置、子进程按需初始化/失败重试/父先退出、借用编译器、canvas 身份与 bridge 槽回收。 |
| `node crates/wire-wasm/tests/presentation.mjs` | 75 项检查：共享 canvas 的 WebGPU device 身份、owner 交替、两种关闭顺序与 surface LRU 驱逐。 |
| `node crates/wire-wasm/tests/d3d9.mjs` | 954 项检查：234 个实际 COM 槽/内部导出隔离、真实 JIT、固定管线/VS2/PS2 像素、Present 与 GDI DC 共用存储、Reset 身份与 lease、HRESULT 与原始访存/输出/返回槽/堆复用故障、状态块录制保留、引用回收和进程退出/静态拒绝。 |
| `python3 tools/probes/check_wasm_graphics.py --out target/wasm-graphics-check` | 显式 macOS/WKWebView/WebGPU 检查，需本机 Apple SDK、clang、Rust linker 和 release 核心 Wasm；仅编译本项目控制源，真实 GPU 完成更新/复制/清屏/读回/释放，不属于常规单元测试或性能入口。 |
| `python3 tools/probes/check_wasm_graphics.py --out target/wasm-draw-check --draw --shader-artifacts <compiler-output>` | 同一显式环境，加当前 `build_wasm_shaders.py` 的源码/产物哈希匹配输出；本项目完整 PE 一次覆盖资源、二十个 shader、阶段连接、296 次 Draw、54 次读回和 64 个 canvas 像素，并验证客体资源/绘制缓存/借用编译器的独立寿命。 |
| `python3 tools/probes/audit_api_migration.py` | 完整旧契约/实际新绑定、目录外命名调用及 COM 方法迁移台账；输出不依赖外部输入。 |
| `node crates/wire-wasm/tests/winsock.mjs` | 1083 项检查：两种 ABI 全组绑定/序数、WSADATA、启动引用、地址、内核事件互通、故障事务及保存等待帧/deadline。 |
| `node crates/wire-wasm/tests/winsock-names.mjs` | 16799 项检查：两种 ABI 的五十八项新增声明、经典序数和正式 PE 导入、完整 SOCKET/指针与 stdcall、真实 JIT 工作线程的临时缓冲隔离/回收、BSD IPv4/hostent/普通与 Ex 结果布局、local/数字解析与错误、1024 配额与专用释放、公共 heap/VMM 拒绝、输入/输出/错误槽/返回帧故障及无后端失败保持。 |
| `node crates/wire-wasm/tests/apcs.mjs` | 3473 项检查：五类 alertable 等待、队列容量/顺序、完整上下文/参数、网关 callback、嵌套/跨线程唤醒、回调间故障恢复、VEH 与 callback 内退出。 |
| `node crates/wire-wasm/tests/images.mjs` | 25312 项检查：两种 ABI 全导入/实际 JIT、stock/PE 图标资源、A/W 名称、完整模块/字符串/句柄、shared/owned/当前光标、容量/单调身份、损坏资源/格式错误、模块卸载/原址替换及故障事务。 |
| `node crates/wire-wasm/tests/basic-strings.mjs` | 602 项检查：两种 ABI 的十四项绑定/静态导入、实际 JIT、raw byte/UTF-16/CP932、重叠/拼接、signed count、ordinal 显式 NUL/代理单元、完整指针和输出/错误/返回帧故障事务。 |
| `node crates/wire-wasm/tests/windows-printf.mjs` | 580 项检查：四项 A/W API、PE32 cdecl/stdcall、AMD64 寄存器/栈和 va_list、32/64 位数字/指针、CP932 转换/precision/边界、惰性参数、源/格式/参数/返回槽别名、完整高指针及实际 JIT。 |
| `node crates/wire-wasm/tests/resources.mjs` | 839 项检查：两种 ABI 全导入/实际 JIT、数字与 A/W 名称、完整语言/高指针、两类资源视图、映像编辑和保护、RT_STRING/CP932/原始 UTF-16、容量/别名/故障原子性及模块卸载/原址重装。 |
| `node crates/wire-wasm/tests/user-objects.mjs` | 569 项检查：两种 ABI 全导入/实际 JIT、共享名称/UTF-16 容量、Unicode lowercase、atom/名称键、完整属性值、窗口隔离/销毁回调、故障与重试。 |
| `node crates/wire-wasm/tests/send-timeout.mjs` | 1749 项检查：同队列忽略超时、完整消息/结果、A/W、让出、输出重叠与首个/完成后故障、ERRORONEXIT/错误槽修复及真实跨线程拒绝。 |
| `node crates/wire-wasm/tests/user-dependencies.mjs` | 498 项检查：十九项声明/两种 ABI/完整静态导入与七参数 JIT、模态失败 -1、剪贴板所有权保持、桌面查询输出保持、caret profile、前台状态及错误槽/返回帧故障。 |
| `node crates/wire-wasm/tests/fonts.mjs` | 478 项检查：PE32 四项 LOGFONT 创建、编码/精确输入、stock/选择/删除、共用 GDI 额度、metrics 明确失败、故障事务与 JIT。 |
| `node crates/wire-wasm/tests/shell-folders.mjs` | 1382 项检查：两种 ABI 的四项 CSIDL/profile/VFS 验证与创建、A/W 编码、flags/token/类型错误、完整高地址、输出/错误槽/返回帧故障事务、provider 范围及静态导入/实际 JIT。 |
| `node crates/wire-wasm/tests/unavailable-endpoints.mjs` | 244 项检查：两种 ABI 音频/字体无后端协议、完整静态导入/实际 JIT、忽略无效输入、完整指针输出、LastError、返回槽别名和故障事务。 |
| `node crates/wire-wasm/tests/wave.mjs` | 1471 项检查：完整三十九项声明/静态导入、两种 ABI、QUERY 不访问输出、完整句柄清零、无设备/句柄失败、A/W 错误文本截断/实际写入跨度、零容量/未知码、返回槽重叠、跨页故障及六参数实际 JIT。 |
| `node crates/wire-wasm/tests/paths.mjs` | 1074 项检查：两种 ABI 十五项全导入/实际 JIT、根/扩展前缀/DBCS、扩展名/MAX_PATH/引号、NULL/完整指针、VFS 查询/递归目录寿命、输出/输入/错误槽/返回帧故障与别名。 |
| `node crates/wire-wasm/tests/raw-input.mjs` | 35717 项检查：两种 ABI 全入口/PE 导入/实际 JIT、注册原子性、设备布局/名称、真实宿主包/双击/滚轮、NOLEGACY/等待/指针互通、PM_REMOVE/缓冲/默认清理、完整指针/故障与重试、跨线程/窗口退出，以及数据包和消息额度的整批拒绝。 |
| `node crates/wire-wasm/tests/cursor.mjs` | 两种 ABI 的游标/裁剪、位置与消息坐标、指针提升和布局、QS_POINTER、寿命及故障事务。 |
| `node crates/wire-wasm/tests/winhttp.mjs` | 143 项检查：两种 ABI 的 direct proxy 布局、无配置/无 transport 失败、完整指针、返回槽别名及输出/错误槽故障。 |
| `node crates/wire-wasm/tests/ime.mjs` | 268 项检查：两种 ABI 的完整窗口身份、无 context/IME 查询、错误优先级、不访问 composition 输出和错误槽故障。 |
| `node crates/wire-wasm/tests/winrt.mjs` | 534 项检查：两种 ABI 的 HSTRING 所有权/引用头偏移、只读 payload、复制/比较、输出事务、完整指针、真实激活失败、PE 导入及启动拒绝时数据页回收。 |
| `node crates/wire-wasm/tests/sspi.mjs` | 604 项检查：两种 ABI 全入口、A/W 函数表与保留槽、只读 backing、空包列表、名称编码、真实失败协议、客体间接调用、输出/返回别名及跨页事务。 |
| `node crates/wire-wasm/tests/bstr.mjs` | 330 项检查：两种 ABI 的成员/进程堆与独立页、长度/嵌入 NUL/原始 unit、重分配别名/补零、完整指针、ordinal/静态导入、返回槽别名、输出故障和启动拒绝回收。 |
| `node crates/wire-wasm/tests/provider-ordinals.mjs` | 91 项检查：两种 ABI 的 provider ordinal 直接导入、静态/动态转发、GetProcAddress、错误 provider/未知序数、延迟解析及精确 work/退休预算。 |
| `node crates/wire-wasm/tests/safe-arrays.mjs` | 18674 项检查：两种 ABI 的十八项 named/ordinal 静态导入及实际 JIT、多维反序描述符/独立索引、全部支持元素宽度、奇数字节 BSTR、VARIANT/BYREF/嵌套数组深拷贝与 VariantCopy/Clear、锁/空数组/溢出/配额、循环与重复所有权拒绝、部分复制回收、完整指针/输出与首返回帧故障、LastError 保持。 |
| `node crates/wire-wasm/tests/variant.mjs` | 870 项检查：两种 ABI 的 16/24 字节 scalar/BSTR/BYREF、完整高地址、奇数字节 BSTR、自复制/回滚、未实现类型明确错误、输出/错误槽/返回帧事务、named/ordinal 静态导入及实际 JIT；PE32 进程堆共用与 AMD64 独立 owner。 |
| `node crates/wire-wasm/tests/property-values.mjs` | 2122 项检查：两种 ABI/全部五项声明及 API-set、PROPVARIANT 整记录/独立复制、task/BSTR/CLIPDATA/vector/嵌套数组、锁/环/重复拥有、临时对象回收、源/输出/返回故障及真实静态导入/JIT。 |
| `node crates/wire-wasm/tests/platform-endpoints.mjs` | 659 项检查：两种 ABI/23 项完整静态导入、实际 JIT 的按值 VARIANT 和 ICMP 失败分支、完整 INVALID_HANDLE_VALUE、空/非法释放、无可信结果/回调/reply、输出及错误槽/返回帧/结构跨页故障和保存返回槽。 |
| `node crates/wire-wasm/tests/managed-runtime.mjs` | 1782 项检查：SID/TRUSTEE/token 类型与级别、每线程模拟及关闭后身份保持、账户长度和多输出事务、task realloc owner/失败保留、虚拟 profile/priority/PID、窗口关系、无后端失败、整组正式导入与真实 JIT。 |
| `node crates/wire-wasm/tests/signal-and-wait.mjs` | 288 项检查：双 ABI、对象权限/owner/限额、有限等待、单次 signal、返回帧与 APC 准备故障恢复。 |
| `node crates/wire-wasm/tests/runtime-dependencies.mjs` | 912 项检查：两种 ABI 的已完成 IOSB、常见/NTWIN32 错误、pending 短路、完整字节数及输出/错误槽/返回故障、无后端状态保持、全部十五项正式导入和实际 JIT 查询。 |
| `node crates/wire-wasm/tests/devices.mjs` | 238 项检查：AMD64 全入口、空设备集合、高位句柄、HID status/输出保持、订阅寿命、provider 边界及输出/错误槽故障。 |
| `node crates/wire-wasm/tests/multimedia.mjs` | 1154 项检查：两种 ABI 的能力记录、计时请求/引用/容量/VM 隔离、DWORD、故障差异和 LastError 保持。 |
| `node crates/wire-wasm/tests/runtime.mjs` | 跨模块一次驻留执行、缓存调用者/被调者的短预算与延迟 FLAGS、异常恢复、链接 authority、安装失败回收/源码变更/销毁。 |
| `node crates/wire-wasm/tests/import-traps.mjs` | 354 项检查：初始与后续动态 DLL 的显式诊断、两种 ABI/原始 TLS 与 DllMain、名称/ordinal 停止现场、无伪造输出/返回/work、重试保持、严格查询与装载回滚。 |
| `node crates/wire-wasm/tests/cpu.mjs` | 8861 项检查：固定 CPUID、时间及主机错误恢复、CRC32C 独立多项式参照、POPCNT、特性相关位扫描、提示/fence/MOVNTI、精确访存、短预算、延迟 FLAGS、导入边界和跨模块驻留隔离。 |
| `node crates/wire-wasm/tests/gateways.mjs` | 244 项检查：两种布局、四类逻辑出口、映射代码和 guard、直接/间接转移、CALL 返回帧、FLAGS、预算优先级、跨入口指令与缓存边界。 |
| `node crates/wire-wasm/tests/services.mjs` | 301 项检查：六种时钟 API 的实际 JIT 调用/服务/继续链路、DLL 资格和绑定隔离、PE32 宽返回、保存返回地址后的输出别名、跨页失败与重试、时钟错误与 FILETIME、活动租约、FLAGS/FP/SIMD 保持和源码撤销。 |
| `node crates/wire-wasm/tests/crt-startup.mjs` | 823 项检查：十二项原始数据的 PE 导入/动态查询/共享访问器、参数/宽字符/环境、动态回滚及暂存地址避让、控制字事务、初始化/排序的真实 JIT 续点与故障进度、DllMain 深度、normal/quick 退出/返回槽/回调故障、真实锁等待及拒绝回收。 |
| `node crates/wire-wasm/tests/crt.mjs` | 1252 项检查：PE32 cdecl/PE 导入/JIT、四类 provider/fresh 查询、无符号字节/CP932、strtoul、真实线程 errno、x87 数学/EDX:EAX/舍入、FILETIME 边界、完整输入与双输出故障事务、别名返回、拒绝回收及大请求拒绝。 |
| `node crates/wire-wasm/tests/dsound.mjs` | 1035 项检查：公开签名/ordinal/COM 资格、引用/通知/真实等待、PCM/混音/重采样/循环、环形锁/控制、静态导入/JIT、背压、全部声部故障事务、独立音频/CPU fault、子进程独立/按需输出、配置失败重试及父先退出与关闭一次。 |
| `node crates/wire-wasm/tests/gdiplus.mjs` | 602 项检查：启动/矩阵/分配全子集、实际 PE/JIT/动态查询、按值 GUID/最长清栈、明确未实现状态、完整点批次故障、返回槽/输入别名及启动拒绝清理。 |
| `node crates/wire-wasm/tests/wmi.mjs` | 1407 项检查：PE32 全四套 vtable、已实现查询/枚举/BSTR、原不支持槽位、实际 JIT、NULL/跨页/输入/首帧故障、清栈/别名和启动拒绝清理。 |
| `node crates/wire-wasm/tests/com.mjs` | 605 项检查：两种 ABI 的共享 COM/WinRT 引用、线程隔离、独立任务分配/跨线程释放、未注册工厂失败协议、调用帧和输出故障事务。 |
| `node crates/wire-wasm/tests/crypto.mjs` | 17936 项检查：独立 Node 摘要参照、五种算法块边界/流式输入、复制与完成状态、命名/默认 provider、引用/失效、双 ABI/完整指针、故障事务/配额/VM 隔离及全九项静态导入和实际 JIT 调用。 |
| `node crates/wire-wasm/tests/bcrypt.mjs` | 523 项检查：两种 ABI 的 provider、NTSTATUS、任意长度随机输入、零长度/上限、预检/提供者错误、别名及故障事务。 |
| `node crates/wire-wasm/tests/desktop.mjs` | 449 项检查：两种 ABI 的设置/profile、完整 station/desktop 失败协议、DWM 子集、隔离、完整输出和错误槽/返回帧故障。 |
| `node crates/wire-wasm/tests/guid.mjs` | 609 项检查：两种 ABI 的文本/混合端序、输入和容量错误、任务字符串所有权、v4 位与随机源、预检和提供者错误保持。 |
| `node crates/wire-wasm/tests/loader.mjs` | 485 项检查：两种格式、节/头/BSS、低对齐共享页、IAT/ILT 回退、首选/显式/冲突重定位、初始线程和静态 TLS 回调、单 work 预算、服务自动处理及恢复、失败装载隔离、畸形元数据、PE32 DEP 与源码撤销。 |
| `node crates/wire-wasm/tests/modules.mjs` | 527 项检查：依赖图/IAT、DLL 初始化顺序与共享依赖、名称/ordinal/数据导出、客户及 provider 转发、循环导入/转发拒绝、独立 TLS/回调与通知参数、短预算、失败与写帧重试、绑定/映射隔离、输入所有权、provider 身份和畸形导出。 |
| `node crates/wire-wasm/tests/provider-aliases.mjs` | 314 项检查：七组 provider 的静态 IAT/实际 JIT、动态加载及首次导出查询、DLL 版本和大小写资格、规范/别名共享堆/文件/COM 状态、故障事务、独立模块身份和 FROM_ADDRESS 回退、转发/延迟导入及真实 work/退休计数。 |
| `node crates/wire-wasm/tests/module-services.mjs` | 892 项检查：已装载身份、规范路径、A/W 文件名与截断、函数/数据/ordinal/转发、provider 资格与保留区冲突隔离、TEB/PEB 身份和 LastError、完整句柄、保存返回帧及输出别名、跨页/错误槽故障与重试、FLAGS/SIMD 保持、实际 JIT 调用和单 work 预算、源码撤销；不采样时钟。 |
| `node crates/wire-wasm/tests/dynamic-modules.mjs` | 3670 项检查：动态 A/W/Ex 装载、已有/新增依赖、保留/释放引用、重入 DllMain、失败逆序清理、失败依赖保留与拒绝查找、共享及循环依赖回收、独立 TLS 与槽重用、单 work 预算、首帧和清理输出故障重试、GetModuleHandleEx 引用/pin/地址归属与事务输出、线程通知禁用契约；每架构 70 次装卸后元数据、生成代码和 JS 实例恢复基线。 |
| `node crates/wire-wasm/tests/module-search.mjs` | 3776 项检查：统一静态/动态/转发/延迟搜索、同名 DLL 完整路径身份、默认及受限搜索标志、A/W DLL 目录与 CWD、NULL/空字符串、跨页/错误槽故障与返回帧别名、provider 路径约束和失败装载命名空间隔离。 |
| `node crates/wire-wasm/tests/files.mjs` | 15961 项检查：同步 I/O、EOF/稀疏延长/截断、64 位定位、只读创建、A/W 路径变更、元数据布局与身份、别名/游标/权限/标志、共享和 DLL 映像寿命、安全属性拒绝、跨页/错误输出故障与重试、时间源失败、1024 句柄边界及 ReadFile 源码撤销。 |
| `node crates/wire-wasm/tests/file-enumeration.mjs` | 58365 项检查：A/W/Ex 查找与 CP932/UTF-16、通配符/排序/路径快照、固定布局/文件属性、普通句柄与查找隔离、4096 活动查找上限及关闭后新身份、目录批次/重启/共享游标、NT 状态/IOSB/低字节 BOOLEAN、返回帧别名及跨页/错误槽故障恢复。 |
| `node crates/wire-wasm/tests/mappings.mjs` | 29530 项检查：两种 ABI、A/W 创建、64 位固定地址/offset、只读/guard、文件修改与多视图同步、源码撤销、section/别名/view 寿命、截断拒绝、输出/调用帧故障、失效 backing 的原子拒绝、启动失败清理及 section/view 容量边界。 |
| `node crates/wire-wasm/tests/threads.mjs` | 2188 项检查：两种架构的计算线程时间片、完整 CPU/FP/SIMD/FLAGS 隔离、独立模板与动态全线程 TLS、加载锁下已启动线程 I/O 与背压唤醒、新线程通知序列/禁用、等待 deadline/调用帧、创建和卸载输出故障重试、非法入口修复、别名权限/挂起属性、64 槽上限/身份复用保护、初始线程退出及静态拒绝清理、单 work 阶段恢复与 REP 比较跨线程续点。 |
| `node crates/wire-wasm/tests/sync.mjs` | 15757 项检查：PulseEvent 单/全唤醒、wait-all、超时/APC/timer、无持久信号、完成冻结与事件/信号量/递归互斥体/abandonment、相对/绝对/周期定时器、Cancel、混合原子等待、名称/权限/别名、输出/时钟故障、待完成调用寿命/取消、安全属性和容量。 |
| `node crates/wire-wasm/tests/exceptions.mjs` | 9205 项检查：AMD64 四参数 RtlUnwind 的完整返回值/内部上下文、三种 provider、查询/清理及跨 filter 展开、初始/最终故障事务，两种 ABI 的显式 filter、完整指针/LONG/状态恢复与 AMD64 filter 目标展开、共享链展开/摘链/CONTEXT、重复 catch 续点回收与软件/CPU/访存/取指/guard/x87/SIMD 异常、Windows 记录与受检 CONTEXT、嵌套、PE32 注册链/filter、单 work 预算、返回故障、原异常保留、线程让出/退出、装载回调交错、未处理/非法 disposition 与注册容量/身份；动态数组及回调函数表共享容量、完整输入/返回指针、范围/名称/首帧校验、输出别名、返回记录/输出故障和无效范围修复、阻塞 I/O/装载/线程退出；静态表装卸寿命、畸形 PE 表拒绝、语言搜索及展开预检的四次回调后元数据 guard 重试与 cleanup 延后；直接目标展开的外部/NULL 记录、未初始化 CONTEXT 存储、原记录保持、查询回调、cleanup 顺序/让出、跨页最终写回故障、输入预检及不可达目标/特殊恢复拒绝；跨 VEH/嵌套异常/显式及内部查询回调的自身 cleanup、调用者栈衔接、外层回调目标保留、放弃查询输出、最终写回重试、后续异常复用及 collided/装载边界拒绝。 |
| `node crates/wire-wasm/tests/virtual-unwind.mjs` | 484 项检查：两种 ABI 的实际映像范围/动态装卸/逻辑 provider/输出别名与跨页故障；AMD64 调用者栈、未初始化字段保持、固定帧/动态 RSP、handler 标志与输出、保存 GPR/XMM 地址、部分 prologue/epilogue/零 opcode、chained/间接记录、machine frame、far save/宽基址、完整输出预检、guard/元数据故障和非法输入。 |
| `node crates/wire-wasm/tests/messages.mjs` | 7812 项检查：MSG32/64、窗口/范围/线程过滤及 quit、metadata、跨页/guard 输出重试、返回帧别名、阻塞参数/返回保存、WaitMessage 新旧输入及无 work 忙等、跨线程 Post/退出、完整 HWND/LRESULT 和 Dispatch 清栈、host 批次原子拒绝/鼠标坐标/时间/文本/A-W 翻译、窗口枚举/销毁、容量和 provider 动态查询；分类新旧输入/INPUTAVAILABLE、对象消息 wait-all 的原子性/首次 deadline/引用寿命、消息与可等待计时器唤醒、timer 合并/替换/完整 ID/1024 容量/取消/时钟失败/最终 deadline 及 TIMERPROC 参数/清栈/void 结果。 |
| `node crates/wire-wasm/tests/user-state.mjs` | 1171 项检查：两种 ABI 的 A/W 文本过程及输出重试、通知重入销毁/首次 frame 故障、focus/capture 宿主路由、WINDOWPOS/NCCALCSIZE/默认 MOVE-SIZE/短预算/记录 guard 重试、topmost/无效 flags/溢出、异步与队列键态/US 布局/HKL 宽度/符号容量/输入输出别名/ToUnicode 与宿主文本隔离。 |
| `node crates/wire-wasm/tests/virtual-memory.mjs` | 996 项检查：两种 ABI/Ex 的 reserve/commit/query/protect/decommit/release、完整句柄/SIZE_T/4 GiB 以上保留与查询、输出跨页故障/返回帧别名、错误槽与首 frame 重试、DWORD 截断、非易失状态/FLAGS/FP/SIMD 保持、代码保护/修改/反提交/同址重用、DIB 所有权、flush 与 kernelbase 动态查询/归属。高地址范围先以 TOP_DOWN 查找并释放，再按完整固定地址重新保留。 |
| `node crates/wire-wasm/tests/process-create.mjs` | 1636 项检查：父子位宽组合、真实独立执行、CPU 提供者继承、继承列表/文件游标/双向管道/标准 I/O/进程句柄、共享预算、启动/环境、挂起/等待、父先退出、正常/强制退出与故障回滚。 |
| `node crates/wire-wasm/tests/known-folders.mjs` | 381 项检查：两种 ABI 的五种 GUID/profile、目录验证与创建、COM task 释放、DWORD/完整指针、输入输出别名及输出故障不创建目录或消耗分配。 |
| `node crates/wire-wasm/tests/shell-arguments.mjs` | 1825 项检查：两种 ABI 的原始 UTF-16、引号/反斜杠、空输入、连续 Local 参数表、NULL 终止指针、别名、高地址、LocalFree 与输出/输入/空闲块故障事务。 |
| `node crates/wire-wasm/tests/process-memory-info.mjs` | 340 项检查：两种 ABI 的基础/EX 尺寸、完整句柄、DWORD 容量、实时提交量/私有量/峰值、QUERY/QUERY_LIMITED 权限、provider、输出边界/跨页事务及返回帧别名。 |
| `node crates/wire-wasm/tests/process-queries.mjs` | 15273 项检查：两种 ABI 的 PID/TEB、OpenProcess、完整句柄、查询/亲和性/processor/group/package、标志/保护/容量、伪句柄转换/复制/缩减权限/源和目标进程权限、真实句柄 Ex 内存/flush、混合等待/不消费部分信号、别名身份与有限等待、跨页输出/保存帧和复制故障。 |
| `node crates/wire-wasm/tests/process-memory.mjs` | 340 项检查：两种 ABI 的 VM_READ/WRITE/OPERATION 组合权限、完整 SIZE_T/句柄/高地址、计数/原帧别名、零长度、远端只读/guard/跨页失败、不消费 guard、数据与计数/错误槽事务、跨多个复制块的双向重叠，以及写入代码后撤销并重编译旧入口。 |
| `node crates/wire-wasm/tests/thread-contexts.mjs` | 391 项检查：两种 ABI 的选择性 control/integer/segments/FP、未请求字节保持、Get/Set 独立权限/复制缩减、暂停/当前线程/退出身份、完整 PC/SP、特权位归一化、MXCSR/非规范地址拒绝、结构对齐与跨页读写故障、RtlCaptureContext 的原 RAX、返回状态、FP 与 provider 查询。 |
| `node crates/wire-wasm/tests/snapshots.mjs` | 2255 项检查：两种 ABI 的 Toolhelp 布局/大小、创建时线程/优先级、创建/退出后旧快照保持、ANSI/Wide 共享游标、跨页输出不推进、返回槽别名、完整句柄、继承/防关闭/不重用、128 活动容量及 flags 优先级、未请求类型和 provider 资格。 |
| `node crates/wire-wasm/tests/thread-descriptions.mjs` | 332 项检查：两种 ABI 的原始 UTF-16/代理项、NULL/空名称、32767 单元边界、独立本地分配与 LocalFree、权限/句柄/退出寿命、HRESULT/LastError、输入/输出跨页故障及旧名称保持。 |
| `node crates/wire-wasm/tests/process-options.mjs` | 581 项检查：两种 ABI 的进程/线程错误模式、sticky 对齐位、线程 mask 与输出故障、DWORD/指针/SIZE_T、属性容量查询/实际尺寸/列表与错误槽原子性、八类元数据形状、调用者值指针、保留参数/重复/满/删除/高地址及 provider 查询。 |
| `node crates/wire-wasm/tests/debug-output.mjs` | 124 项检查：两种 ABI 的原始 ANSI/UTF-16/CP932、NULL、无接收端完成、字符串/首帧故障；VEH 返回后的宽转窄记录写入与最终 LastError 写回故障不重放处理器，原预算和累计计数保持。 |
| `node crates/wire-wasm/tests/breakpoints.mjs` | 291 项检查：两种 ABI 的 DebugBreak 记录/CONTEXT PC/SP、跳过/原位重试/重定向、让出、首次 guard 与返回栈故障恢复、FLAGS/SIMD 和 LastError、未处理停止；AMD64 语言搜索跨逻辑入口及处理器内目标展开的调用者桥接。 |
| `node crates/wire-wasm/tests/free-and-exit.mjs` | 1338 项检查：两种 ABI 的 worker 自卸载、不返回已释放代码、剩余引用/逻辑 provider/无效模块仍退出、TLS/通知/退出码、首次栈帧预检与最终 TLS 输出故障、不重放通知、活动装载回调的无副作用拒绝。 |
| `node crates/wire-wasm/tests/process-exit.mjs` | 979 项检查：两种 ABI 的正常/强制退出、完整句柄和缩减权限、DWORD/259 退出码、不可读返回槽、返回/错误槽故障、当前线程 FLS 与进程分离顺序、后继帧故障不重放、worker 发起退出及其余线程停止、活动加载/递归退出拒绝与强制中断、终态编辑/零预算/重复调用、生成实例回收和最终输出/内存/USER 寿命。 |
| `node crates/wire-wasm/tests/system-profile.mjs` | 2114 项检查：虚拟固定卷 A/W 输入及 AC/无电池 profile；两种 ABI 的硬件/内存/拓扑布局、实际提交/保留/释放和 4 GiB 以上范围、容量/长度/错误槽事务、输出故障重试/保存返回槽、命令行地址/启动记录、CPUID 对应能力及 DEP、DWORD 截断、FLAGS/FP/SIMD 保持与 provider 资格；不采样时钟。 |
| `node crates/wire-wasm/tests/heaps.mjs` | 74992 项检查：两种 ABI 的 HeapQueryInformation 普通模式、完整 SIZE_T、短容量/未实现类、双输出故障/错误槽原子性及别名，两种 ABI 的进程/私有堆、PEB 身份、尺寸/对齐/容量/枚举/跨堆拒绝、空闲合并/128 KiB 小块提交与释放、原址及移动扩容/清零、跨页写入与源/错误槽/返回帧故障重试、完整 SIZE_T/句柄、固定 Local/Global 全部别名与 Unlock 区别、可执行堆代码重用/源码撤销、静态拒绝清理、FLAGS/FP/SIMD 保持与 provider 资格；不采样时钟。 |
| `node crates/wire-wasm/tests/locals-critical.mjs` | 4036 项检查：两种 ABI 的动态槽额度/重用、完整指针/DWORD、错误槽、跨页/权限/首返回帧故障、FLS 析构实参和预算、首帧与后续帧重试不重放、退出码保持、critical section 布局/递归/窄写/竞争/阻塞调用保存、输出与原帧别名、FLAGS/FP/SIMD 保持、provider 资格；锁和消息等待遇 guard 后在 VEH 中调用 Sleep，仍能恢复原参数/返回并正确完成。 |
| `node crates/wire-wasm/tests/inline-sync.mjs` | 777 项检查：两种 ABI 的 SRW token/共享计数/独占及 Try、完整宽度、只读与跨页写入、InitOnce 完整 Context/失败重试/输出别名与故障、条件等待首次 deadline/零超时/原锁模式重获/错误槽故障/嵌套处理器续点，以及 provider 资格和无关 CPU 状态保持。 |
| `node crates/wire-wasm/tests/address-waits.mjs` | 274 项检查：两种 ABI 的 1/2/4/8 字节比较、完整 SIZE_T/高地址、无效尺寸先于访存、初始跨页/错误输出故障、保存参数/返回/首次 deadline、重试不重读比较内存、超时后停止时钟、处理器中 Sleep 保留外层等待、CPU 状态保持及同步 API-set 资格。 |
| `node crates/wire-wasm/tests/environment.mjs` | 356 项检查：两种 ABI 的已有映射与启动分配隔离、虚拟初始变量、PEB 环境与字节数、大小写名称、CP932/UTF-16 容量、变量修改与独立 ANSI/Wide 快照、专用释放/错误编码/重复释放、普通 heap/VMM free 拒绝、输出及参数镜像故障的原子性、无分配泄漏、返回槽别名、静态拒绝清理和 CPU 状态保持。 |
| `node crates/wire-wasm/tests/system-paths.mjs` | 708 项检查：两种 ABI 的十项目录查询、A/W 参数顺序与 NUL 计数、DWORD 截断/完整高地址、容量不足不读输出、跨页输出故障、TMP/TEMP/USERPROFILE 选择与相对路径解析、返回槽别名及 kernelbase 动态资格；不采样时钟。 |
| `node crates/wire-wasm/tests/nls.mjs` | 7142 项检查：两种 ABI 的 28 项服务、CPINFO/Ex 与 DBCS、代码页与 locale 数据、CP932/CP1252/UTF-8 转换、严格/宽松无效序列、代理项、自定义替换与禁用 best-fit、有符号长度/内嵌 NUL/零容量、精确读取边界、完整输出与 used-default 预检/别名/故障、错误槽与返回槽、C1/大小写及 CPU 状态保持；不采样时钟。 |
| `node crates/wire-wasm/tests/nls-calendar.mjs` | 4646 项检查：两种 ABI 的六个 A/W/Ex 日期/时间接口、公历边界/weekday/被忽略字段、16 类 TIME 标志组合与自定义图片、单引号/原始 UTF-16、CP932/CP1252/显式 ACP、完整高地址/DWORD、容量/别名/输入/输出/错误槽故障、NULL 输入按需墙钟及提供者错误保持。 |
| `node crates/wire-wasm/tests/nls-callbacks.mjs` | 2983 项检查：两种 ABI 的 A/W/Ex 枚举词汇、flags 并集、BOOL/提前停止、完整 lParam、首字符串/帧预检、下一帧故障冻结返回与游标、普通代码故障/修复、32 层容量、嵌套/让出/异常、DllMain/线程内退出/AMD64 目标展开，以及 TEB locale、CP_THREAD_ACP、默认/neutral 归一化、线程隔离和输出故障保持。 |
| `node crates/wire-wasm/tests/nls-characters.mjs` | 1119 项检查：两种 ABI 的 ANSI/Ex C1 分类与大小写、DBCS 实际输出个数、忽略 ExW locale、CP932/CP1252/ACP/invariant、任意负计数/内嵌 NUL、完整及未配对代理项、容量/别名/返回槽/跨页故障、精确读取边界、高地址/DWORD 和无关 CPU 状态保持。 |
| `node crates/wire-wasm/tests/startup-queries.mjs` | 1418 项检查：计算机/用户名格式与 NUL 计数、长度及错误槽原子性、VFS 当前目录/完整路径/file-part 编码偏移/高地址、环境单次展开/未知变量/放大额度、版本 A/W/CSD/Service Pack 布局、层级比较、完整 64 位 mask/Win32 清栈/EDX:EAX 返回、ANSI 精确读取边界、NTSTATUS 与 provider 范围；不采样时钟。 |
| `node crates/wire-wasm/tests/version-resources.mjs` | 1257 项检查：客体/文件两种架构交叉、六项 A/W 接口、原始资源大小/转换区域/截断复制、fixed/translation/空值及 CP932、ID/语言回退、畸形资源和 VFS 共享错误、完整指针/DWORD/返回槽别名，以及跨页输出/转换区/错误槽/首返回帧故障原子性。 |
| `node crates/wire-wasm/tests/timezone.mjs` | 178 项检查：配置复制/拒绝与 owner 回收、固定偏移跨日、冬夏模式与当前季节 FILETIME 精度、默认双向转换、空日期/时间格式、CRT 变量、装载后重配拒绝及四种父子 ABI 继承。 |
| `python3 tools/probes/check_wasm_timezone.py` | 显式 macOS/Linux、Node、loopback、原生时区库与 release 核心检查；12 个宿主/UTC/南北半球冬夏案例的 Windows/NLS/FILETIME/默认转换逐值核对，不采集性能。 |
| `node crates/wire-wasm/tests/calendar.mjs` | 882 项检查：两种 ABI 的独立公历参照、闰年/日期范围/精度、默认 UTC 与显式相对/绝对 DST/南半球/重叠/缺口、完整 FILETIME 比较/指针、别名和输入/输出/错误槽/返回帧故障重试、墙钟错误及 CPU 状态保持。 |
| `node crates/wire-wasm/tests/cpu-times.mjs` | 352 项检查：两种 ABI 的完整累计时长、NULL/高地址/重叠/返回槽、独立提供者、参数/批量输出/错误槽/返回帧故障先于采样、异步等待/拒绝/类型/范围/重试/CPU 修改保护/销毁不提交、JSON u64 精度与宿主 DWORD 错误，以及真实 JIT 静态导入。 |
| `python3 tools/probes/check_wasm_cpu.py` | 显式 macOS/Linux 宿主检查，复用独立采样库、生产 HTTP/样本解码/WasmApplication，逐值核对 PE32/AMD64 的一次真实采样、正常退出及资源回收；记录核心/宿主哈希，不依赖旧执行器，也不进入常规测试发现范围。 |
| `node crates/wire-wasm/tests/pointer-interlocked.mjs` | 2939 项检查：独立 BigInt MulDiv 参照、完整指针/SIZE_T、零长度/guard/DEP、LONG 回绕与只读比较失败、SLIST 两种布局/深度/序列回绕/高地址、写入预检和故障重试、原返回帧别名、代码撤销、CPU/LastError 保持与 provider 资格。 |
| `node crates/wire-wasm/tests/registry.mjs` | 76050 项检查：17 接口与两种 ABI、完整根/句柄/高地址、generic/MAXIMUM_ALLOWED/只读权限、CP932/UTF-16/原始值、旧式子路径默认值、长度/MORE_DATA/补终止符、输出及返回槽别名、首帧/源/跨页/guard 故障事务、动态查询、VM 隔离与 4096 次句柄身份耗尽；不采样时钟。 |
| `node crates/wire-wasm/tests/tokens.mjs` | 2174 项检查：两种 ABI 的 token/SID 指针与六类结构、真实/伪进程和线程查询权限、无模拟令牌、LastError/CPU、复制/缩减权限/保护关闭、128 活动容量、不复用身份、跨页输出/错误槽/首帧/guard 事务及别名。 |
| `node crates/wire-wasm/tests/sids.mjs` | 509 项检查：两种 ABI 的 BYTE/DWORD/完整指针、字段取址、头部验证与短路/精确字节、零初始化及 NULL authority、长度/相等/重叠复制、原返回槽及源/输出/错误槽/guard 故障恢复。 |
| `node crates/wire-wasm/tests/registry-enumeration.mjs` | 1825 项检查：两种 ABI 的原始名称拼写、A/W 字符与字节容量、未命名/任意值和元数据、删除后旧句柄及同路径重建、叶子/树权限、WOW 视图、明确不支持的元数据/Flush、批量输出故障/返回槽别名、全新增声明静态导入及真实 JIT 调用。 |
| `cargo test --manifest-path crates/wire-wasm/Cargo.toml --locked --offline --target-dir target/wasm-rewrite registry --lib` | 三项共享状态回归：整条父路径节点/字节配额失败不发布、值替换/删除释放配额、删树/重建后旧句柄与首次名称拼写。 |
| `node crates/wire-wasm/tests/message-format.mjs` | 1222 项检查：两种 ABI 的系统/字符串消息、CP932/UTF-16 计数、flags/语言/占位限制、短容量、Local 分配/尺寸/释放与空闲复用、完整指针/DWORD 截断、输入/输出/错误槽/首帧/guard 故障重试、别名、CPU/LastError 和 provider 资格。 |
| `node crates/wire-wasm/tests/file-operations.mjs` | 2708 项检查：文件替换/备份/元数据/身份/只读映射/失败保持、无回调扩展复制/取消、改名/删除重建、公开词法路径用例/CP932/UTF16/MAX_PATH、双 ABI 和输入/输出/错误槽故障、全六项静态导入及真实 JIT 调用。 |
| `node crates/wire-wasm/tests/file-mutations.mjs` | 1516 项检查：CopyFile A/W 的 COW/节点身份、共享/映像/section 拒绝、全宽文件时间、写属性权限及复制缩减、全 1 时间抑制/共享/独立打开与无采样、临时文件碰撞/前缀/提示/编码/高地址、输出故障/序号事务与 provider 资格。 |
| `node crates/wire-wasm/tests/dib.mjs` | 1076 项检查：两种 ABI/两种行方向/RGB 与 BITFIELDS、直接客体写入和 GDI 写回、DIBSECTION/部分 BITMAP 输出、选中 DIB 的兼容创建/零尺寸、删除/映射寿命、GetDIBits 方向/批次/别名/跨页重试、重叠 blit/缩放/裁剪、全部目的行预检、指针失败不消耗身份、PE32 无 DEP 的 DIB 源码修改/删除失效、静态 DllMain 拒绝后的 DIB backing 回收。 |
| `node crates/wire-wasm/tests/painting.mjs` | 13090 项检查：两种 ABI 的窗口像素/独立快照、兼容位图/stock brush、对象布局/所有权、源重叠 SRCCOPY、矩形合并/裁剪/擦除、完整 PAINTSTRUCT 预检、擦除后输出故障恢复和返回槽别名、EndPaint 保留新区域、QS_PAINT 与输入 mask 隔离、合成/显式消息寿命、UpdateWindow/默认 paint、resize 保留/拒绝及错误槽重试、origin 输出事务、DC 容量/单调身份和展开跳出擦除时的 DC 回收。 |
| `node crates/wire-wasm/tests/geometry.mjs` | 631 项检查：两种 ABI 的十三项 RECT/96-DPI 边框、按值 POINT、signed/空值/回绕、输入输出重叠和返回槽别名、NULL 布尔语义、完整指针、状态保持及输出故障原子性。 |
| `node crates/wire-wasm/tests/display.mjs` | 2449 项检查：两种 ABI 的 adapter/monitor/mode/metrics 记录、SDK 标志、完整句柄/指针/packed POINT、窗口和交集、错误/输出保持、裁剪 RECT/回调 BOOL/短预算/临时帧故障/取指修复、调用上下文恢复与 provider 范围。 |
| `node crates/wire-wasm/tests/window-relationships.mjs` | 555 项检查：两种 ABI 的 child/parent 创建和坐标、完整 control ID/低 DWORD 查找、带边框换父/循环拒绝、父移动后查询、宿主 mouse/capture、子窗口/资源清理、owner/message-only、EnableWindow 通知与首帧故障、DefDlgProc 文本/关闭。 |
| `node crates/wire-wasm/tests/dialog.mjs` | 909 项检查：两种 ABI 控件/整数/枚举、完整消息参数、回调让出/装载/退出、模态失败 -1、静态导入/JIT 及输出/返回帧故障。 |
| `node crates/wire-wasm/tests/window-queries.mjs` | 432 项检查：两种 ABI 的桌面/状态/坐标及 AMD64 placement、完整静态导入与实际 JIT，PE32 的 atom/AW/Unicode 查找、逻辑桌面身份/类/生命周期、状态位与 LastError、POINT wrapping/批次/屏幕原点/零数量、44 字节 placement/高地址/预检次序/结构和返回槽别名及故障恢复。 |
| `node crates/wire-wasm/tests/menus.mjs` | 1277 项检查：PE32 全二十七项静态导入/实际 JIT、二十项对象状态和七项明确失败协议、44/48 字节条目/掩码/bitmap、CP932/原始 UTF-16、递归命令/共享子菜单/30 层回滚、双输出/别名/故障恢复、窗口菜单/客户区/DC 像素、失败创建/共享窗口和线程退休及启动拒绝。 |
| `node crates/wire-wasm/tests/classes.mjs` | 1024 项检查：PE32 十九项完整静态导入/实际 JIT、元数据/持久 A/W 名称和菜单、替换 UTF-16、extra/过程/实例/全局匹配、基本类/controls、dwSize 精确读取、跨 A/W 长标题、变长 frame 故障恢复、返回槽别名、显式注册失败和静态拒绝 backing 回收；AMD64 保留原 absent 范围。 |
| `node crates/wire-wasm/tests/windows.mjs` | 2058 项检查：AMD64 跨 A/W 创建的标题/类名转换与 CREATESTRUCT 栈寿命，两种 ABI/A/W 类注册与名称策略、完整 HWND/LRESULT、创建/显示/销毁/拒绝清理、CREATESTRUCT、基本查询/extra、过程转发、初始/下一 frame 故障、最终只读 frame、跨 DLL/TLS 续点、默认文本的大容量/输出故障恢复、WM_CLOSE、创建中自销毁、跨 WNDPROC 展开及未完成窗口回收、错误槽故障与 atom 提交。 |
| `node crates/wire-wasm/tests/endpoints.mjs` | 28377 项检查：标准句柄/完整重定向/进程参数镜像、主机输入/EOF/批量输出、Waiting 与短预算、环形队列背压/恢复、管道别名/权限/半关闭、调用帧别名/guard/跨页故障、输入源码撤销、启动拒绝清理、标准别名和管道/容量预约边界、非消费 Peek/同步取消、输出别名/半关闭/完整高地址/错误槽故障及三个新增导出的静态导入和六参数 JIT 调用；端点调用不采样时间，重定向到普通文件后恢复文件时间样本。 |
| `node crates/wire-wasm/tests/module-queries.mjs` | 979 项检查：两种 ABI 的枚举/信息/A/W 名称、权限、装卸寿命、完整指针、截断与输出/返回槽别名及故障事务。 |
| `node crates/wire-wasm/tests/etw.mjs` | 1268 项检查：两种 ABI 的严格导入、完整 REGHANDLE/keyword、禁用事件检查范围、无效句柄短路、隔离/限额/注销、guard 顺序、输出事务和保存返回帧。 |
| `node crates/wire-wasm/tests/lazy-modules.mjs` | 4189 项检查：未装载转发链、ordinal/环路/失败与依赖 detach；惰性可选 DLL、RVA/VA、真实目标参数和完整架构上下文、零成本解析与单 work 预算、初始化重入/失败回收、原子输出故障重试、卸载恢复与显式引用寿命、provider 调用、畸形/重叠元数据及全局 4096 槽边界与重用。 |
| `node crates/wire-wasm/tests/startup-failure.mjs` | 1834 项检查：静态 DllMain 拒绝、已调用模块逆序通知、主映像与全部 TLS/生成代码回收、持续拒绝与 PC 编辑、零/单 work 预算、返回帧/TLS 槽/错误槽故障修复与恰好一次清理。 |
| `node crates/wire-wasm/tests/strings.mjs` | 860 场景：REP、宽度、DF、重叠、故障与续点身份。 |
| `node crates/wire-wasm/tests/stack.mjs` | 112 场景：FLAGS 栈、批量栈、ENTER、权限与部分进度。 |
| `node crates/wire-wasm/tests/vector.mjs` | 5819 项检查：整数 SIMD、隐式掩码、插入/提取、FLAGS、故障和驻留隔离。 |
| `node crates/wire-wasm/tests/string-compare.mjs` | 188532 项检查：四种 SSE4.2 字符串比较的独立集合/区间/逐元素参照、控制字段、隐式终止、32/64 位饱和长度、边缘部分匹配、掩码/索引、隐式和重叠目的、延迟 FLAGS/SETcc、完整及未对齐访存、故障/短预算、前缀、FP 隔离和跨模块驻留。 |
| `node crates/wire-wasm/tests/floating.mjs` | 59985 项检查：精确算术/舍入/转换/点积、近似误差界、MXCSR、异常顺序及数值调用的 CPU backing 隔离。 |
| `node crates/wire-wasm/tests/mmx.mjs` | 11944 项检查：独立 lane/浮点转换参照、物理 x87 别名、标签/TOP、EMMS、#MF/#XM、窄/跨页访存、故障与源码撤销、MMX/SSE 驻留混合及 VM 隔离。 |
| `node crates/wire-wasm/tests/masked.mjs` | 12417 项检查：掩码字节、隐式/段地址、只写权限、内联命中、故障事务、x87 转换、跨模块驻留与源码撤销。 |
| `node crates/wire-wasm/tests/fpu-state.mjs` | 10556 项检查：x87 等待/非等待控制、TOP/物理标签、元数据、三种 FX 布局、标签重建、CW 保留位/ES/B、保留区、精确故障、驻留快照、VM 镜像隔离及源码撤销。 |
| `node crates/wire-wasm/tests/fpu-environment.mjs` | 10730 项检查：四种旧式环境指令的两类布局、指针/opcode、TOP/物理标签重建、只读 payload、精确宽度/跨页/guard、失败写入抑制复位、等待/非等待与延迟异常、短预算、FLAGS/SIMD 隔离、段/地址/操作数前缀、源码撤销和跨模块驻留。 |
| `node crates/wire-wasm/tests/x87.mjs` | 155345 项检查：独立 BigInt FP80 四则/平方根/转换/舍入/余数/缩放参照、PC/RC/范围边界、商位与部分约减、双结果分解及缩放往返、NaN/不支持编码、寄存器及四类内存操作数、常量/比较/栈故障、原始编码分类、条件搬运及延迟 FLAGS、未屏蔽异常提交、精确跨页/guard/十字节访问、短预算及跨模块约减循环、驻留隔离和源码撤销；混合数值模块验证超过 127 项的导入编码。 |
| `node crates/wire-wasm/tests/x87-math.mjs` | 31809 项检查：八种超越指令的独立高精度参照、SDM 误差界、特殊值/定义域、PC/RC、极小/范围结果、双结果与二元弹栈、NaN/栈故障、未屏蔽异常与延迟 #MF、BCD 独立转换及十字节/跨页/guard/故障事务、延迟 FLAGS、跨模块驻留与数值入口隔离。 |

可选的 x86 指令差分入口为 `node tools/probes/check_string_compare.mjs`，需要支持 GNU 风格内联汇编的 C 编译器和 SSE4.2 x86-64 执行环境；可通过 `CC` 选择编译器。本次在可用的 x86 兼容执行环境中完成 18432 组字符串比较结果/FLAGS 对照。工具独立于常规测试目录，生成输入和编译产物在运行结束后自动清理，常规 Wasm 检查不调用该工具。

未屏蔽 x87 异常的检查依据为 Intel SDM 和独立整数参照。真实 x86 硬件的异常差分尚待核对；本机可用的 x86 兼容执行环境在项目生成的除零对照中忽略异常屏蔽位，已排除出该项参照。

可选 PE 编译检查为 `node tools/probes/check_wasm_pe_entry.mjs`，需要 MinGW 的 i686/x86_64 编译器及 dlltool，可通过 `WIRE_CC32`、`WIRE_CC64`、`WIRE_DLLTOOL32`、`WIRE_DLLTOOL64` 指定；版本资源组另需对应 windres，可用 `WIRE_RC32`、`WIRE_RC64` 指定。入口在独立临时目录编译本项目控制源，结束后清理产物，不进入常规测试发现范围，也不依赖外部游戏输入。各组保留已有通过记录；显示组此前已通过四个 ABI/优化组合；注册表组此前已通过；token/SID 组此前已通过；本阶段新增并运行 `--com-guid-only` 四个组合，覆盖 COM/WinRT 引用、线程、任务内存、GUID 及未注册工厂，未重复全量运行。

按改动选择对应入口；两种 ABI 通常各覆盖 `-O0`/`-O2`，AMD64 专用展开组覆盖相应的编译与汇编输入。

| 参数 | 验证范围 |
| --- | --- |
| `--painting-only`、`--memory-only`、`--unwind-only` | 绘制/DIB 像素、VMM/动态代码，以及 AMD64 目标展开/cleanup/函数表回调。 |
| `--profile-only`、`--environment-only`、`--queries-only`、`--calendar-only` | Windows 结构布局、虚拟系统/内存统计、PEB/环境/启动记录、身份/路径/版本、日历/时区。 |
| `--heaps-only`、`--locals-only`、`--inline-only`、`--address-only` | 堆/本地分配、TLS/FLS/临界区、SRW/条件变量/InitOnce、地址等待与故障续点。 |
| `--nls-only`、`--helpers-only`、`--format-only`、`--mutations-only` | 代码页/locale/日期时间图片/枚举回调/线程 locale、指针/整数/SLIST、消息格式与本地分配、文件复制/临时文件/时间。 |
| `--process-only`、`--options-only`、`--contexts-only` | 进程句柄/权限/亲和性、错误模式/启动属性/调试输出、进程内存/线程 CONTEXT/挂起重定向。 |
| `--virtual-unwind-only`、`--snapshots-only`、`--exit-only` | 映像定位/显式虚拟展开、Toolhelp/线程描述，以及自卸载/断点嵌套/FLS/分离/线程退出顺序。 |
| `--process-exit-only` | worker 发起正常/强制进程退出、FLS 线程身份/通知顺序、DllMain reserved、互斥体放弃、输出与终态代码回收。 |
| `--display-only` | 矩形/POINT/显示记录、嵌套 monitor 回调/DllMain/让出和回调内线程退出。 |
| `--version-resources-only` | windres 生成的 PE 资源表、A/W 文件版本、fixed/translation/CP932、截断和查询指针/长度。 |
| `--com-guid-only` | COM/WinRT 共享引用、线程隔离、任务内存跨线程释放、GUID 文本/v4 随机源及未注册工厂。 |

核心 Rust 单元测试现有 85 项，工作区合计 88 项通过，覆盖存储 backing/共享磁盘与目录事务、内存所有权/实时计数/峰值/源码失效、句柄和调用帧、同步对象、键盘/DIB/菜单、GDI 容量、类身份、展开、便携图形及 provider 准入。宿主/核心 wasm32 严格 Clippy、格式和差异空白检查通过。当前定向回归堆、COM/task、运行库、BSTR、VARIANT/SAFEARRAY、随机源和别名；其余模块保留已有通过记录，按变化范围选择复查。构建与静态检查命令：

```sh
cargo test --manifest-path crates/wire-wasm/Cargo.toml --locked --offline --target-dir target/wasm-rewrite --lib
cargo clippy --manifest-path crates/wire-wasm/Cargo.toml --locked --offline --target-dir target/wasm-rewrite --all-targets -- -D warnings
cargo clippy --manifest-path crates/wire-wasm/Cargo.toml --locked --offline --target wasm32-unknown-unknown --target-dir target/wasm-rewrite --lib -- -D warnings
cargo check --locked --offline --lib --target-dir target/wasm-rewrite
```

旧入口的图像、资源和快照检查作为迁移历史保留，根工作区不再提供原生解释器测试入口。复用组件的后续回归通过新核心的相应稳定输入验证。

核心 Wasm 的本地 Rust 编译依赖闭包未包含旧 CPU 执行器、执行 IR、旧 lowering 或原生后端；旧 x86/x64 命名空间只复用独立的 x87 数值核心、网关地址/Windows 页保护常量及九个共享 DXGI/D3D11 协议文件，未引入旧图形 native 协调器。缓存预算计费覆盖保留的生成输入与保守元数据额度，浏览器引擎内部代码内存不在此计费中。

显式音频宿主验证为 `python3 tools/probes/check_wasm_audio.py --out target/wasm-audio-check`，需要 macOS SDK/clang/WKWebView/OfflineAudioContext；项目生成输入、宿主和结果位于忽略的 target 下，不依赖外部资料，也不进入默认测试发现范围。

## 文档维护

过时阶段报告与旧设计已移出常规文档目录，README/索引指向新入口。当前目标、能力、证据及缺口只维护在本文件；逐接口状态由台账生成器维护，实现契约链接现行架构/API 文档。已被后续实现取代的阶段状态和逐轮工作记录不继续累计。

## 完成条件

- 产品只有 Wasm JIT 客体执行路径，普通执行留在同一引擎环境，无原生解释回退或旧 IR 执行层。
- 既有可复用兼容能力完成迁移，D3D/WebGPU 实际提交和资源释放通过验证；未迁移能力如实列出。
- x86/AMD64 普通操作直接生成 Wasm，复杂操作使用专用同 Wasm 语义函数；未知操作精确报告，不能静默跳过。
- 完整 CPU、FLAGS、访存权限、异常、短预算、REP、自修改、线程及回调状态满足契约。
- 真实游戏长区间有可复核的整体性能改善；新旧比较和解释参照保留明确来源，不只报告启动前缀或自有循环。
- 默认构建、说明和文档与最终源码一致，过期计划和中间报告不重新进入常规文档目录。

整体重写与性能目标保持未完成。
