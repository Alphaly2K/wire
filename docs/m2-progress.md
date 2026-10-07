最新检查点：[三个 PE32 样本兼容进度](games32-progress-2026-10-07.md)。
当前按用户最新要求继续 Rewrite、totsulover，并恢复 AliceToPapa x86-64 基础支持。
恋×シンアイ彼女已有真实标题菜单；Metal 固定管线和 bulk Clear 后完成 193 秒 / 6,000 Present 重放，
用户听验确认 BGM 当前没有问题，后续仍需正文、场景切换和持续可玩验收。
Rewrite 已实现有界 PS2/VS2、字体度量、资源 DLL 与 effect 状态块，
22B指令 / 3,101 Metal Draw / 522 Present 后已检查真实完整标题菜单，正文与性能仍待推进；
totsulover 缺少其要求的外部“ソフト電池”运行时。详情见 [Rewrite 检查点](rewrite-shader-loader-2026-10-07.md)。
解释器继续作为主路径，保留 checked memory、保守 FLAGS 与 FP80 语义。
以下性能数据是此前阶段的历史记录，不是本轮跨游戏对照。
已扩大到约23亿 guest 指令的真实启动 profile，优化 callback IR 连续调度、
紧凑栈调用、完整日志的分配和 macOS UI 轮询/前台 QoS；该轮128项回归通过。
三对完整启动AB/BA首 Present中位数25.031→18.996秒（约−24%），
测量段37.191→29.534秒，六次全部GPR/EIP/EFLAGS及绘制/呈现计数相同。
实测、边界和接续见 [启动性能记录](performance-startup-2026-10-06.md)。
第二轮根据完整路径统计增加窄GPR与I32内存算术的紧凑执行，移除raw重复取指并简化热循环。
三对同负载首Present19.078→18.032秒，吞吐76.70→82.14 MIPS（约+7%），133项release回归通过。
仍有约18秒启动延迟；热点仍集中在IR/cache与raw decode，继续暂停功能扩展。
详见 [第二轮性能与profile](performance-ir-2026-10-06.md)，FLAGS/checked memory/x87边界没有放宽。
第三轮已开始块内优化：安装/绑定时规划2–8条同页纯寄存器序列，共用查表/页检查/收尾，
保留所有guest入口、预算、callback ESP和完整FLAGS恢复recipe。
三对完整启动吞吐82.36→85.42 MIPS（约+3.7%），首Present17.984→17.425秒，收益仍较小。
这只是批量执行，还没有完成专用融合opcode或常量/复制传播；见[静态序列性能记录](performance-scalar-runs-2026-10-06.md)。
AliceToPapa 为64位Unity/Mono应用，现已建立独立 PE32+/64位内存/整数解释器与 probe64 诊断；
原样本已恢复并隔离实测：入口退休13条指令后停在 `GetSystemTimeAsFileTime` 的未实现 Win64 gateway。
UnityPlayer / Mono 装载探针分别因 TLS/delay imports、TLS 被拒绝，尚未进入引擎或建立窗口。
尚无 Win64 API runtime、Unity/Mono、D3D11 或64位 IR，普通 run 仍明确拒绝 AMD64。
基础回归和扩展范围见[x86-64样本](x86-64-sample-2026-10-06.md)。
以下保留此前功能推进阶段的历史记录；当前已经具备真实图像与 AppKit 上传。

# M2 进行中：最新进度与交接（2026-10-06）

M2 **尚未验收，不打 tag**。真实 guest DLL 装载、重定位、导出、DllMain 和无 callback 的静态 TLS 已实现，
`kinglove.tpm` / `KAGParser.dll` 已初始化，确认读取 data.xp3 和 patch.xp3 的索引/脚本内容。
按用户授权直接移植 Wine 的 GDI+ 矩阵/启动逻辑和窗口枚举，现已越过 `layerExDraw.dll` 的依赖缺失，
继续装载 varfile、KaichoTrans、addFont、scriptsEx。随后补齐当前所需的 guest VMM，
解除 `dirlist.dll` 的 reserve/commit 阻塞，dirlist 和 extNagano 完成初始化。
菜单/基础GDI/常用USER32/统一ABI成组实现后，真实游戏已经创建 TVPMainWindow，
继续到约 **452.7M 指令 / 1.128M API**，因 AdjustWindowRectEx 缺实现走错误退出。
还没有可见游戏画面或持续主循环。此前本地检查点包括 `75f90a6`（线程/SSE）、
`852eba3`（资源/堆/I/O/基础SEH）、`ca94a36`（x87/系统/NLS）、`a3de840`（主IR优化）和`f97ab8d`（真实profile/x87热点优化）。

