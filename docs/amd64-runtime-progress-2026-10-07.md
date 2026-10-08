# AMD64 / Unity / Mono 启动推进（2026-10-08 接续）

这是接续 `654a605` 的Unity logo之后初始化检查点。普通 `wire run` 仍拒绝 AMD64；
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

AliceToPapa隔离副本的 **pass152 已实际执行398次guest GPU Draw、144次DXGI Present**。
窗口由guest自己改为 `visible=true`、客户区1024×720，HWND=`0x7ffd00630000`，
标题 `AliceToPapaADV_1-0-0`。不再停在隐藏bootstrap窗口，也没有强制显示guest隐藏窗口。
最新退休指令3,348,585,448、work3,355,396,173、已完成API6,332,527，
日志为 `wire-out/games64/alice/linked-pass-152.txt` 与 `api-pass-152.tsv`。
这证明Win64窗口、实际guest Draw和公开Vulkan WSI提交路径已经越过；
用户已实际观察到 **Unity logo动画**。pass137–152保存的是最后一次成功Present，
该时刻为深色背景；单帧捕获不能否定之前的动画，也不足以验收游戏标题画面、输入或持续可玩。

pass126–132逐步补齐DXGI虚拟output/modes、GetWindowPlacement/DWM、NOTOPMOST定位、
虚拟ClipCursor、windowed swapchain、UNORM/sRGB同族后缓冲view、typeless深度纹理/DSV。
pass133已实际到达渲染线程第一次 `DrawIndexed(6,0,0)`；pass134记录完整绑定状态。
独立实现的Win64 InputLayout解析32字节input元素与ISGN/ISG1，
按semantic/index消解APPEND偏移并保存真实属性；未使用的额外元素不生成attribute。
随后guest VS/PS、VB/IB、CBV/SRV/sampler、viewport/scissor、blend/depth/stencil/raster状态
接到owned Vulkan资源的真实Draw，保留render target内容并等待实际GPU完成。
没有把固定64×64离屏诊断pipeline冒充guest绘制，也未将unknown Draw返回空成功。

pass135曾停在 `oleaut32.dll!#2`（SysAllocString）；BSTR分配、长度、字节长度、释放、
重分配及对应ordinal/动态导出已经成组接入，并通过高地址、embedded NUL、未配对UTF16、
奇数字节长度、alias重分配、OOM长度及checked fault回归；实际pass137起已执行分配与释放。
随后补齐消息extra/time/position元数据、WinHTTP隔离proxy profile与无传输时的真实失败、
Winsock owned手动复位event与合作等待；pass139已经越过这些边界，停在 `inet_pton`。
地址解析/格式化及16/32位字节序转换已成组接入；pass140实际执行IPv4/IPv6解析后，
曾停在socket ordinal23。隔离profile无网络transport，socket/WSASocketA/W已接入真实能力失败，
返回完整64位INVALID_SOCKET；pass141证明guest自行处理失败并继续。
修正CriticalSectionEx低位flags后，pass142又前进约2.9亿退休指令，
当时停在 `user32.dll!GetKeyboardLayout`；后续键盘、packed sqrt、鼠标跟踪与身份查询已越过。
pass146/147曾因缺少Device1而发生真实空对象读取；补齐实际Device1/Context1 vtable后，
pass148继续约4.3亿指令，到R32 typeless深度纹理创建；pass150–152继续越过其view/clear及R16纹理/RTV。
当前明确失败是CreateShaderResourceView的format/range/usage校验，尚不能当作新shader已开始渲染。
线程6的网络尝试没有继续阻止主线程推进。
原始游戏目录保持只读，运行与Player.log只写在 `wire-out/games64/alice/runtime/`。

CPU解释器/IR仍为主路径，没有guest CPU JIT。此前Mono的TLS/DllMain、合作线程、
真实异常handler/RtlUnwindEx、程序集合加载/domain重置、PhysX后端选择与输入初始化已经通过。
HID/SetupAPI隔离设备枚举为空，WinRT Gamepad无注册类时真实失败，未伪造设备。
实验性D3D11仍不是完整FL10_0符合性；GS→compute仅完成独立验证，guest GS/SO尚未接入。

