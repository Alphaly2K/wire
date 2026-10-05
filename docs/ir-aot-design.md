# 安装期 x86 → 定长 IR：许可核实与原型设计（2026-10-05）

## 许可结论与实现边界

wire 的 Cargo.toml 标明 LGPL-2.1-only。QEMU 整体以 GPLv2 发布，但不是每个文件都是 GPL。
本次核实固定版本 v10.1.0：

- [官方项目许可](https://www.qemu.org/docs/master/about/license.html)：整体 GPLv2，部分文件采用其他许可。
- [target/i386/tcg/translate.c](https://github.com/qemu/qemu/blob/v10.1.0/target/i386/tcg/translate.c)
  和 [cc_helper.c](https://github.com/qemu/qemu/blob/v10.1.0/target/i386/tcg/cc_helper.c)
  文件头为 LGPL-2.1-or-later，可以选择 LGPL 2.1。若实际移植，仍必须保留版权/许可、记录来源和修改，
  并逐个核查相关文件、模板及依赖；不能根据两份文件头认定整个 TCG/x86 子系统都是 LGPL。
- [tcg/tci.c](https://github.com/qemu/qemu/blob/v10.1.0/tcg/tci.c) 为 GPL-2.0-or-later。
  不能把它复制或逐句改写成 Rust，再仅按 LGPL-2.1-only 分发。

本轮仅核实 QEMU 许可头，运行时、IR 和标志计算从现有 wire 实现独立编写，没有导入 QEMU 代码。
参考 ISA 行为、抽象算法或把独立模拟器用于差分，不等同于复制受版权保护的源码表达；
机械转写、近似结构/表格复制并不因换语言就成为独立实现。此处结论针对这条具体实现路线，
不是对未来任何形式的“参考 QEMU”作法律担保。

前端选用 MIT 许可 iced-x86 的 decoder/instr_info 功能，不启用编码器或汇编器；
SHA-256 内容绑定选用 sha2 的 MIT 许可选项。第三方代码留在各自依赖中，不复制进项目。

## App Store：不能靠把代码称为数据保证通过

[Apple App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)
（2026-10-05 读取）：

- 2.5.2 限制下载、安装或执行会引入/改变 app 功能的代码，不以“CPU 是否能直接执行”作唯一判断。
- 4.7 明确允许 retro game console 和 PC emulator apps 提供游戏下载，同时适用 4.7.1–4.7.5。
- 4.7.2 限制未获许可向这些软件暴露原生平台 API/技术。

本项目的约束是wire不自建宿主机器码JIT；用户已接受将Wasm交由系统JSC执行/JIT的可选加速路线。
IR不自动免除审核规则，使用JSC也不等于任何平台/签名配置都能获得相同JIT能力。
合法分发应建立在实际模拟器类别、内容/导入方式和 API 能力边界上，不能把改格式当作审核保证。
安装期 IR 仍然派生于原游戏代码，不自动消除原游戏的版权或分发限制。

## 优先目标与两层 IR

优化目标是**运行时 guest x86 吞吐**。安装期允许更慢的 CFG、数据流分析、常量折叠和打包；
不能用 AOT 翻译速度提高来代替解释执行速度提高。首先支持当前 PE32 目标，x86-64 留待单独扩展。

采用 `x86 → Semantic IR → 保守优化 → Execution IR → 解释器`。
Semantic IR 可以有 SSA、类型、地址表达式和完整 guest 边界信息，不必强求定长；它不在每条指令热路径上解析。
Execution IR 保持 opcode 集合小、类型明确、取指简单。AOT 优化在导出执行格式前完成，
运行时不做 x86 变长解码，未覆盖入口和动态/修改过的代码由原 x86 解释器兜底。

**解释执行是主路径**，优化优先依据该路径的实测热点。Wasm仅在平台能力、语义与状态同步均满足时加速；
当前wire主程序未接入Wasm调度。同一Semantic IR也可直接lower为Wasm操作，由平台JSC编译/执行；不在Wasm里再次解释Execution IR。
进程内JavaScriptCore与WKWebView的内存/gateway条件分别测量，详见 [JSC可行性记录](wasm-jsc-feasibility.md)。

执行成本同时取决于每次分发成本与分发次数。`IR ops / retired guest instruction ≤ 1.5` 是测量目标，
不是正确性约束；复杂 x87 指令可以调用预编译的软件实现。优先用 fused typed opcode/共享寻址描述符，
避免把一条普通 x86 指令拆成大量地址、加载、运算和存储分发。100–200 MIPS 等数值属于待验证目标。

## 目标 Execution IR：固定 8B，类型编进 opcode

建议以两个 little-endian u32 组成一条 **8 字节**指令。以下编码已用于本轮整数热路径子集：

| 字段 | 位数 | 含义 |
| --- | ---: | --- |
| opcode | 8 | 操作及类型；如 ADD_I32、ADD_IMM_I32、LOAD_U8、FADD_F80 |
| dst | 6 | 目标 vreg，0–63 |
| srcA | 6 | 第一源 vreg，0–63 |
| srcB | 6 | 第二源 vreg，0–63 |
| metadata | 6 | 算术为 eager_flags；内存/LEA 为 scale、segment、base/index presence |
| aux | 32 | 由 opcode 唯一决定：立即数、常量池/地址描述符/分支/导入编号 |

第一 word 从低到高为 `opcode:8 | dst:6 | srcA:6 | srcB:6 | metadata:6`。
算术 opcode 的 metadata 为 eager_flags。
标志掩码是六位紧凑编号，不能与 x86 EFLAGS 原位值混用。未用字段必须置零；不把 vreg63 当 sentinel。
寄存器型 ADD_I32 与立即数型 ADD_IMM_I32 是不同 opcode，handler 不再读取独立 width 字段。
Jcc 采用 16 个条件 opcode，aux 为目标；控制流的最终持久格式应使用 image RVA 或 block ID。

LOAD/STORE/LEA 不改 FLAGS，复用 metadata：bit26–27 为 scale 的 log2，bit28–29 为
segment（0=平坦、1=FS、2=GS），bit30/31 分别表示 base/index 存在。
srcA/srcB 是 base/index，aux 是32位 displacement；dst 在 STORE 中是待写源寄存器。
LOAD/STORE_I32、LOAD/STORE_GPR8/16、LOAD_U8/U16/S8/S16_I32 与 LEA_I32 都是一条8B操作，
无需额外寻址 dispatch。GPR8 的编号4–7沿用 x86 AH/CH/DH/BH 编码；GPR16/8 load 保留未写位。
FS/GS 在运行时读取线程基址，LEA 不加 segment。

最小核心包含常量/移动、整数算术/扩展、guest load/store、条件与控制流、CALL_NATIVE、
受控 fallback；暂时不为整个 x86 ISA 枚举一套 IR opcode。只有新增热点、能减少总执行成本时才加融合指令。
opcode 编号与字段含义必须版本化，不序列化 Rust enum ABI、函数地址或宿主指针。

64 位整数/F64 常量置于共享 LE 常量池，FP80 常量保持原始 80 位。寻址描述符保存 base、index、scale、
displacement、segment，FS/GS 基址在运行时取当前线程值，不能在安装期绑定主线程 TEB。
常量池、地址描述符、guest-PC 映射和恢复表都要计入**总 IR 尺寸**；8B 指令不等于每条 guest 只占8B。
若 8B 编码无法兼顾实际热点，应通过对照测试决定是否整体升级为另一个定长格式，避免添加变长 escape 解码。

## vreg file：64 × 8B = 512B

用户提出的编号适合调试、跨边界和简单解释器。建议保留编号，但修正 x87 的精度表示：

| vreg | 用途及不变量 |
| --- | --- |
| 0–7 | EAX、ECX、EDX、EBX、ESP、EBP、ESI、EDI；PE32 值位于低32位 |
| 8 | EFLAGS 伪寄存器；允许内部 lazy recipe，观察点必须得到完整标志值 |
| 9–15 | 保留；可分配 guest EIP 镜像、FS/GS 基址等，具体编号需版本化 |
| 16–31 | **八组双槽 FP80**：16/17、18/19…30/31；每组低槽存64位 significand，高槽低16位存 sign/exponent |
| 32–63 | 翻译期临时量；纯 guest 数值/地址，不装宿主裸指针 |

512B 是 vreg bank，不是完整 CPU context。另有 x87 TOP/tag/CW/SW/环境地址、XMM 的8×128位、
MXCSR、FLAGS recipe、调度和故障恢复元数据。线程上下文不能漏掉这些状态。

**不能把一般 x87 寄存器直接改成 F64。** 即使 PC=53，x87 仍有扩展指数范围；中间舍入、NaN、
denormal、异常及可观察的80位存取也不等价。F64 可以服务于实际 F64 运算或经证明等价的快路径。
静态消解 TOP 必须证明所有入边的栈深与映射一致；FXCH、push/pop、调用及 FLDENV/FRSTOR
等会使证明失效。未知入口或不一致 join 保留动态 TOP，并同步物理 FP80 状态。

I8/I16 写回 EAX…EDI 必须保留其余位，AH/CH/DH/BH 需要显式高字节抽取/合并语义；
普通临时量的窄写可约定零扩展。不能让 opcode 类型隐含地破坏 x86 子寄存器规则。
临时量默认不跨 guest 控制流/ABI 边界存活；确需跨越时，必须有恢复表和线程保存约定。

## FLAGS：极度保守的静态证明

本轮只做连续直线段的逆向逐位活性分析。入口默认不知道后继需求，出口令六位全部 live。
只有能证明某位在任何读/观察点之前，被后续指令**确定覆盖**时，才允许跳过即时计算。
当前规则：

- 所有跳转、调用（含 CALL_NATIVE）、返回、fallback、不连续地址都是全 FLAGS 边界。
- 可能 fault 的内存/栈访问是全 FLAGS 边界；内存算术本身也完整计算自己的标志。
- ADC/SBB 读取 CF；INC/DEC 保留 CF，并保守记为读取；CMOV/SETcc 按条件读取所需位。
- PUSHFD、LAHF、环境保存、异常/CONTEXT 等未知消费者不假定无害，先完整物化再 fallback。
- 不做跨块/跨函数消除，不依赖“硬件未定义标志以后必然无人读取”的猜测。

静态掩码控制即时计算；被省掉的位仍留有 a/b/result/carry/type recipe，便于预算停止、单步、
线程保存、CPU fault 和宿主边界准确恢复。后续确定覆盖之前没有观察时，旧 recipe 才可以直接丢弃。
**lazy 表示是恢复保障，不能拿它代替用户要求的静态证明。** DF/IF 等其他 EFLAGS 不参与此优化。

将来 CMP/TEST + Jcc 融合时，不能把“只读取 ZF”误当成“其他位全死”：两个 guest 指令之间可能有
单步/预算边界，分支后也可能读取其他 FLAGS。必须保存两条原始 guest 的精确恢复点，或在边界拆开执行。

## CALL_NATIVE：一等 opcode 与受控 Win32 gateway

把 CALL_NATIVE 作为一等 opcode 是合适的。这里调用的是**已编译进宿主的 Win32 API shim**，
并不直接执行 Windows 内核 syscall，也不把 Windows 调用号映射成 Darwin syscall。

目标格式的 aux 指向 image-local 导入描述符：DLL、名称/ordinal、IAT RVA 与 ABI profile。
装载时解析到受控 gateway ID；磁盘上不出现 ARM64 地址或可执行 trampoline。动态解析使用相同注册表。
stdcall/cdecl/fastcall/thiscall 的参数位置、栈清理、返回值、LastError、guest pointer/结构转换
由 ABI profile 明确规定，不能依赖宿主 C ABI 的参数/指针宽度。

执行前物化 FLAGS 并同步全部可观察 guest 状态。保留真实 x86 返回地址与栈布局，
支持 blocking/restart、线程切换、嵌套 guest callback、SEH/异常恢复；不得在返回时盲目覆盖 callback 更新的状态。
反向调用使用预编译入口和 callback token，不生成 ARM64 trampoline。

**IAT 必须运行时检查。** 安装时识别的 `call [IAT]` 可能后来被 hook；目标改变时走当前 guest
目标的普通 CALL，不能绕过 hook。未知导出不以任意函数指针或 FFI 调用方式尝试执行。
Native replacement（FFmpeg/zlib 等）是另一层优化，只有版本、完整 ABI、对象/分配生命周期和回调都匹配时才启用。

当前原型已具有 CALL_NATIVE opcode，用 IAT RVA 作 image-local identity，bind 保存已解析的 guest stub，
每次调用检查 IAT。仍通过现有 run-loop API dispatcher 执行，保留既有栈清理/线程/回调机制；
尚未实现跳过 stub 检查的直接 gateway 快路径或通用第三方库替换。

## Guest 边界、内存与代码失效

每条原始 guest 指令要能找回 PC、长度、IR 范围和 fault 恢复状态。统计按 retired 原始 x86 指令，
融合后不得按一条 IR 算一条 guest；分时片/预算/调试也遵守原始边界。内部临时操作不能暴露半更新的 GPR/FLAGS。
当前原型是一条 guest 对一条热路径/预解码操作；故障行为对照现有 wire，尚未补齐完整 x86 fault→SEH。

guest memory check 与 FLAGS 消除相互独立。当前 GuestMem 只做32位范围和 committed-page 检查，
并无完整 VirtualProtect/PAGE_GUARD/4KiB guest protection；不能声称已有权限语义。
VMM 应在任何省略访问检查的快路径前实现。Apple Silicon 的16KiB宿主页不等于 Windows 的4KiB保护页。

所有经 checked writable pointer 的 CPU/API/heap 写入，均递增 watched-code-page generation。
代码页写后永久回退原解释器，本轮不运行时重译；跨页指令检查两页。未预解码、重叠入口、
修改/重定位导致源字节不一致时也回退。PE 节混有数据，线性扫描不是完整代码发现证明。
宿主写、解除映射、保护变化和将来的多宿主线程都需由统一 VMM 纳入失效规则；当前只支持单宿主 CPU 协作调度。

## 当前实现与目标格式的距离

当前 `.wir` **版本3**仍使用56B头和64B固定预解码记录，以 EXE SHA-256 绑定源文件，字段显式 LE。
版本2文件需重新 `wire prepare`。常见 I32 GPR 移动/算术/比较/立即数、NOP、直接分支、
上述 typed load/store/LEA，在 AOT 时生成上面8B instruction，放入冷记录的两个 word。
装载后建立独立的 **16B 热条目（8B指令 + 4B next guest PC + 4B next slot）**。
命中结果只含热条目及索引，慢路径才复制完整64B记录；避免大 enum 让每次命中都搬运冷数据。
顺序 successor 在 bind 完成源字节验证后建立，只能指向已接受记录；运行时用 expected PC 验证 hint，
每条指令仍检查代码页 generation，跨页检查两页。修改代码、分支、fault、fallback 和线程恢复
不会绕过这些检查。页索引使用 shift，消除热路径动态除法。
无trace/opcode统计时，连续typed指令在一个解释段内调度，减少每条指令重复的外层halt/API分发；
CLI的opcode统计现默认关闭，`--op-stats`显式开启；旧`--no-op-stats`保留兼容。
分支、冷记录、fallback与stub边界返回外层。预算和1,048,576条指令的协作分时边界精确截断，
中途fault只计已成功退休的前缀。单步与开启观测时仍逐条返回，FLAGS恢复规则不变。
GuestMem预先验证宿主页尺寸为2的幂，checked访问用shift定位页；标量访问先检查首个页，
跨页继续检查剩余所有页，写入继续递增代码代数。没有unchecked访存路径。
内存算术、其他窄寄存器和 CALL_NATIVE 使用64B预解码慢路径；SSE/x87/其他未覆盖指令用原解释器。

**解释器的512B vreg bank、完整 Semantic IR、所有类型的8B lowering、紧凑持久文件与通用 block chaining 还未实现。**
当前热子集借用现有 EAX…EDI 存储，不能将其包装成已经完成的64-vreg后端。
64B冷记录包含原始字节、PC、操作数和分析信息；磁盘与 resident footprint 仍偏大。
当前 dense PC 索引每个源字节占4B，另有 hot/cold records 和页代数；不能只报告8B热指令的尺寸。
本机目标的cache向量有效载荷约107,942,472B：冷记录69,373,632B、热条目17,343,408B、
dense索引12,552,192B、native target表8,671,704B、区域页代数1,536B。
不含allocator容量/元数据、GuestMem页表、guest commit页及临时输入buffer，不能当peak RSS。

本轮已扩大 typed memory 与顺序链接覆盖。下一步按真实热点扩展融合、通用 block chaining，
并设计紧凑热代码与必要 side tables、稀疏索引和完整 vreg bank。任何减少页代数检查的措施需另有正确性证明。
64B格式是可回归比较的过渡原型，不是最终小规模 IR。

## 验证与测量（2026-10-05）

`cargo test --release` 共71项通过：1 unit、34 differential、5 FPU、10 IR、1 Wasm、20 Win32。
34族差分同时比较 raw x86、IR 与 Unicorn；IR 测试检查静态 FLAGS 边界、预算/fault 恢复、
自修改/重叠入口、格式错误/EXE不匹配、SIB/FS/高字节以及 CALL_NATIVE/IAT hook。
本轮增加 typed store 对顺序链接的代码失效、拒绝记录不可经 successor 到达、
MOVZX窄访问在未commit边界上的精确宽度与FLAGS/fault恢复检查。
连续段额外验证每个预算位置、分支/fallback/fault退休计数与精确协作分时/保存FLAGS。
`cargo clippy --all-targets -- -D warnings` 与 format 检查通过。

合成测试 `cargo run --release --example ir_bench -- 10000000 [memory]`：整数60,000,003条、
SIB load/add/store循环50,000,004条 retired guest 指令。
关闭 trace/opcode profiling，无 API 调用；AOT/绑定不计入执行时间，交替运行3对，终态寄存器/FLAGS/PC/指令数严格相同。
访存循环还比较最终数据word。Apple M4（Mac16,12），macOS26.5.1；顺序测量，不与build/test并发。

| 循环 | 优化版raw x86 | 优化版IR guest MIPS |
| --- | --- | --- |
| ALU/branch | 43.49 / 35.30 / 36.40 | 66.99 / 61.56 / 55.17 |
| checked SIB load/add/store | 42.04 / 36.66 / 38.74 | 79.32 / 78.11 / 75.93 |

同批连续调度优化前的IR为ALU57.81/50.56/56.32、访存75.05/67.16/66.89。
最初typed-memory/顺序链接前的IR访存样本曾为63.64/63.34/63.26，第一阶段为90.30/90.40/87.74；
后续测试raw本身也显著下降，说明本机负载/调度存在大幅波动，不能跨批次把这些绝对值相减。
保留全部分阶段数据于 `wire-out/ir-performance/`。这些是合成循环，不能外推为游戏吞吐或100–200 MIPS承诺。
对照程序两种后端都正常退出，641条指令、43个API调用，输出同为21B `wire control program\n`；
IR命中638条，fallback3条，CALL_NATIVE7次。该程序过短，不能用它测性能。

真实目标完整目录下的对照及文件尺寸见本轮进度记录；当前仍停在 RtlUnwind。
游戏 `.wir` 有1,083,963条记录，其中562,602条使用8B热指令（原为320,634），文件69,373,688B，
说明下一版必须压缩持久记录、改进代码发现与索引，而不能宣称目标“IR规模小”已经达到。