2026-10-05 转入 no-JIT 性能路线，增加安装期预解码、8B typed I32 热路径与保守 FLAGS 分析，
并实现 CALL_NATIVE/IAT guard；IR原型不等于M2验收。

## 最新：主窗口逻辑建立，继续图形与呈现

后续 Win32 以用户指定的 Hangover 为主要参考，已核查固定 Wine 子模块的逐文件 LGPL 许可。
菜单/DC/bitmap/DIB/BitBlt、常用窗口/控件消息、BSTR、序号导出和16DWORD gateway 已实现。
真实游戏插件已继续装载，TVPMainWindow 真正创建，但尚未可见呈现；退出0依然是错误路径。
详细结果、限制和接续见 [功能组推进](common-win32-progress-2026-10-06.md) 与
[Hangover路线](hangover-win32-design.md)。

## 此前：Guest VMM 完成当前所需范围，准备图形渲染

实现独立于宿主 backing 的 4KiB guest 页、reservation/allocation 元数据及 checked 读/写/执行权限。
VirtualAlloc/Free/Protect/Query 和当前进程 Ex 变体支持 reserve/commit 分离、decommit/release、
保护与查询；PAGE_GUARD 一次性触发，PE 装载后应用 section 权限。
保护改变、写入、decommit/release 和重映射都失效 watched code，IR 绑定不消耗 guard。
没有生成 host 可执行页，也未启用 unchecked 访问。Windows VM 扩展、自动 stack 增长及 CPU-fault→SEH 仍待实现。

真实游戏的 4MiB reservation 和后续16个4KiB commit 成功，dirlist/extNagano 初始化完成。
补齐 Borland CRT 命中的 0x67 MOV/LEA 16位寻址，其他0x67指令保持明确拒绝。
最终 IR/raw 退休 **289,176,536 / 289,185,350** 条指令，全部 GPR/EIP/EFLAGS 相同，均退出3。
当前阻塞是 menu.dll 静态导入的 InsertMenuItemW，不把退出 API 或初始化轮询计数视为启动/性能验收。
完整 **102 项回归**、Clippy release all-targets、format 通过。

图形实施准备已写入设计：先补菜单真实状态解除插件边界，再建立 DC/bitmap/DIB 与逐像素 fixture，
然后推进 blit/混合、GDI+ 绘制和 Metal 窗口呈现。现有 SetPixel/GetPixel 尚未实现，不能当 framebuffer。
详情与复现见 [VMM 推进记录](vmm-progress-2026-10-06.md) 和 [图形渲染设计](graphics-rendering-design.md)。
进入可重复的持续主循环后，再做至少60秒、多场景、多次 profile 与性能复测。

## 此前：Wine Win32 逻辑移植，推进到 guest 内存保留

逐文件核查 Wine 10.0 的 LGPL-2.1-or-later，选择 LGPL 2.1 与 wire 合并，保留原版权、源码快照、
COPYING.LIB 与 SHA-256。GDI+ stdcall ABI 来自上游 spec；矩阵运算、逆变换、点/向量变换已移植，
对象使用真实 guest heap 和生命周期记录。未实现绘图/字体/编码器等功能返回 NotImplemented，
不将 GpStatus::Ok 用作默认返回；字体资源 API 目前只有 ABI，明确失败。
窗口枚举遵循 Wine 的快照/销毁跳过/回调 FALSE 停止流程，保留 guest 控制权转移和 LastError。
完整 **95 项回归**、Clippy all-targets、format 通过；IR/raw 重放停在同一内存 reserve 阻塞，全部 GPR/EIP/EFLAGS 相同。
该阶段之后的 VMM 和新阻塞见上方最新记录；持续主循环形成后再扩大性能采样。
详细实现、许可、限制和复现见 [Wine 移植推进记录](wine-win32-progress-2026-10-06.md)。

## 此前：真实 DLL/TLS 已装载，推进到绘图插件依赖

完整 90 项回归、Clippy all-targets、format 通过，自编译 MinGW DLL 的真实导入、DllMain、
重定位和卸载探针通过。DLL 使用独立 0x50000000..0x60000000 arena；主栈按 PE reserve eager commit，
解除 __chkstk 超出旧 128KiB 栈的问题。静态 TLS 对既有/新线程保持独立模板，失败/卸载清理已覆盖。
非空 TLS callback/delay import、线程 DLL 通知、循环依赖 SCC 卸载、完整 loader 搜索及 guard stack 增长仍未实现。
当时优先级为真实 GDI/GDI+ 与插件初始化，再推进呈现；进入持续主循环后再扩大性能 profile。
完整阶段结果及复现见 [DLL/脚本推进记录](game-dll-progress-2026-10-06.md)。