此前AMD64性能AB的终点是CoCreateGuid：before为20.69亿退休指令/536.798秒，
最终IR为13.61亿/54.766秒，整体MIPS3.85→24.86；程序集加载183.887→18.898秒。
真实计时器导致轮询指令/API量不同，不能把时间差当作同指令序列的纯IR收益。
此后新增路径、API统计、GPU同步和帧捕获均改变测量范围，本轮不报告性能AB收益。
详细历史对照见 [AMD64性能记录](performance-amd64-ir-2026-10-08.md)。

`probe64 --capture-frame PATH.ppm`可保存最近一次成功Present的实际guest后缓冲；
这是显式诊断，会启用CPU读回并改变时序，正常Present没有CPU读回。
捕获失败只报告诊断错误，不将已成功提交的GPU帧伪装为失败。
批量API扫描与全程TSV仍可用于查明后续库函数和COM调用，见
[API清单记录](api-inventory-2026-10-08.md)。

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
| pass-114 | 761,820,010 | 1,363,934 | 反复28字节HeapAlloc失败，随后Mono错误路径栈溢出 |
| pass-115 | 756,848,329 | 1,364,898 | 明确诊断：AMD64 mapping capacity exceeded，512MiB提交额度用尽 |
| pass-116 | 1,377,667,739 | 2,662,022 | Mono加载/domain重置完成；guest x87 FLD未实现 |
| pass-117 | 2,081,189,211 | 3,706,308 | x87通过；`CoCreateGuid` |
| pass-118 | 1,799,025,225 | 3,844,123 | GUID通过；`IsMouseInPointerEnabled` |
| pass-119 | 1,800,877,589 | 3,846,514 | mouse pointer查询通过；`EnableMouseInPointer` |
| pass-120 | 1,798,274,204 | 3,843,439 | pointer真实失败回退；`WindowsCreateStringReference` |
| pass-121 | 1,798,936,012 | 3,843,963 | HSTRING/WinRT回退通过；`RegisterRawInputDevices` |
| pass-122 | 1,798,402,000 | 3,843,661 | raw input真实失败回退；`HidD_GetHidGuid` |
| pass-123 | 1,808,121,280 | 3,854,319 | 消息队列/全程API汇总接入；同一HID边界 |
| pass-124 | 1,800,454,041 | 3,846,116 | HID/SetupAPI通过；`RegisterDeviceNotificationW` |
| pass-125 | 1,803,222,221 | 3,851,692 | 设备通知/输入初始化通过；DXGI `Adapter.EnumOutputs` |
| pass-126 | 1,802,941,080 | 3,851,383 | DXGI output通过；`GetWindowPlacement` |
| pass-127 | 1,801,776,303 | 3,850,039 | window placement通过；`DwmGetWindowAttribute` |
| pass-128 | 1,803,768,779 | 3,852,365 | DWM通过；NOTOPMOST错误引出`DialogBoxParamW` |
| pass-129 | 1,803,057,348 | 3,851,375 | NOTOPMOST定位修复；`ClipCursor` |
| pass-130 | 1,805,201,332 | 3,853,964 | 虚拟ClipCursor/交换链接入；sRGB RTV view |
| pass-131 | 1,802,705,854 | 3,851,136 | sRGB同族view通过；typeless深度纹理 |
| pass-132 | 1,952,994,572 | 4,143,003 | depth/stencil通过；`CreateInputLayout` |
| pass-133 | 1,949,486,129 | 4,139,093 | InputLayout通过；第一次`DrawIndexed(6,0,0)` |
| pass-134 | 1,948,175,213 | 4,137,707 | 第一次Draw真实绑定状态诊断 |
| pass-135 | 1,986,709,001 | 4,209,632 | 269 Draw / 90 Present；BSTR ordinal2 |
| pass-136 | 1,957,428,392 | 4,148,895 | 宿主输入分支；`GetMessageExtraInfo` |
| pass-137 | 1,991,623,926 | 4,221,045 | 326 Draw / 109 Present；WinHTTP proxy查询 |
| pass-138 | 1,990,475,570 | 4,217,608 | 338 Draw / 113 Present；`WSACreateEvent` |
| pass-139 | 1,993,710,950 | 4,220,625 | 332 Draw / 111 Present；`inet_pton` |
| pass-140 | 1,990,805,404 | 4,217,882 | 地址转换通过；338 Draw / 113 Present；socket ordinal23 |
| pass-141 | 1,993,384,119 | 4,220,631 | socket真实失败回退；332 Draw / 111 Present；CriticalSectionEx flags=1 |
| pass-142 | 2,282,797,097 | 4,472,765 | critical section修复通过；335 Draw / 112 Present；`GetKeyboardLayout` |
| pass-143 | 2,579,099,762 | 4,982,429 | 键盘布局查询通过；338 Draw / 113 Present；`SQRTPS` |
| pass-144 | 2,857,875,295 | 5,498,459 | SQRTPS通过；401 Draw / 144 Present；`TrackMouseEvent` |
| pass-145 | 2,864,441,305 | 5,506,021 | 鼠标跟踪真实失败后继续；399 Draw / 144 Present；`GetComputerNameW` |
| pass-146 | 2,855,877,159 | 5,496,864 | 身份查询通过；391 Draw / 141 Present；缺少Device1导致Unity空对象读取 |
| pass-147 | 2,854,687,975 | 5,488,685 | 显式Windows7-like开关未解决；366 Draw / 133 Present；同一空Device1读取 |
| pass-148 | 3,281,270,686 | 6,203,425 | Device1通过；401 Draw / 145 Present；`CreateTexture2D(format=39/0x27)` |
| pass-149 | 3,270,966,962 | 6,192,414 | 398 Draw / 144 Present；同一R32 typeless边界 |
| pass-150 | 3,292,130,479 | 6,220,488 | R32资源/view通过；399 Draw / 144 Present；depth-only视图clear flags=3 |
| pass-151 | 3,346,638,205 | 6,333,631 | depth clear通过；309 Draw / 114 Present；R16 typeless `format=53/0x35` |
| pass-152 | 3,348,585,448 | 6,332,527 | R16纹理及4次RTV创建通过；398 Draw / 144 Present；SRV格式/范围校验 |