## 此前：推进到真实 guest DLL 装载边界

后续短性能检查点已增加分支链、空FLAGS计算跳过及精确整数倒数除法，81项回归通过。
五对短启动阶段中位数25.40→27.62 MIPS，仅约9%；用户指出样本太小，要求恢复游戏推进。
**当前优先级是继续真实DLL/插件装载**，进入持续主循环后再扩大profile与复测范围。
细节及测量限制见 [短性能检查点](perf-sprint-2026-10-05.md)。

按用户要求先推进游戏，再复测：加入基本 x86 RtlUnwind、异常跳出宿主回调的控制权传递，
类/窗口对象、创建/销毁/转发 guest WndProc、线程消息队列及消息定时器。
原游戏21.43M的RtlUnwind阻塞解除；先进入真实catch并报Unknown error，随后补齐类/窗口/SetTimer，
到达约24.40M指令、38.36K APIs的`kinglove.tpm`插件加载路径。
当前失败为`Not Susie Plugin`：旧LoadLibrary只是模块句柄，GetProcAddress仍为所有导出制造stub，
真实PE仅导出V2Link/V2Unlink，没有GetPluginInfo。下一步通用PE DLL装载需处理首选基址0x10000000与heap冲突、
重定位、imports/exports及DllMain；缺失导出应返回NULL。

完整79项回归、release build、Clippy/format通过。回调内逐条检查非局部栈转移，主解释循环继续使用连续IR。
collided unwind/自动CPU-fault SEH、窗口呈现/GDI/完整输入等仍待实现；逻辑窗口对象不等于屏幕上可见窗口。
推进后3次profile共2,938运行期样本，x87 42.65%、IR/cache 34.96%，未再采到常量解析热点。
无profiler的当前阶段IR/raw对照及功能/ABI边界见 [游戏推进记录](game-progress-2026-10-05.md)。
当前阶段IR三次为27.53/27.18/23.56 guest MIPS，raw为21.49/21.32/22.48；终态全部GPR/EIP/EFLAGS相同，
初始化计数略有差异，调度仍有波动，不能跨阶段构造加速比。

## 此前：真实 CPU profile 与 x87 热点优化

沿解释器主路径检查点 `a3de840`，使用完整游戏目录抓取四次 IR、两次 raw 的 macOS sample，
另抓一份 Instruments Time Profiler；排除启动/IR绑定，只分析 `Cpu::run_loop` 的运行期样本。
基线 4,781 个样本里软件 x87 占 **63.75%**，IR执行/缓存 **20.14%**；
固定十进制常量反复解析占 **15.92%**，APFloat通用除法 self 占 **27.88%**。
此前整数IR优化覆盖了有限的实际CPU成本。

本轮只解析一次LN2，FLD常数按四种RC缓存；级数中的1…255小整数除法用精确u128商/余数和
nearest-even舍入替换，subnormal/下溢风险/NaN/Inf仍回APFloat。保留binary128中间值、FP80最终舍入和异常逻辑。
26,536次与APFloat逐位oracle比较通过；FLDPI额外验证切换RC后的位模式。
优化后三次采样（2,420个运行期样本）x87占 **48.68%**，未再采到常量解析热点。

四对AB/BA无profiler对照：guest MIPS中位数 **15.02→18.76（约+25%）**，
全进程CPU时间中位数 **1.402s→1.044s（约−26%）**；每对CPU时间减少约19%…29%。
guest初始化有轻微计时路径差异，并非严格同路径；一次Instruments记录83%运行期样本在E core，
因此不能把跨批次绝对MIPS当成稳定上限。完整 **72项回归**、Clippy/format通过。
仍停在RtlUnwind，性能目标和M2均未验收。详细口径、热点表、复现命令与本机trace位置见
[CPU profile记录](cpu-profile-2026-10-05.md)。下一瓶颈仍是软件浮点division及IR调度，按profile继续定位。

## 此前：解释器主路径的运行时优化

用户再次明确：**解释执行为主，Wasm仅作可用时的加速；当前急需提升主解释器吞吐。**
本轮沿`dd17853`的IR原型继续，`6813158`已记录JSC可行性首个探针。