指令与 API 计数受真实时间/线程调度影响，上表不构成相同路径的性能 AB 对照。

## 键盘查询与 packed SSE 平方根（2026-10-08）

按静态清单成组接入GetKeyboardLayout / List / NameA/W、GetKeyboardType、
MapVirtualKeyA/W / ExA/W、ToUnicode / Ex与GetKeyNameTextA/W，共13个Win64入口。
隔离桌面明确使用一个虚拟US非IME布局：HKL=`04090409`、KLID=`00000409`，
与日语LCID=`0411`及ACP/OEMCP=932独立；不宣称已接入日本JIS扫描码或宿主布局切换。
实现普通set-1扫描码、左右modifier、keypad/navigation、Shift/Caps/Ctrl字符与英文键名。
GetKeyboardLayout可查询存活guest线程；参数DWORD截断，HKL与buffer保持64位。
查询/转换支持checked输出、短buffer终止与跨页故障原子性；这些入口只属于USER32，
不会出现在kernel32/kernelbase/imm32的静态分派或动态导出中。

ToUnicode仅使用caller提供的256字节状态，无dead-key或Alt+numpad组合backend；
不消费host text，也不更新guest排队按键状态。现有TranslateMessage继续消费宿主已解析的Unicode，
因此输入查询不会吞掉缓存的日语文字。AppKit目前仍没有完整的Windows物理扫描码、
左右modifier事件或多布局同步，这组查询的完成不代表整个输入系统已完成。
布局字符/键名规则适配固定Hangover Wine的LGPL源码，保留原始input.c、notice和hash，
范围见`third_party/hangover/README.md`；没有引入QEMU GPL实现。

pass143真实完成GetKeyboardLayout一次及MapVirtualKeyExA 119次，
越过输入初始化并继续到SQRTPS。已将SQRTPS/SQRTPD接入既有精确整数平方根路径，
逐lane处理MXCSR舍入、DAZ、NaN与异常；聚合全部lane异常后才提交目标XMM。
packed m128保持16字节对齐/readable校验；scalar形式保持高lane。
扩展Unicorn差分覆盖single/double、scalar/packed、四种舍入、零、NaN、非正规数及随机位模式；
独立回归检查多lane invalid/denormal聚合、unmasked #XM、内存fault和目标不提前提交。

pass144继续到TrackMouseEvent。已登记其Win64 ABI并校验24字节TRACKMOUSEEVENT、
完整HWND和flags；QUERY返回实际空注册状态，CANCEL空注册可成功。
AppKit目前没有hover/leave tracking事件源，HOVER/LEAVE/NONCLIENT注册仍真实返回
FALSE/ERROR_NOT_SUPPORTED，而非报告将投递不存在的事件。
回归覆盖空查询、取消、非法size/HWND、未投递消息、专属DLL归属及跨页QUERY输出fault。
本次完整release回归323项通过，6项ignored；Clippy release all-targets `-D warnings`通过。
日志为`/private/tmp/wire-keyboard-final-{all-tests,clippy}.txt`。