- `.wir`升级版本3，旧文件重新prepare；磁盘仍是64B冷记录，持久紧凑格式未完成。
- 8B typed load/store/LEA与MOVZX/MOVSX memory覆盖常见SIB、FS/GS、AH及窄寄存器保留语义。
  真实目标compact记录从320,634增至562,602；总记录1,083,963，文件仍69,373,688B。
- 热命中结果减少冷记录复制，16B热条目带next PC/slot；源绑定拒绝记录不能被预链接。
  顺序执行仍逐条检查代码页代数，跨页查两页，自修改继续回退。
- 连续typed解释段摊薄外层调度，遇分支、冷操作、fallback与stub返回；无观测时批量，单步/trace/stats逐条。
  精确停在预算与协作分时边界，中途fault仅累计成功退休的前缀，线程FLAGS完整保存。
- CLI默认关闭opcode频率计数，避免每条指令更新HashMap并使普通IR运行自动获得连续调度；
  `--op-stats`显式开启，旧`--no-op-stats`兼容。CPU trace继续按需开启。
- checked GuestMem将动态页尺寸除法换成经过验证的shift，首个页单独检查，跨页逐页验证；代码写跟踪保留。
- FLAGS静态证明未放宽，全部恢复边界继续有效，没有unchecked memory路径。

`cargo test --release` **71项通过**：1 unit、34 differential、5 FPU、10 IR、1 Wasm、20 Win32。
新增预算各位置/中途fault退休计数、精确分时、链接失效及精确窄访存检查。
Clippy all-targets、format、build通过。control两后端仍为641指令/43 APIs/相同21B输出。

Apple M4/macOS26.5.1，关闭trace/opcode profiling、顺序运行、AOT/装载不计时。
10M轮合成样本，完整终态对照：

| 循环 | raw guest MIPS | 主IR guest MIPS |
| --- | --- | --- |
| ALU/branch（60,000,003 guest） | 43.49 / 35.30 / 36.40 | 66.99 / 61.56 / 55.17 |
| checked SIB load/add/store（50,000,004 guest） | 42.04 / 36.66 / 38.74 | 79.32 / 78.11 / 75.93 |

完整游戏目录最终三对旧IR/新IR：13.44→17.46、15.32→16.15、14.85→15.67 guest MIPS。
**第一对路径完全一致**：21,423,118指令、24,568 APIs、全部GPR/EIP/EFLAGS及RtlUnwind阻塞相同，吞吐约+30%。
其他两对存在轻微guest初始化路径差异，不能视为严格同路径加速比；本机raw与各阶段绝对值也有显著负载波动。
所有结果在`wire-out/ir-performance/`，不能与此前23–25 MIPS游戏样本直接相减。
主路径仍偏低，尚未达到长期性能目标。约23%指令raw fallback；一次开启统计的独立采样中0f共4.03M、
x87 d9约0.786M，应继续细分未提升0f/SSE、x87、内存算术与其他冷操作，而非转去依赖Wasm。
游戏仍停在RtlUnwind，没有data.xp3读取/可见窗口，M2不验收。

可选Wasm实验已从执行探针推进至**实际IR直接lower单个寄存器自然循环**，全FLAGS与每guest预算恢复，
ALU50/FLAGS86个512B状态对照通过；公开JSC native shim、嵌套回调、临时TypedArray指针同步fread、
grow/旧view/短读边界已验证。macOS需公开allow-jit签名才能证明实际JIT；本机默认fast-memory越界探针SIGBUS，
诊断关闭该引擎选项后通过，尚无公共API生产解法。该实验未接入wire主程序，不是完整guest backend，
目标iOS仍未验收。详见 [JSC记录](wasm-jsc-feasibility.md)；解释器编码/性能/总cache尺寸见 [IR设计](ir-aot-design.md)。

## 此前检查点：安装期 IR 与计划评审（2026-10-05）

- QEMU整体GPLv2，但v10.1.0的i386 translate.c/cc_helper.c为LGPL-2.1-or-later，tci.c为GPL-2.0-or-later。
  本轮未导入QEMU实现；采用MIT iced-x86 decoder和独立wire语义。已补齐新增依赖/许可原文。
- `wire prepare EXE --out FILE` 生成EXE SHA-256绑定的版本2 `.wir`；`wire run EXE --ir FILE` 使用IR。
  默认后端仍为原解释器，`--no-op-stats`用于公平比较。运行时不生成宿主机器码。
- 常见I32 GPR操作和直接分支已经是8B typed instruction，内存/窄寄存器等走预解码操作，
  SSE/x87/其他未覆盖指令回退；不声称最终紧凑后端已完成。磁盘保留64B冷记录，内存热条目12B，
  512B bank、紧凑文件、稀疏索引和block chaining尚未实现。
- FLAGS只在连续直线段内证明后续确定覆盖、无读/观察时省去即时计算；调用/控制流/faultable访问全live。
  有lazy recipe用于单步、预算、fault、线程与API边界完整恢复；不做跨块/函数猜测删除。
- CALL_NATIVE是一等opcode，记录image-local IAT身份、bind后保存guest stub并每次验证当前IAT，
  hook后调用真实guest目标；仍复用现有API dispatcher/stack cleanup/callback，不序列化宿主指针。
- CPU/API/heap的checked writable pointer统一更新代码页代数；页写后回退，未预解码/重叠入口回退。
  当前GuestMem仍只做范围/commit检查，不具备完整VMM权限，尚不能启用unchecked memory fast path。
- 评审了用户提供的Project design.md：采纳两层IR、少分发、统一gateway/VMM和可测指标；
  用户进一步明确接受Wasm经JSC使用系统JIT，并已完成本机公开API执行/内存探针。
  x86-64、整个Wine移植和通用codec replacement留作后续范围；完整Wasm后端尚未实现。
  具体修正见 [方案评审](project-plan-review.md)，IR编码/寄存器/FP80/边界见 [设计](ir-aot-design.md)。

验证：`cargo test --release` **66项通过**（1 unit、34 differential、5 FPU、6 IR、20 Win32）；
34族CPU差分增加IR与raw x86对照，仍以Unicorn为参照。format/Clippy/build通过。
IR边界回归包括FLAGS静态证明、预算/fault恢复、自修改、重叠入口、文件错误、FS/SIB/高字节及IAT hook。

性能：关闭trace/opcode profiling，AOT/装载不计入运行时间。3对18M指令合成循环的raw为
59.18/60.14/58.67 guest MIPS，IR为70.03/76.75/79.69；只是ALU/branch样本。
control两后端均正常退出，641条指令、43 API调用、完全相同21B输出；IR638/fallback3/CALL_NATIVE7。

完整目标目录、相同配置下两对真实游戏测量：

| 对照 | raw guest MIPS | IR guest MIPS | guest指令 / API调用 |
| --- | ---: | ---: | --- |
| 1 | 23.32 | 25.02 | 两后端均21,422,574 / 24,560 |
| 2 | 23.58 | 24.07 | 两后端均21,420,983 / 24,552 |

每对的全部GPR、EIP、EFLAGS和RtlUnwind阻塞相同；不同重复之间guest路径也略有变化，
不能把一次结果当稳定上限。第二对IR命中16,496,618条、fallback4,924,365条（约23%）、
CALL_NATIVE23,414次。仅约2–7%的实际收益，**没有解决最终性能目标，也没有解除功能阻塞**。
日志与report在忽略的 `wire-out/ir-validation/full-game/`，没有加入游戏代码/资产。
曾另用只含.cf的临时文件根验证过短路径，不能与完整目录的21M路径混作同一个benchmark。

目标IR文件69,373,688B、1,083,963条冷记录，320,634条compact。
当时优先级为typed memory/寻址融合和顺序执行；本轮已落地，后续继续按运行热点扩大覆盖与优化块调度，
再压缩持久格式与resident cache。不要优化AOT转换速度来代替运行时收益。

JSC路线补充：`examples/jsc_wasm_probe.c`通过系统JavaScriptCore.framework得到Wasm add=42，
再经`JSObjectGetTypedArrayBytesPtr`同步写linear memory，由Wasm读回123456。
直接ArrayBuffer API会拒绝WebAssembly.Memory，TypedArray API路径可用但指针有效期受限。
此为本机macOS执行/内存可行性，尚未证明目标iOS的实际JIT或性能；见 [验证记录](wasm-jsc-feasibility.md)。

## 已完成并验证

### 解释器与差分测试

- 原有 24 组 CPU 差分测试全部保留；现在共 **34 组**。
- 初始 XMM 状态随机化，终态比较全部 8 个 XMM 的 128 位；原有 GPR、EFLAGS、数据/栈内存比较继续执行。
- 新增 SSE 传送、整数/逻辑/unpack/shuffle、移位、浮点算术/比较/转换和标量边界访问测试。
- 修复 `UNPCKLPD/UNPCKHPD` 错用 32 位 lane、`MOVQ xmm,xmm` 未清高 64 位、
  多种标量 SSE 指令按 16 字节读取内存的问题。
- 新增 `SHLD/SHRD`（0f a4/a5/ac/ad，16/32 位、立即数/CL、寄存器/内存），
  并验证 count=0/1、按 5 位屏蔽计数和 ISA 定义范围内的标志。