pass145确认Unity处理TrackMouseEvent的真实失败并继续，随后到GetComputerNameW。
已成组接入GetComputerNameA/W、GetComputerNameExA/W、GetUserNameA/W。
它们返回隔离guest身份：NetBIOS `WIRE`、DNS hostname/FQDN `wire`、无DNS domain，
账户名`wire`与已有`C:\\Users\\wire`目录一致；不查询或暴露宿主账户/网络身份。
长度参数是DWORD指针，计算机名成功长度不含NUL，用户名成功长度包含NUL；
短buffer更新required含NUL，分别返回ERROR_BUFFER_OVERFLOW / MORE_DATA / INSUFFICIENT_BUFFER。
空domain仍写NUL；invalid Ex类型失败不改输出。成功前检查所有输出，支持跨页fault原子性。
计算机名入口只属于kernel32/kernelbase，用户名只属于advapi32，静态/动态导出归属均有回归。

pass146完成GetComputerNameW后停在UnityPlayer `0x18093f906` 对RCX=0的真实读取，
不是新缺失Win32导入。只读反汇编确认该地址执行`mov rax,[rcx]`，随后调用vtable slot33
CheckFeatureSupport(feature=5,size=56)。指针从global `0x182128f18` 读取；
初始化在 `0x18091b602` QueryInterface请求IID
`a04bfb29-08ef-43d6-a49c-a9bdbdcbe686`（ID3D11Device1），未检查失败返回即保存为空。
pass146时bridge只支持ID3D11Device及基础vtable，Device1真实返回E_NOINTERFACE。
没有映射空地址；随后独立接入Device1/Context1公开ABI及下述实际语义。

Unity二进制含有`force-windows-7-like-d3d11`字符串，但实际诊断不能据此假定基础路径有效。
诊断CLI新增可重复`--guest-arg VALUE`：以Windows规则分别quote每个literal参数，
更新GetCommandLineA/W与PEB CommandLine的checked缓冲和UTF16长度，保留原EXE路径。
参数不会经过shell，也不修改Windows版本查询；默认不添加兼容开关。
回归覆盖空参数、引号、空格、尾随反斜杠、日语/CP932、NUL/超长拒绝和跨输出fault原子性。
pass147显式使用`--guest-arg -force-windows-7-like-d3d11`，仍在同一地址发生空Device1读取；
该开关未解决问题，未设为默认。

## Device1 / Context1与typeless纹理（2026-10-08接续）

按公开ABI独立追加Device1的7槽和Context1的19槽，保留所有基础槽号、IUnknown身份、
AddRef/Release与保护页。DxgiDevice→Device1查询返回控制Device，保持接口查询传递性。
GetImmediateContext1返回真实已有Context；copy/update的flags=0转入实际基础操作。
SetConstantBuffers1只支持完整buffer（first=0、count=4096或NULL默认）；partial/offset明确拒绝。
DiscardResource/View/View1校验owner、类型及rect内存后保留内容作为合法discard hint，
不更改绑定与引用。deferred/context-state、Blend/Raster1和共享资源创建返回真实UNSUPPORTED，
预检并清零输出；GetConstantBuffers1、ClearView、SwapDeviceContextState与非零copy flags等仍未实现。
CheckFeatureSupport(feature=5,size=56)沿用实际支持范围，全零不广告高级能力。
公开声明核对来源为Wine 10.0固定commit `b073859675060c9211fcbccfd90e4e87520dc2c2` 的
`include/d3d11_1.idl`；没有移植Wine/DXVK renderer实现，不宣称完整D3D11.1。