- `examples/lockstep.rs` 现在也比较 XMM；`--xmm-hex` 可传入 8 个逗号分隔的 16 字节初值。
- Unicorn 2.1.5 对 `CVTPS2PD m64` 有 16 字节过度读取：普通转换仍做差分；
  精确 m64 边界访问用独立断言验证 wire，未把这个参照缺陷复制到解释器。

### 线程与同步

- 延续已有单宿主 CPU 的协作式调度，切换点为阻塞 API、显式 yield 和约 1M 指令的分时片。
- TLS/FLS 的值按 `(tid, slot)` 保存；LastError 按当前 TEB 的 +0x34 保存。
- 工作线程初始化 x87 控制字为 0x037f、MXCSR 为 0x1f80。
- 原 TEB arena `0x7ffc0000+` 的第 30/31 个槽会覆盖主 PEB/TEB；迁至 **0x7ff00000+**，
  并通过创建 40 个工作线程的回归检查。
- 分时片会处理等待超时，防止另一个持续 runnable 的自旋线程让超时永不唤醒。
- 修复 `InterlockedCompareExchange` 的 Exchange/Comparand 参数颠倒；
  `InitializeCriticalSectionEx` 的 SpinCount 使用第二参数。
- 新增真正执行 guest 代码的事件唤醒/线程退出测试，以及 wait-all 原子获取测试。

### 资源与 stdcall

- `LoadStringA/W` 使用 **16 字节**参数清理（旧交接写的 20 字节错误）。读取映射 PE 的真实 RT_STRING，
  支持 W 版本 capacity=0 的资源指针返回、截断和 NUL 终止，A 版本转换为 CP932。
- `FindResource[A/W/Ex]` 支持数字/名称键；`LoadResource/SizeofResource/LockResource` 可直接读取映射资源。
- 资源目录与字符串长度有边界检查。
- 补齐特殊目录、PathIsDirectory、WNetGetUniversalName、RtlGetVersion、GetClassInfoEx 等栈清理，
  并覆盖原静态导入清单内所有缺失项（视频/对话框等仍可保持功能未实现，但栈参数大小已知）。
- 单元测试检查目标程序 **346 个静态导入**都有清理记录；最新游戏日志的 unknown arity 警告为 0。
- `FormatMessageA/W` 支持少量系统错误文本、FROM_STRING 和 ALLOCATE_BUFFER；生成的块可由 LocalFree 真正释放。

### 真实堆

- 独立 guest arena **0x10000000..0x30000000**，16 字节对齐，分离 scratch/线程栈。
- HeapAlloc/Free/ReAlloc/Size/Validate/Create/Destroy 使用分配表和可合并空闲区；
  realloc 保留内容，支持原地增长、移动、HEAP_ZERO_MEMORY 和 IN_PLACE_ONLY 失败保留原块。
- 私有堆拥有独立句柄与分配归属检查；固定最大大小按分配容量检查。
- CRT malloc/calloc/realloc/free 以及固定型 Local/GlobalAlloc/Free 接入同一实现。
- DWORD 尺寸溢出或 arena 用尽返回失败，不再以 bump 分配冒充 realloc。

### 编码、路径与同步文件 I/O

- 系统 iconv 提供 CP932、Windows-1252、UTF-8 转换；实现 MultiByteToWideChar/WideCharToMultiByte，
  包括长度查询、显式长度、-1 的 NUL 计数、缓冲区不足及 CP932 非法输入的严格/替换处理。