pass148以默认参数越过上述空对象读取，继续到线程23的1×1、format=39（0x27）、bind=0x48深度纹理创建。
原descriptor诊断以十六进制输出，不能将其中27读成十进制RGBA8格式；新诊断同时打印十进制format。
pass149仅包含RGBA8/BGRA8修复，仍在同一R32边界，未因此继续。
宿主后端现独立实现RGBA8/BGRA8/R32/R16 typeless存储：27→UNORM RGBA、90→UNORM BGRA，
使用mutable image；typed views必须明确选择28–32或87/91，资源GetDesc仍保留原typeless格式。
不会把typeless格式作为shader/vertex/view类型，默认/typeless或跨族view失败。
typed view按真实GPU格式能力验证；sRGB清除转换线性颜色，copy始终保留原始字节。
39带DEPTH_STENCIL时保留真正的D32 storage，DSV40与FLOAT SRV41共享深度aspect；
无depth bind时使用R32 storage并允许FLOAT/UINT/SINT view41–43，不将D32纹理替换成普通颜色附件。
depth上的UINT/SINT与默认typeless view仍拒绝；允许深度按4B像素读回和同格式copy。
独立GPU探针验证两族、7种typed SRV、多层mip、UNORM/sRGB clear及typed↔typeless复制共928字节，
再验证R32 color/depth、视图限制、depth clear和copy，共1024字节。
pass151继续到128×1、format=53（0x35）的R16 typeless颜色纹理；已使用R16 storage与正确2B像素布局，
颜色支持FLOAT54/UNORM56/UINT57/SNORM58/SINT59；depth bind使用D16 storage、DSV55及UNORM SRV56。
五种颜色view、UNORM clear与D16 clear/读回另检查576字节，最终四族共1600字节。
原有深度、cube、volume与传输回归继续通过。证据为
`wire-out/typeless-resource-probe-2026-10-08/report.json`。
guest COM回归另覆盖原descriptor、view引用、invalid HRESULT及输出fault不创建对象/增加引用。
资源探针退出仍有MoltenVK报告1MB未释放，不能据此声称已完成泄漏排查。

pass150越过R32创建/view，暴露ClearDepthStencilView对flags=3的过严检查：
depth-only视图没有stencil时，应取flags与存在aspect的交集，只清真实depth；
stencil-only/flags=0在该视图上是合法无操作，不表示实现了stencil存储。
核对DXVK Native 1.9.2的公开兼容行为后独立修正，未复制其context实现；
微软公开文档还明确Depth需要clamp到0–1，现对有限值和±Inf正确clamp，使用中的NaN仍明确拒绝。
回归覆盖flags=3实际depth clear、stencil-only未使用NaN、无flags、4种clamp与错误保留像素。
guest COM实际验证FLOAT来自XMM3并在D32资源读回32个正确深度像素。

`16dd486`保存了D3D11资源与Mono线程服务检查点，完整release回归281项通过，Clippy无warning。
之后统一Win64 API参数表与kernel32/kernelbase动态导出发现：已实现函数不再依赖此前是否登记静态gateway。
其它DLL的专属函数不会因此出现在kernel32；未知函数仍返回NULL/ERROR_PROC_NOT_FOUND。
GetConsoleCP沿用CP932 profile；GetCPInfoEx A/W输出284/544字节，代码页/默认字符/lead-byte表与A/W名称完整写入，
无效flags及checked输出fault保留原输出。新增两项回归后完整release283项通过，Clippy无warning。
pass116已经完成并明确下一个x87边界；不将Mono加载完成当作正式窗口或首帧验收。

小堆分配按16–2048字节桶使用64KiB owned slab，保留16字节对齐、HEAP_ZERO_MEMORY与HeapSize。
释放块可重用，同桶多余空slab回收；大块保留独立页。HeapReAlloc保留旧数据，失败保留旧块。
guest process heap与native callback/COM页分别管理，不允许HeapFree释放服务对象。
独立4096×28字节分配测试仅提交128KiB（旧实现需16MiB），数据逐块不重叠且释放/重用/重分配通过。
当前游戏325M work时提交206MiB，而旧路径350M–450M work观察为415–420MiB；
时间与调度路径不同，不能据此宣称MIPS或启动时延的AB收益。
完整release284项通过，Clippy无warning；诊断命令每25M work打印PC/API数/提交MiB与最近三个API。

pass116的x87边界已补：AMD64 FLD/FST/P、FILD/FIST/P/TP、基本浮点/整数算术、
比较/FCOMI、FCMOV、交换/释放、常量、舍入/sqrt/scale及既有软件超越函数复用同一FP80核心。
32/64/80位实数与有符号16/32/64位整数转换、control-word舍入和栈tag保持；
FIP/FDP扩为完整64位，Win64异常上下文捕获/恢复不再截断，PE32环境仍明确序列化32位布局。
checked跨页失败不改变CPU或留下部分store；新产生的未屏蔽算术异常仍明确拒绝，
尚未实现x87延迟异常投递。x87环境指令/FPREM/FPATAN等尚未补齐，不宣称完整x87。
2,304组Unicorn寄存器方向/pop/舍入算术差分通过；独立FP80半ULP舍入、64位整数、
跨页load/store、未屏蔽异常拒绝与高指针上下文恢复通过。
完整release286项通过、5项ignored，Clippy无warning；pass117已完成并停于CoCreateGuid，见上方真实结果。

Win64窗口定位已准备并通过独立回归：SetWindowPos/MoveWindow使用40字节WINDOWPOS及
56字节NCCALCSIZE_PARAMS，HWND/lppos/LPARAM保持64位；WM_WINDOWPOSCHANGING可修改位置/尺寸，
必要时实际调用guest WM_NCCALCSIZE后提交客户区，WM_WINDOWPOSCHANGED调用默认WNDPROC时
按真实client变动派发WM_MOVE/WM_SIZE，不由定位API强制替guest处理这些消息。
SWP_SHOWWINDOW/HIDEWINDOW改变真实guest visible标志；支持NOZORDER和HWND_NOTOPMOST的同步定位，
异步/z-order等未接通的请求返回ERROR_NOT_SUPPORTED，未声称宿主已实现这些效果。
MoveWindow当前没有Win64 GDI invalid region，bRepaint尚无后端区域刷新；图形仍由后续Present推进。
默认消息规则沿用固定Hangover LGPL参考，出处/著作权保留在源码。
独立guest测试每个work暂停/恢复，验证callback修改x/cx、嵌套default通知、64位临时buffer释放、
visible/RECT/客户区及未支持flags不改变窗口。

## 首帧之后的剩余工作

正式可见窗口与guest Draw/Present已经通过。当前继续检查真实帧内容及游戏托管初始化，
补齐实际出现的运行时/图形边界，验收标题画面和输入响应。
windowed discard/sequential交换链走public VK_EXT_metal_surface/VK_KHR_swapchain；
flip、exclusive、waitable frame latency、多RT、多viewport、guest GS/HS/DS/SO、
push-data、descriptor数组、缺失shader资源、非零depth bias等当前仍明确拒绝。
不能把GPU提交次数或Unity logo动画等同于完整可玩或全部D3D11支持。
实际游戏请求feature level11_1/11_0/10_1/10_0，设备仍明确使用实验性10_0子集；
MoltenVK缺少原生GS，独立GS→compute验证不是guest GS支持证明。

## 已实现的运行时范围

- HID class GUID使用Windows字段布局；隔离PnP设备数据库为空，SetupAPI HDEVINFO有owned高地址生命周期，
  32字节Win64 device/interface记录、ERROR_NO_MORE_ITEMS、无效set/record及checked故障保持真实语义。
  HidD未知设备句柄失败并保留输出；无owned preparsed data的HidP返回INVALID_PREPARSED_DATA，
  MaxDataListLength为0且不修改LastError。没有物理HID/gamepad或report解析器，DEFAULT/PROFILE仍不支持。
  设备通知A/W登记owned window/interface订阅、注销及窗口销毁清理，不虚构设备arrival；service/handle通知拒绝。
- GUID创建使用宿主OS熵与Windows字段布局；字符串解析/格式化、task heap所有权及动态导出成组接入。
  profile没有HKCR ProgID/activation-context注册，未解析名字真实返回CO_E_CLASSSTRING。
- HSTRING reference使用guest 24字节header和完整64位指针；owned copy只读、引用计数与释放，
  length/raw buffer/empty/embedded NUL/ordinal比较已接入，空字符串有稳定只读NUL buffer。
  无注册WinRT类时activation返回REGDB_E_CLASSNOTREG，不发放虚构Gamepad factory。
- Win64 48字节MSG、Peek/GetMessage A/W、PostMessage/PostThreadMessage/PostQuitMessage、
  TranslateMessage与keyboard状态按线程维护。GetMessage空队列接入真实合作等待，
  DispatchMessage执行已有guest WNDPROC并保留返回值；checked输出fault不消费消息。
  AppKit Unicode在TranslateMessage时生成字符，键盘异步状态和取出消息后的状态分离。
  WM_TIMER procedure、WaitMessage/MsgWait、QS高位flags、完整鼠标捕获仍未实现。