- GetCPInfo(932) 返回 MaxCharSize=2 和正确 lead-byte 范围。
- SSE/SSE2 检测 6/10 返回 1；SSE3 仍不广告完整支持。
- `wire run --cwd DIR` 指定文件根；默认 EXE 所在目录。根映射为 guest `C:\`，
  guest 当前目录独立于宿主 cwd。shell 的特殊目录暂时统一映射到这个根。
- 支持 A/W 路径、大小写匹配和反斜线；拒绝 `..` 越界、其他盘符、UNC/device 路径及指向根外的符号链接，
  创建新文件也检查最近存在的父目录。
- CreateFile、ReadFile、WriteFile、GetFileType、GetFileSize/Ex、SetFilePointer/Ex、SetEndOfFile、
  FlushFileBuffers、CreateDirectory、Get/SetCurrentDirectory、GetFullPathName、GetFileAttributes、
  PathFileExists/PathIsDirectory、FindFirst/Next/Close、CloseHandle 已落地。
- 最新游戏日志显示 `.cf` 配置文件成功打开、读取 **927 字节**，随后正确读到 EOF。
- QPC 计数修正为与 QueryPerformanceFrequency 一致的 10 MHz。

### 基础软件 SEH

- SetUnhandledExceptionFilter 存储回调并返回旧回调。
- RaiseException 构建 x86 EXCEPTION_RECORD、CONTEXT、EXCEPTION_POINTERS，沿 FS:[0] 遍历处理器，
  用 guest 回调执行，支持 ContinueSearch 与 ContinueExecution 及处理器修改 CONTEXT 后恢复。
- 未处理时调用已注册 filter；不能恢复的异常会明确报出异常码，而不再直接返回并落入 int3。
- 检查 noncontinuable、循环链与不支持的 disposition。
- guest 回调的返回哨兵绑定到调用线程 TEB，避免其他线程误结束当前宿主回调。
- **未实现完整 MSVC EH / RtlUnwind、nested/collided unwind，也没有把 CPU 内存 fault 自动转为 SEH。**
  不应把基础 dispatch 测试通过视为完整 C++ 异常支持。

## 本轮新增：软件 x87（2026-10-04）

- `src/cpu/fpu.rs` 实现 8 个物理 FP80 寄存器、完整 tag word、TOP、控制/状态字和环境地址；
  `ThreadCtx` 保存整套状态，SEH CONTEXT 的 FLOATING_SAVE_AREA 与初始扩展区也填入寄存器数据。
  测试覆盖真实调度切换，以及 SEH 回调破坏现场后从修改过的 CONTEXT 恢复。
- 32/64/80 位 FLD/FST/FSTP、16/32/64 位 FILD/FIST/FISTP/FISTTP、寄存器加载/交换/释放/存储，
  加减乘除及反向/弹栈形式、FCOM/FUCOM/FCOMI/FUCOMI、FCMOV、FCHS/FABS/FTST/FXAM、
  FLD 常数、FRNDINT/FSQRT/FSCALE、环境保存恢复与 FNSAVE/FRSTOR 已接入。
- 基础算术使用 `rustc_apfloat` 的 80 位软件格式；PC=24/53 保持 x87 的 15 位指数范围，
  使用 round-to-odd 中间结果避免 64 位到低精度的双重舍入。四种 RC 均覆盖。
  FSQRT 使用整数平方根与余数做舍入，不依赖宿主 f64。
- CRT `pow` 的 FYL2X、FYL2XP1、F2XM1 使用软件 Quad 中间运算和收敛级数；
  对数归一化避开接近 1 时的相减消失，log1p 路径保留极小增量。
  独立高精度十进制参考验证 sqrt(2)、log2(3)、2^0.5-1、log2(1-2^-64)、log2(1+2^-200)。
  **这不是逐位仿真某款 Intel 微码的超越函数，尚未证明所有极端输入的正确舍入。**
- 异常 sticky bits、masked stack overflow/underflow 的 indefinite、比较条件位、舍入 C1 已验证；
  FNCLEX 保留 TOP/条件码。待处理未屏蔽异常在 FWAIT/后续 waiting 指令明确报错。
  **尚未实现完整的未屏蔽异常精确提交/取消和向 SEH 的 #MF 转换。**
- FPREM/FPREM1、三角函数、packed BCD 等仍明确 Unsupported；不是完整 x87 ISA。
- Unicorn/QEMU 对 x87 异常位和 C1 的报告不完整、FCOMI 未清 SF/AF/OF、masked stack fault
  和 F2XM1(-0) 也与架构要求不一致。差分对比数值、栈顺序、TOP 和比较条件；
  上述部分用独立架构断言验证，没有为了匹配参照而复制这些缺陷。

## 本轮新增：初始化系统/计时/NLS

- GetSystemInfo/GetNativeSystemInfo 返回 guest x86、单 CPU、4 KiB 页、64 KiB 分配粒度。
  GlobalMemoryStatus/Ex 填写完整结构，Ex 校验 dwLength=64；报告固定 512 MiB guest 物理内存，
  可用量随实际提交的应用地址空间页面减少。这是兼容层硬件配置，不是宿主硬件透传。
- timeGetTime 与 GetTickCount 共用单调毫秒时钟；GetTickCount64 正确返回 EDX:EAX。
  timeGetDevCaps、timeBeginPeriod/timeEndPeriod 提供参数校验；周期请求不改变宿主定时精度。
  GetSystemTime 返回真实 UTC SYSTEMTIME。
- GetStringTypeW/ExW 的 CT_CTYPE1 与 LCMapStringA/W/Ex 大小写映射支持长度查询、显式长度、
  -1 包含 NUL、容量不足错误和 case mapping 原地写回。CRT 的 256 字符初始化现在成功。
  Unicode 分类和大小写使用 Rust 表；CT_CTYPE2/3、排序键、locale 特殊规则等仍未实现，
  分类子集不是完整 Windows NLS 数据库。

## 最新验证结果

- `cargo test --release`：**60/60**（1 个静态导入检查 + 34 个差分族 + 5 个 x87 架构测试 + 20 个 Win32 行为测试）。
- `cargo build --release`、`cargo fmt -- --check`、`cargo clippy --all-targets -- -D warnings` 通过。
- 对照 EXE：退出码 0；宿主输出再次确认为 `wire control program\n`，21 字节。
  窗口 API 仍为 stub，所以退出码只验证加载/解释/文件路径，不代表图形验收。
- Unicorn 差分在沙箱外执行；发布可执行文件始终为纯解释器。
- 游戏 `.cf` 成功读取 927 字节并到 EOF；静态导入 unknown arity 警告为 0。

## 最新游戏阻断

最终运行：**21,425,797 条指令 / 24,592 次 API 调用 / 197 种 opcode**。
线程等待和计时会让确切计数略有变化。

```text
outcome: CRASHED: api dispatch: api dispatch: RtlUnwind/MSVC stack unwind is not yet implemented
EIP=0x6fff05a0, ESP=0x0019fd44, EBP=0x0019fd78
RaiseException(0xe06d7363, 1, 3, 0x0019fd6c)
```

原 0x00691284 的 FLD m64 阻断已解决；CRT 数学路径通过后抛出 MSVC C++ 异常。
日志显示搜索处理器 0x6e939b / 0x6da37b / 0x6d9644，随后经 0x67ec25 调用 RtlUnwind。
之前的 RegisterClassExW/GetClassInfoExW 仍返回 stub 失败；需排查异常原因与窗口初始化的关系，
不能把绕过异常或模拟窗口成功当成完成 C++ unwind。
本轮仍未读到 data.xp3 或进入可见游戏画面。

## 最新下一步（按序）

1. 实现通用 guest PE DLL 装载、重定位、真实 imports/exports 与 DllMain，先用自编译DLL验证，
   再解除kinglove.tpm的stub路径；不要伪造不存在的GetPluginInfo导出。已有RtlUnwind正常展开，nested/collided特殊路径仍需单独验证。
2. 接入真实窗口呈现/GDI，并补当前逻辑窗口/消息子集未实现的可见主循环需求。
3. 检查 XP3 大文件读取、存档目录与插件调用后的实际需求。
4. 补剩余实际触发的 x87/Win32 语义，并继续差分与独立行为测试。
5. 达到可见主循环后再验收 M2，开始 M3 的 GDI 帧缓冲/音频/输入接入。

## M3 已认可路线

保留上轮与用户确认的 **SDL3 + 薄平台层**：`src/platform/` 定义呈现帧缓冲、音频队列、
窗口事件接口；SDL3 为首个后端，Rust 用 sdl3-sys 薄封。GDI 软件 blit 上传 streaming texture，
waveOut 接 SDL_AudioStream。iOS 从设计开始考虑 main-callback 模式；需要时可替换为 UIKit/Metal。
图形对照截图在 M3 再收集，本轮未开始引入 SDL3。

## 环境与复现

- 游戏：`/Users/alphaly/Downloads/王様恋愛【体験版】/王様恋愛【体験版】.exe`，data.xp3 约 803 MB。
- commit/tag 使用 `-c commit.gpgsign=false` / `-c tag.gpgsign=false`。
- STUB_BASE 0x6fff0000；scratch 0xf00000；主栈 0x180000；主 TEB 0x7ffdf000；PEB 0x7ffde000；
  工作线程栈 0x2000000+；**工作线程 TEB 0x7ff00000+**；真实堆 0x10000000+；句柄 0x100 起。

```sh
cargo test --release
cargo clippy --all-targets -- -D warnings
cargo build --release
./target/release/wire run "/Users/alphaly/Downloads/王様恋愛【体験版】/王様恋愛【体験版】.exe" --max-instr 50000000 --trace-cpu
# 本轮最终日志：wire-out/m2-x87-system/api.log；首轮 x87 日志：wire-out/m2-x87/api.log。
./target/release/wire run tests/control/control.exe --cwd "$PWD/wire-out/control-cwd" --log-dir wire-out/control-m2
```