- Get/SetMessageExtraInfo、GetMessageTime、GetMessagePos按线程保存完整pointer-width extra、
  DWORD时间与有符号坐标pack；成功取出消息才更新元数据，checked输出fault不消费/不更新。
  当前宿主及posted输入没有extra tag，取出时记录0，不从guest消息指针推测tag。
- BSTR使用owned guest allocation、4B字节长度前缀与双零terminator；支持embedded NUL、
  未配对UTF16、ByteLen奇数长度、alias重分配，OOM保留旧串。ordinal2–7/149/150与动态导出一致。
- WinHTTP隔离profile没有IE配置，查询真实返回FILE_NOT_FOUND并保留输出；system proxy明确为direct。
  没有HTTP transport，WinHttpOpen返回NOT_SUPPORTED，不发放假HINTERNET；无效session返回INVALID_HANDLE。
- Winsock事件组共用owned kernel event与合作调度器；手动复位信号可跨Win32/WSA入口等待和修改，
  DWORD参数截断但handle数组与指针保持64位，alertable/APC与deadline规则沿用真实线程等待。
  地址转换使用Windows AF_INET=2/AF_INET6=23与网络字节序，A/W、短buffer、错误码及checked store独立实现。
  `htons/htonl/ntohs/ntohl`及ordinal8/9/14/15保持guest小端语义，不依赖宿主AF常量或字节序。
  格式错误时地址输出保留，不复现Windows部分解析对失败输出的未定义修改；尚无socket/网络传输后端。
  socket/WSASocketA/W在没有网络provider的profile返回WSAEAFNOSUPPORT与64位INVALID_SOCKET，
  未初始化返回WSANOTINITIALISED；custom protocol/duplicate info返回WSAEOPNOTSUPP，不发放socket句柄。
- pass141已经由guest处理socket失败并继续，随后触发InitializeCriticalSectionEx的flags=1。
  静态核对UnityPlayer唯一对应IAT call（`0x181ab9f5a`）：R15D=12、LEA R8D,[R15-11]，实际参数为1。
  根据固定Wine 10.0的同步profile，修正低24位无效果标志被错误拒绝的问题；
  NO_DEBUG_INFO高位继续接受，动态spin/static-pool/force-debug等尚未实现的高位功能仍明确拒绝。
  锁仍使用真实合作线程owner/递归/阻塞规则，不把EnterCriticalSection改为空成功。
- raw input设备/注册查询真实为空，Win64设备项16字节/header24字节；无后端的有效注册返回
  ERROR_NOT_SUPPORTED，空注册的合法REMOVE可成功；未知raw句柄返回INVALID_HANDLE。
  IsMouseInPointerEnabled为false，启用promotion返回不支持，不假报WM_POINTER能力。
- 实验性D3D11 device/context、owned Vulkan资源、shader编译、状态描述符与真实query；
  生命周期及GPU数据/clear像素已验证，当前guest VS/PS Draw/Present已接通，仍不是完整FL10_0。
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
  private byte data有界、短buffer语义与window association已实现；虚拟output与windowed交换链已接入。
  DXGI状态通知和private IUnknown回调引用仍未实现。没有后端时不会发放假adapter。
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
  子窗口/menu、跨线程发送、reentrant self-destroy、完整默认窗口行为、DPI非96和完整消息等待协议仍有边界。
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
  事件已转成Win64 MSG；所有线程等待且有presenter时定期泵主线程UI，Present使用真实GPU WSI提交。
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
CONTEXT 仅支持 CONTROL/INTEGER/FLOAT；x87 ip/dp 已扩为完整64位并保持Win64捕获/恢复，不能截断。

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

x87 已在这些控制指令上补数据/算术/栈操作与部分软件超越函数，范围及差分见前文pass116接续。
状态保持FP80，FIP/FDP为64位；环境指令、更多超越函数与未屏蔽异常的延迟投递仍未完成。

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
  --max-instr 3500000000 --startup --window \
  --api-trace /Users/alphaly/Projects/wire/wire-out/games64/alice/api-next.tsv \
  --capture-frame /Users/alphaly/Projects/wire/wire-out/games64/alice/guest-present-next.ppm \
  --graphics-backend /Users/alphaly/Projects/wire/wire-out/libwire_graphics.dylib \
  --moltenvk /private/tmp/wire-moltenvk-1.4.2/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib
```

无capture/API统计的实际启动profile和CPU性能优化已经完成；
后续保留SRV真实失败的descriptor诊断，继续检查Unity logo之后的游戏标题画面。
不以LoadLibrary成功、GPU帧数或空背景当作游戏跑通。
这轮后续CPU优化已完成：两组真实窗口AB/BA到同一R16 SRV错误，wall中位数
148.256→97.417秒，退休指令吞吐22.34→33.16 MIPS；完整release328项通过。
owned页槽位缓存仍逐次检查数据权限/guard，FLAGS完整计算，图形后端与功能终点冻结。
采样、确定性矩阵及完整重放见[Unity logo阶段性能记录](performance-unity-logo-2026-10-08.md)。
该SRV实际为ArraySize=UINT_MAX的剩余slice范围，后续应补齐范围归一化再继续标题画面验证。
此前功能检查点完整release回归323项通过：29 unit、37 differential、9 FPU、26 IR、1 Wasm、109 Win32、112 AMD64，6项ignored。
后续CPU检查点为328项通过，AMD64增至117项，其余分组与6项ignored保持一致。
本轮新增WSA事件与IP转换回归，覆盖manual-reset生命周期、Win32/WSA共用等待、64位指针/句柄、
Windows地址族与网络字节、A/W错误码、动态named/ordinal导出、短buffer及checked故障不提前提交；
无network provider的创建失败另覆盖64位INVALID_SOCKET、startup优先级及event仍可正常使用。
CriticalSectionEx回归覆盖guest实际的flags=1、低位组合、DWORD截断、递归owner/释放、
不支持的高位功能及跨页输出fault，不提前改变CPU、输出或完成调用统计。
日志为 `/private/tmp/wire-logo-final-{all-tests,clippy}.txt`；Clippy all-targets `-D warnings`通过。
本轮原生COM回归另见`/private/tmp/wire-typeless-native-com.txt`，资源与Draw探针见图形设计记录。
Python扫描器5项原创fixture回归通过。API汇总、设备发现/通知的故障原子性、生命周期与DLL发现均有独立覆盖。
最终Clippy all-targets、format、diff whitespace检查通过。
此前图形检查点新增4项覆盖COM身份/引用/parent、Win64 Desc布局/高指针、readonly保护、故障原子性、
private data短buffer、D3D12 named/ordinal/动态查找与真实失败。GS新增两个unit测试检查strip/绕序与坏记录。
原生GPU ignored测试另行通过2项，包含Rust shader/buffer/context/library生命周期、
跨context拒绝、CBV更新/绑定及实际Draw/readback；两项既有Metal测试
本轮保持默认ignored，未修改PE32 Metal实现。
日志：`/private/tmp/wire-native-graphics-all-tests.txt`、`wire-native-graphics-rust-tests.txt`、
`wire-native-graphics-clippy.txt`。Clippy release all-targets `-D warnings`、format、diff whitespace检查通过。
GS检查点日志为`/private/tmp/wire-geometry-{all-tests,native-rust-tests,clippy}.txt`；
GS证据在`wire-out/geometry-probe-2026-10-08/`。当前标准图形后端的3项native GPU ignored回归
另行通过，swapchain/InputLayout/depth FLOAT参数回归亦单独通过；见图形设计文档的探针记录。
默认cargo build / Wire.app没有捆绑第三方GPU二进制；backend构建与验证详见图形设计文档。
旧文档的13条入口/无 Win64 runtime 是历史状态。

本轮版本比较移植自固定 Wine 10.0 的 LGPL-2.1-or-later `ntdll/version.c`，选择 LGPL 2.1，
保留原始快照、版权、许可证及 SHA-256，范围见 `third_party/wine-10.0/README.md`。
Winsock 仅只读核对公开 ABI/行为，没有复制其 C 实现。没有引入 QEMU GPL 代码。
SSPI 本轮只读核对固定 Wine 10.0 的 sspi.h/secur32.spec 及 curl 8.0.1 的初始化流程，
未复制上游实现；显示器与注册表服务为本地独立实现。
本轮资源/Win64 USER32与目录查询是本地实现；共用geometry仍沿用既有Hangover Wine LGPL来源，
出处见其文件头和 `third_party/hangover/README.md`。未引入新的Wine/QEMU renderer或原生Mono。
