# 渐进重构实施记录

按用户提供的 2026-10-11 计划实施，原生 Mono/R6 忽略。用户要求直接在本地 main 分批提交；不创建 worktree，不 push。用户未跟踪设计文档、PDF、IDE 与系统文件保持现状。

## 2026-10-19 当前批次

最近完整性能验收累计 675 个 export。当前另外接入[五项 paint](refactoring-painting-followup-2026-10-19.md)与[两项控件初始化](refactoring-common-controls-2026-10-19.md)，完整正确性通过；已观察到 Unity/WindowServer 活动与同哈希显著漂移，性能单列待复核。没有撤回本轮正确性通过的结构改动，也没有把受干扰样本当作提升或回退结论。

[CLI 职责拆分重试](refactoring-cli-followup-2026-10-19.md)也已接入：main 保留参数收集/分发，四个命令模块完整保留原处理体，21 组命令与启动功能通过，library 源码保持。最新完整 release 631 / 7 ignored；同源 library 的 debug 477、native 7、工具 26 通过，静态检查和 build 通过。R2/R3 其余领域继续迁移，R6 仍忽略。

随后[十项 PE32 资源契约](refactoring-resources-2026-10-19.md)接入 typed 路由与显式资源借用，Win64 保持 absent。最新完整 release 636 / 7 ignored、debug 482、native 7、工具 26 及静态检查/build 通过，296 个 src Rust 文件与冻结核对一致，executable 较 CLI 减少 3,616 bytes。16 条功能协议、96 次执行、六对资源 trace 和一对 Alice 前缀通过；当前共 17 项新契约正确性通过、性能待复核，累计性能验收仍为 675。

资源检查中暴露的未知 PE32 case 静默生成 integer 问题已独立修正：所有 case 在 IO/启动前校验，合法 59 项三种规模的 177 份字节与预期保持。工具测试 28 项、实际 raw/IR 四次执行通过；运行时未改，证据 `r7-pe32-case-validation-*`，详见[工具边界](../architecture/validation-tools.md)。

再接入[五项双架构启动信息](refactoring-process-startup-2026-10-19.md)，独立基线 `99d280d`，保持命令地址、68/104 字节结构、feature 集合及不同返回帧规则。最新完整 release 640 / 7 ignored、debug 486、native 7、工具 28 和静态检查/build 通过，299 个 src Rust 文件核对一致；PE32 16 条/96 次、AMD64 4 条/24 次功能执行、六对 trace 及一对 Alice 前缀通过。当前共 22 项新契约正确性通过、性能待复核，累计性能验收保持 675。

随后[九项环境接口](refactoring-environment-2026-10-19.md)接入唯一契约，Win64 变量/环境块集合具有私有 owner，PE32 原 scratch 与 ABI-only 策略保持。独立基线 `2ee3a02`；完整 release 644 / 7 ignored、debug 490、native 7、工具 28 和静态检查/build 通过，302 个 src Rust 文件核对一致；120 次功能执行、六对 trace 和一对 Alice 前缀通过。当前共 31 项新契约正确性通过、性能待复核，累计性能验收仍为 675。

## 文件 section 批次（历史）

八项映射 API 汇入唯一文件契约，文件 export 累计 53 个；删除重复 ABI/arity/DWORD 表，按冷绑定 FileApi ID 使用原 handler，保留 Mappings 的资源寿命、错误顺序和输出提交。PE32 八项仍为 ABI-only；Win64 六项 typed、两项 absent。

原构建配置下 release 432、debug 280、native GPU 7 项及严格检查通过。冻结二进制与首轮完全相同，沿用 45 条路径并补足八项超阈值路径的顺序轮换长样本。raw 整数六组 CPU 中位数 +6.597%、control +2.319%、after/control +4.180%，raw bit after/control +3.215%；这些成本经调查后明确记录接受，新三组未全部超过 5% 拒绝门槛。其它最终路径最大回退 +2.089%，RSS 最多增加 192KiB。Alice 200M 三对保持同一预算出口和隐藏窗口，CPU 中位数 −1.00%。完整首轮拒绝、补样及接受依据见 [section 报告](refactoring-file-section-catalog-2026-10-15.md)。

独立 release 单编译单元实验因顺序平衡的三组 bit 长样本均回退超过 5% 而撤回，见 [构建候选报告](refactoring-release-codegen-2026-10-15.md)。此前路径/句柄、网关边界及 helper 布局候选也保持撤回；这些实验没有混入本批代码。

模块 API catalog 扩展前新增独立的 [IR 布局隔离](refactoring-ir-layout-2026-10-14.md)：仅改变 macOS 寄存器段执行器的节归属/对齐，26 个 case/path 三对验收，最大 CPU 中位数回退 +2.12%，release 403、debug 288、native GPU 7 项通过。原 module 候选因 bit-test 持续回退没有接受，后续以该独立批次为新基线继续验收。

后续 module 候选又暴露受检段的布局敏感性；在 `ff7495a` 上独立隔离 `CheckedSegment::execute` 并声明正确 Mach-O 代码属性。26 个 case/path 全部校验，memory +0.07%；SSE +3.63% 经 700M/IR 计数/机器码调查后记录接受。两项布局调整均没有更改 ISA/fault/预算行为，详情和失败轮次继续保存在同一报告。

31 个模块 export 的 catalog 已在新基线独立验收：release 406、debug 291、native GPU 7 项通过，26 个 case/path 的完整初筛/扩样结果校验通过。最终最大 CPU 中位数回退 +2.31%，RSS 最多增加 96KiB；Alice 200M 启动保持相同预算出口和隐藏窗口。静态/动态查询与已绑定 gateway 的差异、控制请求提交顺序及全部候选证据见 [模块 API 契约](refactoring-module-catalog-2026-10-14.md)。

2026-10-15 核心文件迁移新增两项独立 ABI/fault 基线，三版 catalog 候选因持续 PE32 IR 回退全部撤出；调查见 [文件 catalog](refactoring-file-catalog-2026-10-15.md)。随后单独隔离 PE32 五个实际执行边界：release 408、debug 293、native7 通过，28 个 case/path 及整数/位操作 700M 扩样全部校验，最终最大 CPU 中位数回退 +2.01%、RSS 最多增加 128KiB。SIMD 内层未生成独立节的旧候选与删除属性的重建结果都保留，最终改为外层 execute/回滚边界并重新验收。详见 [布局记录](refactoring-ir-layout-2026-10-14.md)。

后续核心文件契约以 `3dcd3c0` 布局为基线完成接入：11 个 export 统一 DLL/ABI/typed ID，Win64 的两个 absent export 保持；release 408、debug 293、native7、工具12 通过。28 个路径三对校验，两个 PE32 IR 路径经同二进制噪声与 utility 700M 调查，最终最大 CPU 中位数回退 +2.32%，RSS 最多增加 224KiB；Alice 200M 保持预算出口。全部失败、调度条件和最终证据见 [核心文件 catalog](refactoring-file-catalog-2026-10-15.md)。PE32 资源状态与其余文件家族仍待迁移。

路径/属性与元数据继续追加 15 个文件 export，合计 26 个；最新完整 release 415、debug 300、native7、工具12 通过。PE32 缓存 ABI-only 执行策略，公开条目改动仍重新绑定；34 个 case/path 三对校验，metadata IR/raw 中位数 −17.85%/−9.02%。AMD64 thread-id 700M 扩样保留 +4.10% 局部成本，其余最终路径最大回退 +2.72%，全部失败、污染和扩样轮次见 [元数据验收](refactoring-file-metadata-catalog-2026-10-15.md)。Alice 200M 保持隐藏窗口/预算出口，目录变更等剩余契约继续迁移。

目录/文件变更再追加 12 个 export，文件合计 38 个；完整 release 415、debug 300、native7、fmt/strict Clippy/build 通过。34 个 case/path 三对校验，事件 raw 210M 扩样未复现初筛回退，最终最大 CPU 中位数回退 +2.25%、RSS 中位数最多增加 96KiB。现有 CopyFile、PE32 ABI-only 与架构错误顺序保持，全部扩样和宿主条件见 [目录/变更验收](refactoring-file-mutation-catalog-2026-10-15.md)。

目录枚举再追加 7 个 export，文件合计 45 个；完整 release 417、debug 302、native7、fmt/strict Clippy/build 通过。首版的 AMD64 memory 成本未接受，最终以契约 ID 冷绑定原 route bit。37 条路径校验，整数补样/同二进制噪声、GUID/path 六对合并和 metadata 700M 后，最大 CPU 中位数回退 +2.79%、RSS 最多增加 32KiB；FindClose 固定负载 PE32 IR/raw、AMD64 分别 −16.23%/−12.73%/−12.45%。Alice 200M 保持隐藏窗口/预算出口，273 个构建输入 hash 核对一致，完整失败与最终证据见 [目录枚举验收](refactoring-file-search-catalog-2026-10-15.md)。

## R0 固定基线

- 代码：`2e78377`，tracked working tree 干净。
- 工具链：rustc 1.98.1 (`48a229cea`)，Cargo 1.98.1，aarch64-apple-darwin。
- before binary SHA-256：`b9c486ecde04a645dc5b3a4fb8b115641d1f4d7b9d115b8c822368885b224184`。
- 默认 release：382 passed / 0 failed / 7 ignored；测试名已保存，后续 harness 迁移逐名核对。
- 原创 V2 fixture 原文件 SHA-256：`8cea5037a73ded7c00f483244ae0eaed1dcad3830a7a6546c33e731f4dc23fff`；仅存在于 ignored audit 目录的生成器将在 R0 纳入版本控制。
- 本地输入与输出：`wire-out/refactoring-2026-10-14/`；只保存必要 executable、manifest、日志和原创小 fixture，不复制游戏素材。

已建立 [当前状态](../status.md)、[文档入口](../index.md) 和 [架构边界](../architecture/runtime.md)。历史报告保留原路径和当时结果。

## 阶段进度

| 阶段 | 状态 |
| --- | --- |
| R0 入口/fixture | 已完成；12 组 golden 输入一致，默认与旧路径各 7 个运行 smoke 通过 |
| R1 GUID 契约试点 | 已完成唯一契约、双架构冷绑定、纯语义/适配器、行为及性能验收 |
| R2 分发收敛 | 累计 675 个 export 已性能验收，另 7 项正确性通过、性能待复核；明细见当前状态与各家族报告；其余 USER/COM/GDI/D3D 等仍 legacy |
| R3 状态归属 | environment/module/Win64 file、local slots、已迁移同步/线程/COM/image 子领域与双架构 cursor 已形成 owner；其余资源继续分批收敛 |
| R4 PE32 Process | 已完成装载/执行编排提取；旧 Cpu 方法只保留委托 facade |
| R5 host/graphics | 已完成 owned UI 请求/事件与 GDI adapter 分离，真实 native 验证通过 |
| R6 原生 Mono | 按用户指示跳过 |
| R7 工具/结构 | CLI、PE32 内核、领域测试与公共工具按职责整理；CLI 性能待复核，后续领域继续迁移，旧入口和脚本兼容保留 |

## 测试 harness 提取

PE32 `Vm` 与固定内存布局移入 `tests/support/x86.rs`；PE64 原创 builder、startup 和 Win64 调用 frame helper 移入 `tests/support/x64.rs`。原 integration target 和测试函数名保持不变；全部 389 个测试名逐名一致。Win32/AMD64 239 项行为测试通过，两个 target 的 strict Clippy 和 fmt 通过。没有变更 runtime 代码。

## R2 Interlocked / SLIST 契约

以文件 section 契约 `619957f` 为基线接入六项契约，PE32 六项 Typed，Win64 仅 InitializeSListHead Typed。两端适配器只借用参数与内存；LONG 回绕、比较失败只读、SLIST 对齐与序列回绕、LastError 和返回槽别名的架构差异保持。独立行为基线先于迁移提交为 `90658e2`。

完整 release 432 / ignored7、debug 280、native7、fmt、strict Clippy 与工具12通过。25 条性能路径全部校验；SSE 的静态代码变化、硬件计数与全部六组计时保留。最终 SSE/IR +1.949%、integer/IR +2.732%，thread-ID 的 +4.557% 局部成本调查后明确接受。Win64 SLIST 固定负载 −27.837%，不推断整体游戏性能。Alice 200M 三对保持隐藏窗口和预算出口，详细结果见 [独立报告](refactoring-interlocked-catalog-2026-10-15.md)。

## R0 fixture 验收

原创生成器已进入 `tools/perf/fixtures/amd64_audit.py`，默认 V2 replay 不再需要 ignored audit 输入。旧 `--audit-probes PATH` 保留；两个入口的 7 个 input manifest 完全一致。`test_perf_fixtures.py` 对照迁移前固定的 12K/100M SHA-256 和结果，工具测试 6 项通过。源函数内容保持原样，没有修改任何 runtime 热路径。

## R1 契约与冷绑定

三个试点 API 的 DLL、scalar 参数、返回约定、路由 ID 来自 `win32::catalog`。PE32 仅 ole32，Win64 同时支持 combase，保持原范围。Win64 DWORD 索引由参数类型派生，PE32 stdcall 字节数由参数数派生；Wine spec 继续作为 legacy ABI 来源。动态 GetProcAddress 使用相同契约，kernel 模块不会错误导出试点。

PE32 公开 `ApiEntry`/`entries` 和 stub 地址保持兼容，内部 Arc 快照保存绑定信息。公开条目变化时重新绑定，未变调用不解析 catalog 或 Wine spec；handler 重入不会借用可变 API 表中的 entry。Win64 保留 HotImport，只给试点增加 typed ID。

独立 GUID 回归从接入前的 5 项扩展到 6 项，加上原 Win32/AMD64，共 245 项通过。覆盖 wrong DLL、signed capacity、高 DWORD 截断、HRESULT/访存差异、output alias return slot，以及公开条目/gateway 替换后的重绑定。全 target strict Clippy 通过。一次沙箱内 AMD64 测试因 Unicorn SIGILL 终止；在允许参照执行的环境重新运行，130 项全部通过，保留最终日志。

增加原创 `gateway-guid` fixture，expected 指令数/工作量/输出不依赖 catalog。绑定阶段的三对串行 AB/BA 使用 210M work；thread-id CPU 中位数 +2.65%，GUID +2.67%，RSS 略降。小样本和扩大样本均保留于 `guid-binding-timing*`；阶段结束还需完整矩阵，不据此宣称性能改善。固定的 `guid-bound-wire` 供后续定位代码布局影响。

## R1 纯语义与阶段验收

纯 GUID 布局/parser/formatter 移入 `win32/com/guid.rs`，包括 legacy allocator API 的格式化复用。两种架构的适配器只借用各自内存，不接触总 ApiState/Runtime64；PE32 HRESULT/LastError 与 Win64 fault/返回 frame 提交仍由 facade 负责。原 Win64 raw string transfer 原样提取为架构 codec，原 helper 委托，避免适配器反向依赖总运行状态。新增 API 操作说明见 [adding-api](../architecture/adding-api.md)。

- 完整 release：390 passed、0 failed、7 ignored；其中新增 6 个独立 GUID 边界测试和 2 个纯语义测试。7 ignored 的外部图形条件未改变，本阶段未修改图形语义。
- debug：6 个 GUID 边界 + 2 个纯语义；fmt、all-target/all-feature strict Clippy、6 个 Python 工具测试全部通过。
- 冻结 after SHA-256：`f458a17312da9340d0f56506f1a812e13966021b7e0018ff0ea1546fdb24780c`，路径 `guid-adapter-wire`。
- 25 个解释器 case/path，各三对串行 AB/BA，全部 work/retired/checksum 验证通过。AMD64 100M work 覆盖整数、访存、SSE、shift、REP、间接调用、large-code、两个 gateway；V2 覆盖 FP、bit、code/data alias/control；PE32 30M raw/IR 覆盖整数、SSE、FP、bit。CPU 中位数变化范围 -3.81% 至 +2.11%，无超过 3% 的回退；峰值 RSS 变化很小，仍保留每轮原始数据。
- V2 第一次命令误写了尚不存在的 case 名；已完成的 FP/bit 六轮结果保留在 `r1-v2-timing`，缺失的四个 alias/control case 在 `r1-v2-cache-timing` 补齐，没有丢弃失败调用或重复挑选结果。
- Alice 原始 guest Mono 启动前缀再执行三对 AB/BA，均到相同 200M work 预算边界，无新增 API/访存错误，Draw/Present 仍为 0；exe/UnityPlayer/guest Mono 输入 SHA 未变。CPU 中位数 before 2.75s、after 2.71s，wall 2.84s/2.82s，RSS 约 375MB。计时/线程导致 RIP 与退休数略有差异，不宣称逐指令相同或完整游戏 FPS 收益。复用原隔离目录，无复制游戏或素材；日志在 `r1-alice-prefix`。

以上完成首批六步，不代表 R2–R5/R7 全仓迁移完成。下一步审计 identity/environment 的 ABI 与实现范围，再按领域扩展。

## R2 identity 契约迁移

六个名字的 DLL、scalar 参数与 typed ID 汇入 `catalog/identity.rs`，删除旧 identity 的 arity/export 表、root dwords 推测、group bit 路由和 PE32 手写重复项。共享契约类型显式记录架构执行策略；`LegacyUnimplemented` 保留 PE32 已知 ABI 的诊断/零返回，Win64 使用 typed handler。历史 PE32 kernel32 GetUserName ABI 登记仍保留并写入限制说明，不在重构里暗改导出范围。

新增 PE32 基线在切换前后逐项验证 8 个 DLL/export 组合的栈清理、LastError、输出不变与错误 DLL。原 Win64 identity 测试已覆盖大小、域名、高位 DWORD、输出 fault 和动态 GetProcAddress。本批 300 项所属测试通过（lib 外部图形 7 ignored）、strict Clippy 与工具测试通过；catalog 增加 export/DLL 唯一性、参数索引和声明限制的自动检查。

三对 210M work 与 R1 after 对照：integer -0.32%、memory +2.13%、thread-id gateway +1.10%、GUID gateway +0.49%；无超过 3% 的 CPU 中位数回退。冻结 `identity-bound-wire`，日志在 `r2-identity-timing`。

补充 PE32 GUID gateway 独立 fixture，70M guest 指令、raw/IR 各三对，以最初基线对照：IR CPU 2.9755s → 2.2423s（-24.64%），raw 3.7758s → 3.0548s（-19.10%）。该 API 密集负载反映冷缓存 ABI、Arc 绑定快照与 typed 路由的共同影响，不推断游戏整体增益。API trace 仍格式化并计数，写入 null device，避免每轮生成数百万行日志；该策略在 manifest 中明确记录，默认工具行为不变。12K smoke 与 7M 小样本均保留，正式轮次位于 `r2-pe32-guid-timing-large`。

## R3 environment/identity 所有权

`runtime/environment.rs` 集中不可变默认身份、版本、codepage/locale 和用户路径。PE32 6.1.7601、Win64 10.0.19045 的版本差异保持；CRT version data export、版本 query/verify、默认 locale/codepage 和 Win64 用户目录使用该来源。算法中的显式 locale 映射和 ABI 布局仍留在各自 codec，不强行合并。

guest 环境变量由 `EnvironmentVariables` 私有 map 拥有，大小写归一化、原始拼写和 UTF-16 block 由该对象处理，原有枚举顺序/双 NUL 保持；架构 allocator 和环境 block 生命周期仍由 Runtime64 管理。Win64 identity adapter 移出 `impl Runtime64`，只借用内存、不可变描述和 LastError 槽地址，原 handler 删除。

完整 release 392 passed / 0 failed / 7 ignored，debug 两架构 246 项、fmt、strict Clippy 通过。profile 没有继承 host 用户或环境值。

性能先测 210M work 三对：integer -0.76%、thread-id gateway -2.56%、GUID +0.05% 左右，memory +3.32% 触发调查。扩展到 700M work 并加入原始冻结基线：memory 相对紧邻 R2 CPU 中位数 +3.73%，相对最初基线 -0.53%；全部三对均未超过 5%，RSS 无实质增长。保留 `r3-memory-investigation` 全轮次。诊断的 IR/fallback/translations/invalidation/epoch/cold counts 相同；RegisterSegment::execute_mixed 的 2925 行反汇编仅有代码/常量地址及重定位偏移差异，opcode/register/control 形态一致，未触碰 ISA/cache 源码。本次记录这项布局成本，不将阶段结果统一称为“无回退”。保留原结构候选，后续布局变化继续对照最初及相邻基线；若出现三对均超过 5% 则拒绝对应候选。

## R4 PE32 Process32

`x86/process.rs` 拥有 PE32 的装载、运行状态与执行编排；CLI 保留参数、IR 文件输入和报表。API trap、DLL thread notify、合作调度和 guest callback 从 Cpu 移入 Process，原 `Cpu::run/call_guest` 通过小 facade 委托。字段声明保持原 CLI 的资源析构顺序，原回调 frame、TEB/栈归属、FLAGS flush、预算及非局部转移顺序保持。段内批执行仍在 CPU 内核，没有增加 trait object 或逐指令字符串分发。

- 完整 release：392 passed / 0 failed / 7 ignored；debug IR/Win32/AMD64 270 项通过。现有 exact timeslice、嵌套 callback budget、nonlocal stack boundary、presentation-limit callback return，以及 DLL/TLS/SEH 回归保持。fmt、strict Clippy、6 项工具测试通过。
- 冻结 `process32-wire` SHA-256：`471cc661b4d94234d5af78127f88228f876ef5c812e9884c3f363f25e081989d`。
- 27 个 case/path：PE32 210M work raw/IR（整数、SSE、FP、bit、GUID），AMD64 100M work integer/memory/SSE/shift/REP/large-code/indirect-call/gateway，V2 FP/bit/code-data alias/control；全部三对，guest 结果与 work/retired 校验通过，RSS 没有实质增长。原始数据在 `r4-process-*-timing`。
- PE32 SSE raw 的初筛 +3.56% 扩到 700M work后为 +0.80%，相对最初基线 -0.86%；旧初始化中间版本的异常轮次也保留于 `r4-load-pe32-timing`，未挑选删除。
- AMD64 memory/large-code 初筛 +4.83%/+4.47%，扩到 700M 并加入最初基线：memory +3.45%（相对最初 +4.08%），large-code +2.20%（相对最初 +3.87%）。三对均未超过 5%；同一二进制重复对照的中位数差异为 -0.55%/+0.11%，故记录 memory 的实际代码生成/布局成本。IR=12008、fallback=0、translations=16、invalidations=0 和 cold/epoch 原因相同；CheckedSegment::execute 的 2156 行指令形态相同，地址/重定位变动，RegisterSegment::execute_mixed 则有 4 行额外生成指令及寄存器分配差异。ISA/cache 源码没有修改。按调查与拒绝门槛接受此候选，后续继续核对该负载；不将其表述为无回退。
- Alice 200M work 启动前缀三对均按相同预算退出，无新增 API/访存错误，未进入 Draw/Present。CPU 中位数 2.64s → 2.66s、wall 2.70s → 2.70s，RSS 约 375MB；线程/计时使 RIP 与 retired 略变。复用原隔离目录，没有复制素材，日志在 `r4-alice-prefix`。

过渡例外仍明确保留：领域 callback 通过旧 Cpu facade 重入 Process；IR 保留地址边界暂引用公开 `api::STUB_BASE`。后续将地址布局与领域控制请求逐步迁出兼容入口。R2/R3 的剩余领域和 R5/R7 未因此自动完成。

## R5 host / graphics

native UI FFI、MetalLayer retain/release 与 CPU frame presenter 移入 `host/ui.rs`。host 接收 owned window/frame 请求并产出普通事件，两架构 guest adapter 分别负责窗口选择/ID 映射和消息入队。PE32 音频、16ms gate、native window sync、事件消费、frame sequence、snapshot/failure、上传及 capture 的顺序保持。Presenter 明确受主线程约束，Frame 在 FFI 前验证尺寸与字节数；旧 `platform::Presenter` 作为公开 facade 保留。

GDI 的 DLL 接受范围、USER client dimensions、LastError 和 diagnostics 移入 `win32/gdi/x86.rs`；图形对象、DIB/font 和 checked raster operations 不再借用 ApiState。PE32 GuestMem codec 和资源上限保留，没有借此次迁移更换 handle arena 或合并两架构内存。native C/ObjC、build.rs、include 和第三方库路径未改变。完整契约见 [host/graphics](../architecture/host-graphics.md)。

- 默认完整 release：394 passed / 0 failed / 7 ignored；新增 2 项 host 边界测试验证 malformed frame、RGB 字节、去重和 16 帧限额。debug Win32/AMD64 + host 共 241 项、fmt、strict Clippy、6 项工具检查通过。
- 提供现有 native graphics/MoltenVK/原创 GS 和隔离 DXBC 后，全部 7 项 ignored 显式运行通过（Metal/software 像素对照、GPU shader/resource owner/lifetime、DXGI draw/swapchain）。输入 SHA 保存为 `r5-native-input-sha256.txt`；没有复制游戏或构建 Mono。
- 新 `examples/host_ui_probe.rs` 在真实 AppKit 主线程验证非主线程初始化拒绝、窗口 resize/remove、上传成功与 failure、RGB capture 和去重；通过日志为 `r5-host-ui-probe.txt`。探针仅使用自己生成的两像素帧。
- `host-graphics-wire` SHA-256：`d72a5cf8f35e0453f137e677eac49a11b7ef7c36cca0471058c4a7481fd9f41f`。
- 29 个 case/path、100M work、各三对 AB/BA，所有结果校验。新增 PE32 GDI fixture 以 CreateCompatibleDC → GetPixel(0,0) 黑色 checksum → DeleteDC 独立验证，raw/IR CPU 中位数变化 -0.53%/+0.65%；trace 仍格式化/计数，写 null device 以限制日志体积。
- thread-ID gateway 初筛 +4.12% 扩到 700M 后为 +2.66%，相对最初 +0.95%；其余初筛回退均小于 3%。memory 扩样相对 R4 -6.04%、相对最初 -2.94%，R4 记录的布局成本已消退。RSS 没有实质增长，全部初筛与扩大轮次保留于 `r5-host-*-timing` / `r5-host-gateway-memory-investigation`。
- Alice 窗口模式 200M 启动前缀三对到相同 budget，guest bootstrap HWND 仍隐藏、client=314x211、无新 API/访存错误，没有提前进入 Draw/Present。复用原隔离目录，日志 `r5-alice-window-prefix`；实际渲染正确性由上述真实 native 探针和 GPU/像素 fixture 验证。

R5 没有把 D3D9/DXGI 的所有 guest COM 语义都迁入新 catalog；它们按 R2/R3 继续逐领域处理。当前完整重构仍有剩余家族和 R7 收尾，R6 按用户要求跳过。


## R2/R3 version 家族

八个版本 API 的 provider/签名/执行策略集中到 catalog；PE32 immutable binding 从 GUID 专属字段扩展为 typed ID，Win64 复用原 BoundImport。删除两端的重复 ABI、arity、DWORD、支持条件与旧 version handler。PE32 ABI-only 与 unknown 条目保持原策略，没有扩展 API 能力。

适配器仅借用内存、GuestEnvironment 与错误槽；纯层级比较迁到 kernel/version，保留 Wine LGPL 来源和版权声明。查询 size、LastError、suite/product 输出、NTSTATUS、return-slot alias 及完整结构先读后验证保持。完整契约见 [version](../architecture/version.md)。

- 新独立基线 3 项通过；完整 release 397 passed / 0 failed / 7 ignored，debug version/GUID/identity/Win32/AMD64 249 项通过，fmt、strict Clippy、工具 6 项通过。R5 的 native 7 项结果仍按原基线记录。
- 冻结 `version-wire` SHA-256：`a3756a2026e491d55eb4d26b8dea2df1022dc081fafdff042543590897aa750f`。
- 11 个 case/path，100M work、三对 AB/BA；整数、SSE、memory、thread-ID 和两架构 version gateway 均校验 work/retired/checksum。version gateway PE32 IR/raw +0.52%/+1.59%，Win64 -0.19%，其它初筛除 PE32 SSE raw 外回退均低于 2%。
- PE32 SSE raw 初筛 +4.39% 触发扩样；700M work 后相对 R5 +2.50%，相对最初基线 -6.72%，各对回退 +2.10%/+2.74%/+3.72%，没有三对均超过 5%。RSS 无实质增长。记录该布局成本，全部初筛与扩大轮次位于 `r2-version-*-timing` 和 `r2-version-sse-investigation`；未改 ISA/cache 热循环。


## R2/R3 NLS / locale 家族

42 个原有 export 集中到 catalog（PE32 20 个 typed、22 个 ABI-only；Win64 28 个 typed、14 个 absent）。删除两端的重复 ABI/arity/DWORD、动态支持列表与旧 NLS handler。公开 nls::dispatch 保留原函数范围并委托新适配器，main gateway 直接消费冷绑定 ID。Wine spec 继续是未迁移 API 的 ABI 来源。

locale 词汇、格式化、C1 字符分类和 calendar 算术形成纯核；宿主 localtime provider 进入 host。Win64 locale 枚举使用冷绑定 ControlImport，进程框架保留线程/executor、callback frame、allocation/release、CPU/trace 提交与 budget/stop-on-false 顺序，adapter 只填充回调输入。详细 flags、部分输出、size/encoding/fault 差异见 [NLS](../architecture/nls.md)。

- 迁移前新增 3 个独立行为基线，迁移后全部通过；完整 release 400 passed / 0 failed / 7 ignored，debug 252 项通过。fmt、strict Clippy、工具 6 项通过。迁移中的 helper 漏调用和常量 cast 检查失败日志保留，修正后的全量验证单独记录。
- 冻结 `nls-wire` SHA-256：`39ef6903fdf338d13bf81596a590756b482a0e9dd5f1398b4fcbeff0c2ec9982`。
- 新 `gateway-nls` 和 `nls-conversion` 原创 fixture 在前后两版先做低 work 功能预检；正式 14 个 case/path 使用 100M work、三对 AB/BA，全部 work/retired/checksum/API count 校验。查询 fixture 累加固定 CP932 返回值；转换 fixture 同时校验 UTF-16 输出和返回 count。默认 case 集合保持原样。
- 相对 version，Win64 查询/转换 CPU 中位数 -33.08%/-11.31%，PE32 相应 raw/IR -0.81% 至 -2.11%；普通整数/SSE/thread-ID 初筛回退均低于 1%。这些是确定 fixture 的变化。
- AMD64 memory 初筛 +3.08% 触发 700M 扩样：相对 version +3.14%、相对最初基线 +1.80%，各对约 +3.07%/+3.39%/+2.72%，RSS 无实质增长。独立 diagnostics 的 IR=12008、fallback=0、translations=16、invalidations=0，以及 cold/epoch/byte/mapping 原因一致。CheckedSegment 与 RegisterSegment 各为 2156/2925 条指令，opcode 序列一致；地址及常量重定位变化，原始反汇编保留为 `r2-nls-*-segment-*.txt`。ISA/cache 源码未改，按调查门槛接受并记录这项成本，未发生三对均回退超过 5%。全部轮次在 `r2-nls-*-timing`、`r2-nls-memory-investigation`，诊断分开运行。

## R2 路径辅助与通用句柄

在 section/Interlocked 基线上接入 13 项 PathApi 和六项 HandleApi，删除重复 ABI 与名称路由。PE32 路径适配器用互斥 Core/Helper ID；Win64 句柄仍由原资源 owner 处理，保留保护位、错误顺序、提前返回与 unblock。完整 release 432、debug 280、外部 native 7 项、fmt、strict Clippy 通过，新增时钟草稿尚未注册。

46 条性能路径全部结果校验；三项短样本扩为三标签长样本后，SSE/raw +1.554%、module/raw +1.679%、section/IR +3.078%。section/IR 的相同计数诊断和 +3.178% after/control 成本明确保留；没有三组均超过 5%。RSS 中位数最多 +96KiB，Alice 200M 三对保持预算出口、隐藏窗口和零 Draw/Present。冻结 SHA、历史失败候选、全部轮次与接受决定见 [路径与句柄验收](refactoring-paths-handles-2026-10-15.md)。R7 随后增加离线性能汇总工具，16 项工具测试通过，不改变 runtime 或 fixture。

## R2 时钟契约

六项时钟 API 的 provider、scalar ABI 和 typed ID 汇入 catalog，删除旧表及字符串 handler；Win64 冷绑定原热路由，输出/frame、PE32 EDX:EAX 和 timeGetTime lookup 差异保持。release 437、debug 285、外部 native 7、工具 19 项及 fmt/strict Clippy 通过。

旧协议出现相同哈希副本仍有明显差异后，独立引入可选稳定启动协议，保留所有失败数据。新协议 49 条路径全部校验；bit/path-helper IR 的 700M 顺序轮换为 +0.586%/−1.201%，其余最大回退 +2.776%，RSS 最多 +144KiB。Alice 200M 三对保持原预算出口、隐藏窗口和零 Draw/Present。完整过程、体积与哈希见 [时钟验收](refactoring-clocks-2026-10-15.md)。R6/Mono 继续跳过。

## R2 进程身份与 LastError

七项 API 收敛到 ContextApi，删除重复 ABI/arity/DWORD 和名称 handler。原 scheduler、TEB/PEB 和进程状态保持，Win64 按 typed ID 冷绑定六个原热路由。独立旧行为基线先通过，再验证切换后的 release 442、debug 290、native 7、工具 19 项及 fmt/strict Clippy。

25 条性能路径全部校验；SSE 700M 初筛 +3.006% 后扩为 2.1B 三标签轮换，仍有 +2.358% 局部成本，其余最大回退 +2.505%，RSS 最多 +160KiB。Alice 200M 三对保持预算出口、隐藏窗口和零 Draw/Present；228 个已注册 Rust 文件与冻结 SHA 一致。全部轮次和接受决定见 [上下文验收](refactoring-context-2026-10-16.md)。

## R2 TLS/FLS

八项 API 迁入 LocalApi，删除重复 ABI/arity/DWORD、名称 handler 及 fresh-provider 的 FLS/timeGetTime 名单。Win64 保留三个 TLS hot body 与原 TEB/slot 资源，general 路由按 typed ID 执行；PE32 保留线程 map 与各自的 error/callback 策略。六项旧行为基线先通过，完整 release 448、debug 296、native 7、工具 19 项及 fmt/strict Clippy 通过。

28 条性能路径全部校验。memory/data-alias 的 2.1B/900M 扩样仍有 +3.511%/+3.071% 局部成本；独立 IR/cache 计数一致，静态段助记符序列一致，其它路径最大回退 +1.884%，RSS 最多 +208KiB。Alice 200M 三对保持预算出口、隐藏窗口和零 Draw/Present；232 个已注册 Rust 文件与冻结 SHA 一致。接受决定和全部失败/扩样数据见 [TLS/FLS 验收](refactoring-locals-2026-10-16.md)。状态 owner 下一批独立处理。

## R3 PE32 TLS/FLS 状态

两张 `(TID, slot)` map 与两个递增计数器收进私有 LocalSlots32，只提供 allocate/get/set/free；adapter 保留 TID、LastError、callback 说明、trace 及 frame 提交，不新增资源限制或锁。Win64 状态保持。完整 release 448、debug 296、native 7、工具 19 项及 fmt/strict Clippy 通过。

28 条性能路径全部校验；context/IR 初筛 +3.673% 扩为 150M 三标签轮换后为 −1.542%，同哈希对照 +3.201% 和明显离散轮次全部保留。其余最大回退 +2.715%，RSS 最多 +80KiB。Alice 200M 三对保持预算出口、隐藏窗口和零 Draw/Present；冻结的 233 个已注册 Rust 文件核对一致。完整接受决定见 [状态拆分验收](refactoring-locals32-owner-2026-10-16.md)。

## R2 事件与信号量

16 项接口接入 SyncObjectApi，删除重复 ABI/arity/DWORD 与字符串 handler；PE32 十项 typed、六项 ABI-only，Win64 十五项 typed、PulseEvent absent。原命名对象、per-alias access、count、waiter wake 和故障提交顺序保持。四项旧行为基线先通过，完整 release 452、debug 300、native 7、工具 19 项与 fmt/strict Clippy 通过。

31 条性能路径全部校验；module/raw 初筛 +3.743% 经 150M 三标签扩样为 +0.527%，control/before +0.070%。Win64 固定 event 网关循环 −21.989%，其余最大回退 +2.725%，RSS 最多 +256KiB。Alice 200M 三对保持预算出口、隐藏窗口和零 Draw/Present，CPU +0.306%；冻结的 235 个 Rust 文件核对一致。完整接受决定见 [同步对象验收](refactoring-sync-objects-2026-10-16.md)。

## R2 Mutex

七项接口接入 MutexApi，删除 PE32 重复 ABI 与名称 handler；三项 typed、四项 ABI-only，Win64 全部 absent。实际 guest 线程的旧行为基线覆盖递归所有权、同名 create、逐层释放、error 288 和 release 在返回槽 fault 前提交。完整 release 454、debug 302、native 7、工具 19 项及 fmt/strict Clippy 通过。

33 条性能路径全部结果校验；最大回退 +1.712%，mutex IR/raw +0.714%/+0.681%，RSS 最多 +64KiB。Alice 200M 三对保持预算出口、隐藏窗口和零 Draw/Present，CPU +2.204%、wall +2.873%。236 个冻结 Rust 文件核对一致；下一组 Wait 草稿未混入验收。独立发现的 event fixture 重复导入保留原数据，后续单独修正。完整接受决定见 [Mutex 验收](refactoring-mutexes-2026-10-16.md)。

## R2 WaitFor* 进程控制

四项接口接入 WaitApi，删除重复 ABI/arity/DWORD 与名称路由。Win64 冷绑定 ControlImport 保留原 saved return frame、APC、deadline、block/unblock 和 trace/counter 顺序，PE32 保留空数组/count clamp、SingleEx ignored alertable 及 MultipleEx ABI-only。三项独立旧行为基线与三种实际 guest APC 等待路径先通过，完整 release 457、debug 305、native 7、工具 20 项及 fmt/strict Clippy 通过。

36 条性能路径全部校验；local/raw 初筛 +5.031% 经 150M 三标签轮换为 +0.376%，同哈希 control/before +0.764%，离散首轮全部保留。其它最大回退 +2.662%，RSS 最多 +112KiB；Wait 网关 Win64 −2.072%，PE32 IR/raw −1.941%/−0.219%。Alice 200M 三对保持预算出口、隐藏窗口和零 Draw/Present，CPU −0.631%。245 个 manifest Rust 文件（src/tests 237 个）核对一致，后续 Delay 基线未混入候选。完整接受决定见 [WaitFor* 验收](refactoring-waits-2026-10-16.md)。

## R2 Sleep 与线程让出

三项接口接入 DelayApi，删除重复 ABI/arity/DWORD 和名称分支。Win64 冷绑定 ControlImport 保留 Returned/Yielded/SwitchThread/Blocked 的原控制结果，PE32 保留 wake/deadline、ignored alertable 与返回帧后 round-robin；trace/counter 和 fault 顺序保持。两项旧行为基线、实际线程让出/APC/timeout 先通过，完整 release 459、debug 307、native 7、工具 20 项及 fmt/strict Clippy 通过。

24 条相关性能路径全部校验，最大回退 +1.063%，RSS 最多 +16KiB；Delay 网关 Win64 −0.717%，PE32 IR/raw −3.387%/−1.001%。Alice 200M 三对保持预算出口、隐藏窗口和零 Draw/Present，CPU +0.325%。246 个 manifest Rust 文件（src/tests 238 个）核对一致；体积增加 704 bytes。完整接受决定见 [Sleep/线程让出验收](refactoring-delays-2026-10-16.md)。

## R2 Waitable timer

九项接口接入 TimerApi，PE32 全部 ABI-only、Win64 typed；删除 timer arity/DWORD、group 15、名称解析和 GetProcAddress 特例，保留 Events owner 及原 access/deadline/故障顺序。两项独立基线先通过，35 项同步/12 项模块及完整 release 461、debug 309、native 7、工具 20 项通过。残留动态查询引用和 unused import 的检查失败日志保留，修正后 fmt/strict Clippy 与 build 通过。

25 条性能路径全部校验，最大回退 +2.525%，RSS 最多 +128KiB，Win64 取消网关 −18.748%。Alice 200M 三对保持预算出口、隐藏窗口和零 Draw/Present，CPU +0.270%。247 个 manifest Rust 文件（src/tests 239 个）核对一致，体积增加 368 bytes，后续虚拟内存草稿未混入。完整接受决定见 [Timer 验收](refactoring-timers-2026-10-16.md)。

## R2 虚拟内存

八项 VirtualAlloc/Free/Protect/Query 及 Ex 接入 VirtualMemoryApi，删除重复 ABI/arity/DWORD 和名称/后缀分发。两架构适配器只借内存、参数与错误槽，保持原 SIZE_T、handle、错误/查询布局、输出/返回帧顺序及内存/缓存 owner。两项独立旧行为基线及 16 项领域回归通过，完整 release 463、debug 311、native 7、工具 20 项和 fmt/strict Clippy 通过。编译导入路径与 GUID legacy 标记的失败记录及修正均保留。

28 条初筛路径全部校验，三项超门槛后再测长样本三标签对照；delay −1.227%、local/IR +1.569%，integer/IR +3.548% 而同哈希 control +4.749%，按调查后门槛接受并保留残留差异。其余初筛最大回退 +2.679%，RSS 最多 +96KiB。Alice 前缀 CPU +0.771%，保持隐藏窗口和零 Draw/Present；249 个 Rust 文件哈希核对，体积减少 2,000 bytes。完整结果见 [虚拟内存验收](refactoring-virtual-memory-2026-10-16.md)。

## R2 Heap

13 项 heap export 接入 HeapApi，删除重复 ABI/arity/DWORD 及根名称分支，PE32 adapter 只借原 GuestHeap、内存、进程堆 handle 和 LastError；Win64 原 slab/block helper 改按 ID 分发。保留 PE32 九项 typed/四项 ABI-only、Win64 五项 typed/其余 absent，以及错误、归属、枚举输出和返回帧顺序。两项独立旧行为基线与八项领域回归通过，完整 release 465、debug 313、native 7、工具 20 项和 fmt/strict Clippy 通过。

28 条初筛路径全部校验，FP/IR 初筛 +3.736% 经 900M 三标签扩样为 +0.265%、同哈希 control −0.046%；其余最大回退 +2.181%，RSS 最多 +128KiB。HeapSize 网关 Win64 −25.973%、PE32 IR/raw −2.316%/−2.573%；Alice 前缀 CPU −0.323%，保持隐藏窗口和零 Draw/Present。251 个 Rust 文件哈希核对，体积增加 1,840 bytes，后续 Local/Global 草稿未混入。完整接受决定见 [Heap 验收](refactoring-heaps-2026-10-16.md)。

## R2 Local/Global 内存

14 项 export 接入七个 LocalMemoryApi ID，删除 PE32 十项重复 ABI、Win64 arity/DWORD 及名称/后缀分发；保留 PE32 进程 GuestHeap 共享与四项 ABI-only、Win64 local block 归属及错误/返回帧顺序。两项独立旧行为基线和 20 项内存回归先通过，完整 release 467、debug 315、native 7、工具 20 项及 fmt/strict Clippy 通过。

28 条性能路径全部结果校验，最大 CPU 回退 +2.979%，RSS 最多 +112KiB；LocalAlloc/GlobalSize 网关 Win64 −2.624%、PE32 IR/raw +0.555%/−2.166%。Alice 前缀 CPU −3.619%，保持隐藏窗口和零 Draw/Present；254 个 Rust 文件哈希核对，体积增加 16,992 bytes，后续系统信息基线未混入。完整接受决定见 [Local/Global 验收](refactoring-local-memory-2026-10-16.md)。

## R2 系统信息与内存状态

四项 export 接入三个 SystemProfileApi ID，删除重复 ABI/arity 和名称分发；双架构 adapter 只借内存和错误槽，保留硬件布局、内存记账、public facade 与各自返回帧顺序。独立旧行为基线先通过，完整 release 471、debug 319、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

首版 Local/Global IR 存在 +7.747% 回退，拒绝后将新增 guard 追加到已有 scalar typed 路由之后；三顺序同哈希复测为 −1.118%。第二版完整 31 条路径全部校验，最大回退 +2.602%，RSS 最多 +80KiB；GetSystemInfo 网关 Win64 −14.393%、PE32 IR/raw −19.368%/−17.740%。Alice 前缀 CPU −0.740%，保持隐藏窗口和零 Draw/Present；257 个 Rust 文件哈希核对，体积增加 400 bytes，Calendar 草稿未混入。完整接受决定见 [系统信息验收](refactoring-system-profile-2026-10-16.md)。

## R2 日历、时区与 CPU 计时

11 项 export 接入 CalendarApi，移除重复 ABI/arity 和名称分发；PE32 三项 typed、八项 ABI-only，Win64 十项 typed。时区转换脱离 Runtime64，宿主 CPU 计时迁至 host；两个 public facade 保留原范围，校验/DST/错误及返回帧顺序保持。七项独立旧行为回归先通过，完整 release 471、debug 319、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

32 条性能路径全部校验，日历转换网关 −14.970%，RSS 最多 +208KiB。三个 Win64 网关初筛超过 3%，同哈希三顺序扩样后为 +1.647%/+4.570%/+2.630%；记录 Local/Global 相对 control +3.018% 的局部成本，IR/cache 计数与专用段助记符序列保持。Alice 前缀 CPU −0.121%，保持隐藏窗口和零 Draw/Present；261 个 Rust 文件核对一致，体积增加 3,808 bytes，后续多媒体草稿未混入。接受决定和完整调查见[日历验收](refactoring-calendar-2026-10-16.md)。

## R2/R3 多媒体计时器

三项 export 接入 MultimediaApi，删除重复 ABI/arity/DWORD、provider 与名称分发；Win64 period map 收进私有 PeriodRequests owner，PE32 保留 stateless period 策略。public facade 精确范围、Win64 静态/动态 provider 差异、容量/引用与输出/返回帧顺序保持。五项独立旧行为回归先通过，完整 release 473、debug 321、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

35 条性能路径全部校验，timeGetDevCaps 网关 Win64 −11.752%、PE32 IR/raw −1.193%/−2.279%，初筛 RSS 最多 +16KiB。四条 PE32 IR 初筛超过 3%，三顺序同哈希扩样后为 −0.192%/+0.737%/+0.210%/+1.106%，其余最大回退 +2.395%。Alice 前缀 CPU −0.331%，保持隐藏窗口和零 Draw/Present；265 个 Rust 文件哈希一致，体积增加 4,064 bytes。接受决定与全部初筛数据见[多媒体验收](refactoring-multimedia-2026-10-16.md)。

## R2/R3 SRW、条件变量与 InitOnce

十二项 export 接入 InlineSyncApi，删除重复 arity/DWORD 与名称/前后缀分发；Win64 SRW map 收进私有 owner，adapter 成功写 guest token 后提交 owner。显式保留完成、Try失败和两种阻塞 outcome，进程保留 CPU、调度与 trace/API count 顺序；PE32 保持 ABI-only。完整 release 479、debug 326、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

36 条性能路径全部校验，SRW 网关 CPU −0.570%，最大回退 +1.837%，RSS 最多增加 176KiB；Alice 前缀 CPU −1.825%，保持隐藏窗口和零 Draw/Present。268 个 Rust 文件核对一致，体积增加 6,128 bytes，契约累计 293 个 export。完整接受决定见[内联同步验收](refactoring-inline-sync-2026-10-16.md)。

## R2 Critical section

八项 export 接入 CriticalSectionApi，删除重复 ABI/arity/DWORD 和根名称分支；双架构 adapter 只借内存和 tid，保留 24/40 字节布局、逐字段部分故障、权限/flags/归属、返回帧与调度顺序。八项独立领域回归及完整 release 486、debug 333、native 7、工具 20 项和 fmt/strict Clippy/build 通过。

39 条性能路径全部校验，锁网关 CPU Win64 −3.853%、PE32 IR/raw −1.418%/−0.813%，最大回退 +2.234%，RSS 最多增加 128KiB；Alice 前缀 CPU −0.149%，保持隐藏窗口和零 Draw/Present。273 个 Rust 文件核对一致，体积减少 976 bytes，契约累计 301 个 export。2026-10-17 接受，完整记录见[critical section 验收](refactoring-critical-sections-2026-10-16.md)。

## R2/R3 地址等待

三项 export 接入 AddressWaitApi，删除根 provider 特例、arity/DWORD 与名称分发；PE32 保持 unknown ABI 和零参数清栈。地址队列、私有序号及阻塞注册由 Threads owner 管理，helper 脱离总 Runtime64，CPU/公共返回仍在进程层。四项领域回归及完整 release 488、debug 335、native 7、工具 20 项和 fmt/strict Clippy/build 通过。

40 条性能路径全部校验，新网关 CPU −0.351%，最大回退 +1.907%，RSS 最多增加 80KiB；Alice 前缀 CPU −0.408%、wall +0.460%，保持隐藏窗口和零 Draw/Present。274 个 Rust 文件核对一致，体积增加 1,904 bytes，契约累计 304 个 export。2026-10-17 接受，完整记录见[地址等待验收](refactoring-address-waits-2026-10-17.md)。

## R2/R3 线程查询与设置

九项 export 接入 ThreadPropertyApi，删除重复 ABI/arity/DWORD、动态查询特例与根名称分发。PE32 六项 typed、三项 ABI-only，adapter 只借内存、Sched 与 LastError；挂起状态切换和 exit 读取由 Sched 管理。Win64 scalar 操作归 Threads owner，error mode 字段私有，CPU/返回/unblock/trace 保持进程提交；创建、退出和 APC/CONTEXT/description 保留专用边界。六项独立旧行为回归及完整 release 494、debug 341、native 7、工具 20 项和 fmt/strict Clippy/build 通过。

43 条初筛路径全部结果校验，四项超门槛后同哈希三顺序扩样；PE32 HeapSize/raw +1.938%、FP/raw −3.254%、线程网关/IR +1.148%，Win64 HeapSize 保留 +4.063% 局部成本（after/control +3.763%）。IR/cache 计数与执行器助记符序列保持，具体成本机制尚未确证；新 Win64 线程网关 −6.625%，初筛 RSS 最多 +112KiB。Alice 前缀 CPU −0.544%、wall −0.043%，保持隐藏窗口和零 Draw/Present；279 个 Rust 文件核对一致，二进制减少 24,496 bytes，契约累计 313 个 export。2026-10-17 接受，完整决定与全部轮次见[线程查询与设置验收](refactoring-thread-properties-2026-10-17.md)。

## R2/R3 线程控制

六项 export 接入 ThreadControlApi，删除重复 ABI/arity/DWORD 和根名称分支；PE32 Create/Exit typed，其余四项 ABI-only，内部 wire worker-return 单独保留。Win64 冷绑定专用控制 ID，Value/Exit/Context 保留原 CPU/计数/trace/阻塞提交；Threads 私有拥有描述/APC queue，创建/TLS/heap/回调仍按明确资源边界编排。八项独立旧行为基线及完整 release 502、debug 349、native 7、工具 20 项和 fmt/strict Clippy/build 通过。

46 条初筛路径全部校验，两条 SSE 同哈希三顺序扩样；PE32 SSE/raw +0.683%，Win64 SSE 保留 +3.473% 局部成本（after/control +3.702%），诊断计数与执行器助记符序列保持，具体成本机制尚未确证。新 APC 错误返回网关 CPU Win64 +0.489%、PE32 IR/raw −23.845%/−19.707%；初筛 RSS 最多 +160KiB。Alice 前缀 CPU −0.949%、wall −0.952%，保持隐藏窗口和零 Draw/Present。283 个 Rust 文件核对一致，二进制增加 22,688 bytes，契约累计 319 个 export。2026-10-17 接受，完整初筛、扩样和历史成本保留于[线程控制验收](refactoring-thread-control-2026-10-17.md)。

## R2/R3 COM 基础

十项 export 接入 ComApi，删除重复 ABI/arity/DWORD、provider 特例和根名称分发，保留 PE32 Ole ABI-only、Ro absent 及 Win64 Ole absent。双架构 Apartment 由线程独占，删除 Win64 活动镜像；TaskMemory 私有登记成员，原 backing allocator、PE32 进程 heap 共享、WMI 工厂和公开 NLS facade 精确范围保持。七项独立回归及完整 release 509、debug 356、native 7、工具 20 项和 fmt/strict Clippy/build 通过。

49 条初筛路径全部校验，四项单轮离散路径同哈希三顺序扩样，共 53 个 workload protocol；初筛最大 CPU 回退 +2.129%，扩样最大 +2.367%。新 COM 网关 CPU Win64 −12.916%、PE32 IR/raw −2.754%/−2.063%；初筛 RSS 最多 +64KiB。Alice 前缀 CPU +0.024%、wall +0.229%，保持隐藏窗口和零 Draw/Present。289 个 Rust 文件核对一致，二进制增加 1,216 bytes，契约累计 329 个 export。2026-10-17 接受，初筛、扩样和历史成本见[COM 基础验收](refactoring-com-core-2026-10-17.md)。

## R2/R3 WinRT 字符串与激活

十一项 export 接入 WinrtApi，删除重复 provider/arity/DWORD、legacy bit 路由与执行时名称比较，保持动态 API set 到 combase 的旧 remapping 和 PE32 unknown ABI。Strings 私有拥有成员、长度、引用数与空 buffer 身份；header 重读与释放只借 Memory/backing map，分配仍由 adapter 协调。五项独立回归及完整 release 514、debug 361、native 7、工具 20 项和 fmt/strict Clippy/build 通过。

50 条初筛路径全部校验，两项离散路径同哈希三顺序扩样，共 52 个 workload protocol；初筛最大 CPU 回退 +1.860%，扩样 +0.082%/−0.210%。新字符串网关 CPU −2.674%，初筛 RSS 最多 +64KiB。Alice 前缀 CPU −0.840%、wall −0.895%，保持隐藏窗口和零 Draw/Present。293 个 Rust 文件核对一致，二进制增加 480 bytes，契约累计 340 个 export。2026-10-17 接受，完整轮次和历史成本见[WinRT 验收](refactoring-winrt-2026-10-17.md)。

## R2/R3 BSTR

八项 export 接入 BstrApi，删除重复 provider/arity/DWORD 与名称分发，Win64 ordinal alias 收入契约模块。双架构资源记录由私有 owner 管理，Variant/WMI 内部 facade 共用原分配入口，保留 heap release、别名重分配及返回/错误差异。五项独立回归及完整 release 519、debug 366、native 7、工具 20 项和 fmt/strict Clippy/build 通过。

53 条初筛路径全部校验，三项同哈希三顺序扩样，共 56 个 workload protocol。SSE/raw 初筛 +3.715%、扩样保留 +2.871% 局部成本（after/control +2.465%），整数 IR/raw 扩样 +0.003%/+1.168%。新查询网关 CPU Win64 −1.027%、PE32 IR/raw −1.331%/−1.912%；初筛 RSS 最多 +16KiB。Alice 前缀 CPU +0.028%、wall −0.325%，保持隐藏窗口和零 Draw/Present。300 个 Rust 文件核对一致，二进制增加 3,664 bytes，契约累计 348 个 export。2026-10-17 接受，完整轮次及历史成本见[BSTR 验收](refactoring-bstr-2026-10-17.md)。

## R2 Variant

四项 PE32 oleaut32 export 接入 VariantApi，移除 common_api 的重复 admission 与名称实现。适配层只借明确资源，通过 Bstrings32 共享 owner 执行深拷贝、释放与失败回滚，保留 scalar 字节、Clear tag、BYREF 生命周期、目标低 WORD、HRESULT/LastError 和返回顺序；Win64 保持 absent。四项独立旧行为基线及完整 release 523、debug 370、native 7、工具 20 项和 fmt/strict Clippy/build 通过。

55 条初筛路径全部校验，五项同哈希三顺序扩样，共 60 个 workload protocol；初筛 CPU 中位数最大回退 +2.137%，扩样最大 +0.625%。新网关 CPU PE32 IR/raw −0.843%/−2.248%，初筛 RSS 最多 +320KiB；Alice 前缀 CPU +0.051%、wall −0.044%，保持隐藏窗口和零 Draw/Present。304 个 Rust 文件核对一致，二进制减少 78,944 bytes，契约累计 352 个 export。2026-10-18 接受，完整轮次与历史成本见[Variant 验收](refactoring-variant-2026-10-18.md)。

## R2 GUID 资源辅助

三个新增 export 汇入原 GUID 契约，家族合计六项；移除重复 ABI/arity/provider、legacy bit 路由与动态 lookup，资源与返回帧仍按架构协调。四项独立旧行为基线及完整 release 527、debug 374、native 7、工具 20 项和 fmt/strict Clippy/build 通过。

63 条初筛路径全部校验，八项同哈希三顺序扩样，共 71 个 workload protocol；保留 multimedia/IR +3.181%、VirtualQuery/IR +3.584% 的局部成本，独立 diagnostics 的计数、CPU 源码及五个 PE32 执行器节助记符序列保持，具体机制尚未确证。原 GUID/raw 扩样 +1.135%，新错误返回网关 Win64 −0.951%、PE32 IR 扩样 −0.279%；初筛 RSS 最多 +112KiB。Alice 前缀 CPU −0.401%、wall −0.530%，保持隐藏窗口和零 Draw/Present。306 个 Rust 文件核对一致，二进制减少 5,088 bytes，契约累计 355 个 export。2026-10-18 接受，完整轮次与成本见[GUID 资源辅助验收](refactoring-guid-allocation-2026-10-18.md)。

## R2 基础字符串、指针与进程查询

29 项 export 汇入三个契约家族，删除混合 common_api、重复 ABI/arity 与名称分发。WMI 内部 BSTR 直接借共享 owner，PE32 资源请求仍由原入口提交；Win64 仅保留原四项 processor/group-count typed。六项独立旧行为基线及完整 release 533、debug 380、native 7、工具 20 项和 fmt/strict Clippy/build 通过。实现提交 `2999453`。

首版 spec 数据重复和两个持续 code-alias 回退候选拒绝；最终共用 spec 数据、按字段恢复缓存，二进制只增加 48 bytes。68 条初筛与七条同哈希三顺序扩样，共 75 个 workload protocol 全部校验；code-alias −5.378%，新 processor 网关 Win64 −6.369%。保留 memory/SSE +3.612%/+3.853% 局部成本及原始离散轮次，独立 diagnostics 与执行器助记符调查未确证具体机制；首次错误节统计已纠正，受静态分析重叠影响的 bit 全组三顺序重跑。Alice 200M 前缀 CPU −0.876%、wall −2.273%，保持隐藏窗口和零 Draw/Present。314 个 Rust 文件核对一致，契约累计 384 个 export。2026-10-18 接受，完整记录见[基础 helper 验收](refactoring-basic-2026-10-18.md)。

## R2 键盘状态与布局

16 项 export 汇入 KeyboardApi，移除重复 ABI/arity/DWORD/provider、旧名称调用与 menu 中重复未实现分支。纯布局算法逐段保持，Keyboard owner 继续管理异步/队列/文字；PE32 三项 typed、十三项 ABI-only，Win64 保留 kernel 静态导入与 user32 动态查询的差异。四项独立旧行为基线及完整 release 537、debug 384、native 7、工具 20 项和 fmt/strict Clippy/build 通过。实现提交 `386f38a`。

28 条初筛与四条同哈希三顺序扩样，共 32 个 workload protocol 全部校验；初筛最大 CPU 回退 +2.652%，RSS 最多 +96KiB。新 keyboard/IR 扩样 −7.217%、raw 初筛 −5.475%，Win64 state/layout +0.954%/−2.034%。保留 integer/IR 扩样 +3.102% 的中位数成本与明显漂移，九次计数/最终寄存器及 x86 源码保持，具体机制未确证。Alice 200M 前缀 CPU −3.102%、wall −3.402%，保持隐藏窗口和零 Draw/Present。320 个 Rust 文件核对一致，二进制增加 2,928 bytes，契约累计 400 个 export。2026-10-18 接受，完整记录见[键盘验收](refactoring-keyboard-2026-10-18.md)。

## R2 raw input 与 IME

24 项 export 汇入 RawInputApi/ImeApi，删除重复 ABI/arity/DWORD/provider 与名称分发；raw handler 只借 Memory，IME 无 context 策略独立于进程总状态。静态与动态 provider、完整 HWND、PE32 ABI-only 和错误/输出顺序保持。实现 `f6f6fbc`，独立基线 `adac711`；完整 release 542、debug 389、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

30 条初筛和四条扩样全部校验，SSE 扩样额外加入双端同哈希复核；原 +19.803% 中位数与漂移轮次保留，六对合并 −0.822%。保留 FP/IR +2.172%、FP/raw +2.986% 和 Alice CPU/wall +2.813%/+2.631% 的局部成本。27 个核心源文件、执行计数和主要代码生成调查记录在案，具体成本机制未确证；二进制 +8,336 bytes，324 个 Rust 文件核对一致。2026-10-18 接受，累计 424 个 export，详见[完整验收](refactoring-input-profile-2026-10-18.md)。

## R2 矩形与窗口边框

13 项 export 汇入 GeometryApi，删除重复 ABI/arity/DWORD/provider 与名称分发；RECT/frame 纯核只处理值，两端适配器只借明确资源。packed POINT、静态/动态 provider、Win64 七项 absent 与错误/输出顺序保持。实现 `fd3da58`，独立基线 `3e96c84`；完整 release 548、debug 395、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

28 条初筛与两条 900M 同哈希扩样全部校验，bit-test/data-alias 初筛 +3.196%/+3.422%，扩样 +1.039%/−1.246%；新矩形网关 Win64 −2.439%、PE32 IR/raw −19.121%/−17.978%。RSS 最多 +64KiB，Alice CPU/wall −5.910%/−8.017%，保持同一启动终点。330 个 Rust 文件核对一致；二进制 +20,624 bytes 超原 +16KiB 文件预算，经 Mach-O 调查明确修订为 +24KiB，section 使用量合计 +42 bytes，主要成本来自 segment 页对齐与链接元数据。2026-10-18 接受，累计 437 个 export，详见[完整验收](refactoring-geometry-2026-10-18.md)。

## R2/R3 游标与指针能力

10 项 export 汇入 CursorApi，删除重复 ABI/arity/DWORD/provider 与名称分发。私有 owner 管理完整句柄、signed 坐标、per-thread saturating count，保持两端 handle/thread key 宽度；Win64 clip、host 鼠标输入与消息元数据共用同一状态。tracking 只借 Memory 和窗口存在性查询，原能力及错误/输出顺序保持。实现 `24d918d`，独立基线 `421938e`；完整 release 555、debug 401、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

31 条性能路径全部校验，最大 CPU 中位数回退 GDI/raw +2.559%，GUID/IR +2.558%；新 ShowCursor 网关 Win64 −3.306%、PE32 IR/raw −0.929%/−1.637%，RSS 最多 +128KiB。Alice CPU/wall −0.082%/−0.181%，保持启动终点。337 个 Rust 文件核对一致，二进制减少 3,680 bytes。2026-10-18 接受，累计 447 个 export，详见[完整验收](refactoring-cursor-2026-10-18.md)。

## R2/R3 图标与光标资源

8 项 export 汇入 ImageApi，删除重复 ABI/arity/DWORD 与名称分发。Images 私有拥有图像、单调句柄与实际 payload，cursor 共用原身份校验；Key 与 icon group/DIB 纯核独立。PE32 DestroyIcon 的明确 unsupported trace 及两端原能力/错误顺序保持。实现 `9fb4b93`，独立基线 `af5d003`；完整 release 560、debug 406、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

34 条性能路径全部校验，最大 CPU 中位数回退 +1.551%；共享加载网关 Win64 −1.912%、PE32 IR/raw −4.620%/−2.813%，RSS 最多 +96KiB。Alice CPU/wall −0.435%/−0.568%，保持启动终点。343 个 Rust 文件核对一致，二进制增加 3,856 bytes。2026-10-18 接受，累计 455 个 export，详见[完整验收](refactoring-images-2026-10-18.md)。

## R2 虚拟桌面与显示查询

16 项 export 汇入 DisplayApi，删除重复 ABI/arity/DWORD/provider 与名称分发；共享 desktop profile 与 device/mode/monitor 编码，callback 使用具名进程控制路由。保留 packed POINT、两端 monitor 验证/CCD 错误/输出差异及 Win64 metrics 动态查询缺口。独立基线纠正原审计遗漏，运行时未迎合错误预期；实现 `0acb937`、基线 `c415d78`。完整 release 566、debug 412、native 7、工具 20 项与 fmt/strict Clippy/build 通过。

37 条性能路径全部校验，最大 CPU 中位数回退 +1.567%；monitor 查询网关 Win64 −0.896%、PE32 IR/raw −20.398%/−18.693%，RSS 最多 +160KiB。保留 integer/IR 中位数与逐对结果不一致的漂移，未宣称通用整数收益。Alice CPU/wall −1.384%/−1.359%，保持隐藏窗口与零 Draw/Present。347 个 Rust 文件核对一致，二进制增加 8,416 bytes。2026-10-18 接受，累计 471 个 export，详见[完整验收](refactoring-display-2026-10-18.md)。

## R2/R3 GDI DC 与设备能力

七项 export 汇入 GdiDcApi，删除重复 ABI/arity/DWORD/provider 和名称分发。PE32 DC 操作直接借用原 Graphics bitmap/font/DC backing；Win64 私有 owner 拥有完整窗口/线程/compatible 身份及单调 handle 范围。原能力差异、ABI-only/absent 策略与错误/返回帧顺序保持。实现 `594a38f`、独立基线 `41f561c`；完整 release 572、debug 418、native 7、工具 20 项与 fmt/strict Clippy/build 通过。

40 条性能路径全部校验，最大 CPU 中位数回退 GUID/IR +2.985%，RSS 最多 +96KiB；新有效 DC 查询 Win64 +0.026%、PE32 IR/raw −4.065%/−3.336%。保留 Win64 网关局部成本、integer/IR 和 GUID/IR 的时间漂移。Alice CPU/wall −0.169%/−0.123%，保持隐藏窗口和零 Draw/Present。353 个 Rust 文件核对一致，二进制增加 3,104 bytes。2026-10-18 接受，累计 478 个 export，详见[完整验收](refactoring-gdi-dc-2026-10-18.md)。

## R2/R3 桌面设置与 DWM

六项契约统一 provider/ABI/typed route，固定 profile 与进程 screen-reader 状态具有明确归属，保留 PE32 ABI-only/unknown 与 Win64 动态表差异。实现 `fb73a4c`，独立基线 `ac29b45`。release 577、debug 423、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

43 条性能路径全部校验；桌面查询 Win64/PE32 IR/raw CPU −1.088%/−20.724%/−21.863%，RSS 最多 +176KiB。bit-test/IR 初筛 +4.269%，含同一 before control 的追加三对 +0.240%，原始漂移数据保留。Alice CPU/wall +0.241%/+0.303%，同隐藏窗口、零 Draw/Present 预算出口。359 个 Rust 文件核对一致，二进制减少 4,544 bytes。2026-10-18 接受，累计 484 个 export，详见[完整验收](refactoring-desktop-2026-10-18.md)。

## R2/R3 窗口类与元数据

十七项契约统一注册、注销、类信息/字段/名称的 provider 与 ABI，Win64 类表和 atom 具有私有 owner，PE32 记录与持久名称 handler 收敛并保留公开 facade 和隐式控件。独立基线 `a899cd5`、实现 `a7f313f`；首次内部可见性和 native fixture 私有入口问题已修正并保留原日志。最终 release 583、debug 429、native 7、工具 20 项及 fmt/strict Clippy/build 通过。

46 条性能路径全部校验；类接口 Win64/PE32 IR/raw CPU −15.632%/−6.676%/−4.082%，RSS 最多 +144KiB。desktop/IR 和 images/raw 初筛 +3.080%/+3.774%，含同一 before control 的三对补证 +0.844%/−0.214%，原始轮次与 integer/IR 漂移保留。Alice CPU/wall −1.888%/−2.156%，保持同预算隐藏窗口与零 Draw/Present 出口。365 个 Rust 文件核对一致，二进制增加 9,040 bytes。2026-10-18 接受，累计 501 个 export，详见[完整验收](refactoring-classes-2026-10-18.md)。

## USER 名称/属性与窗口查询集成候选

十四项名称/属性首版因 Win64 对照成本拒绝，布局候选撤回；trace 修订保留参数所有权、容量与顺序，避免小参数通用 memcpy 和完整记录的早期临时复制。旧协议最终矩阵出现同哈希副本的数倍差异，固定 executable/cwd 的独立调查与显式工具修订见[测量报告](refactoring-fixed-executable-slot-2026-10-18.md)，旧轮次全部保留。

二十五项窗口查询已接入唯一契约与受限 scalar adapter，旧 ABI、arity、dwords/provider 和名称分支已删除，保留两架构 LastError、字段/extra 宽度、WNDPROC 检查和输出故障差异。首轮集成 memory 长样本 +5.114% 触发调查；修正实际 CLI caller 的 codegen 审计后发现 checked_access 的内联 helper 增加栈操作，固定调用边界后长样本降至 +0.842%。实现 `5983f95`、窗口查询 `2e003ac`。

最终 release 600 / debug 446 / native 7 / 工具 23 和 fmt/strict Clippy/build 通过，377 个 Rust 文件核对一致，二进制相对 Classes +5,424 bytes。55 条性能协议、342 次执行及三对 Alice 前缀全部校验；PE32 三条离散路径追加 control 后为 −0.697%/−0.525%/+1.735%，原始慢轮保留。2026-10-18 接受名称/属性十四项与查询二十五项，累计 **540 exports**；详见[完整集成验收](refactoring-memory-call-boundary-2026-10-18.md)。

## 窗口文本与输入所有权

十六项唯一契约、PE32 状态提交/guest 通知协调与 Win64 scalar adapter 已接入，移除重复 ABI 和名称路由；原两架构 NULL、编码、callback 与 absent 差异保持。基线 `71b53a4`、实现 `fe6b552`，完整 release 600 / debug 446 / native 7 / 工具 23 及 fmt/strict Clippy/build 通过。380 个 Rust 文件核对一致，二进制较上一已验收冻结 +6,736 bytes，在预设 +16KiB 内。

59 个性能协议、369 次执行和三对 Alice 前缀全部校验，长度查询 Win64/PE32 IR/raw CPU +1.614%/−25.612%/−22.253%；PE32 四条扩样成本均在 ±1% 内。Win64 rectangle 的数倍差异同时出现在同一旧二进制 control，不归因为实现收益。Alice CPU +0.083%、wall +10.116%，保持同预算隐藏窗口、零 Draw/Present；启动离散保留。2026-10-18 接受，累计 **556 exports**；见[完整验收](refactoring-window-state-2026-10-18.md)。

## 窗口生命周期与过程控制

十七项唯一契约统一创建、销毁、默认过程、过程转发、显示/更新、位置及同步消息；PE32 协调器保留阻塞/非局部转移和先提交状态再回调的规则，Win64 显式 callback/resume 与 UpdateWindow scalar 例外分别保留。旧 ABI、arity/DWORD/provider 和名称执行分支删除。独立基线 `be59d32`、实现 `b3ccab4`，完整 release 603 / debug 449 / native 7 / 工具 23 与 fmt/strict Clippy/build 通过，384 个 Rust 文件核对一致，二进制减少 12,720 bytes。

69 个协议、450 次执行全部校验，真实 SendMessage/WNDPROC 网关 Win64/PE32 IR/raw CPU −0.828%/−1.387%/−7.536%。SSE/raw 扩样相对 before +1.544%、相对 control +3.139% 局部成本保留；GDI/IR 的 after/before +4.357% 同时伴随旧二进制 control/before +4.708%，after/control −0.336%。Alice CPU/wall −0.009%/−0.636%，保持隐藏窗口和零 Draw/Present 预算出口。2026-10-19 接受，累计 **573 exports**；详见[完整验收](refactoring-window-control-2026-10-18.md)。

## 消息队列、过程分发与计时器

22 项契约统一消息投递/提取、DispatchMessage callback/resume、计时器与等待；保留五项 PE32 ABI-only、六项 Win64 absent，以及双架构消息输出故障的不同提交顺序。基线 `f818b33`、实现 `c07b983`，完整 release 607 / debug 453 / native 7 / 工具 23 与 fmt/strict Clippy/build 通过，388 个 Rust 文件核对一致，二进制增加 5,728 bytes，在 +24KiB 预算内。

38 个协议、237 次执行全部校验。post/get 网关 Win64/PE32 IR/raw CPU −1.417%/−10.391%/−1.827%。PE32 SSE/IR 初筛 +10.556%，900M 三标签补证为 −0.246%，同哈希 control/before +4.965%；首轮与后续离散全部保留，未形成持续回退。FP 两架构复核的明显差异继续记录为纯重构性能观测，ISA 源码未改。Alice CPU/wall −0.907%/−7.945%，保持隐藏窗口和零 Draw/Present 预算出口。2026-10-19 接受，累计 **595 exports**；详见[完整验收](refactoring-messages-2026-10-18.md)。

## 对话框、菜单、GDI 与后续累计复核

日志修订 `b433c63` 完成对话框 22、菜单 27、GDI 31 项的累计验收，达到 675 项，独立旧候选及采样证据见[日志报告](refactoring-api-log-2026-10-19.md)。随后五项 paint、两项 comctl32 初始化、十项资源、五项启动信息、九项环境接口与 CLI 拆分完整正确性通过；Unity 后台渲染及同哈希显著漂移期间暂停长计时。

用户确认渲染结束后，以原 675 项冻结与 `d65c389` 环境冻结作三标签轮换，32 条协议、288 次执行及三对 Alice 200M 前缀全部校验。最大 CPU 中位数回退 +2.574%，保留 SSE/control 局部成本与 GDI 同哈希漂移；fixture RSS 中位数最多 +112KiB，累计 executable +44,272 bytes。Alice CPU/wall −1.543%/−0.328%，仍为隐藏窗口、零 Draw/Present 的启动前缀。release 644、debug 490、native 7、工具 28 及静态检查/build 沿用同源码冻结，302 个 src Rust 文件重新核对。2026-10-19 累计接受 **706 exports** 及 CLI，见[完整复核](refactoring-post-unity-performance-2026-10-19.md)。

## R3 Win64 ETW provider

注册表与单调分配器收拢为私有 Providers，调用只借 Memory/参数，原 handler 经过 owner 改名后逐字一致。三项独立旧行为测试先提交于 `480a14e`；完整 release 647、debug 493、native 7、工具 29 与静态检查/build 通过。12 条性能协议、108 次执行及三对 Alice 前缀通过，binary +176 bytes，302 个 src Rust 文件核对一致。ETW CPU −0.149%，其它主路径最大回退 +2.110%；游戏 CPU/wall −0.168%/−0.519%，保持预算/隐藏窗口/零 Draw/Present。没有新增 export，累计仍为 706；见[完整验收](refactoring-etw-owner-2026-10-19.md)。

## R3 Win64 token

query access、flags、资源表与单调编号收拢为私有 Tokens，open/query 仅借 Memory/参数/当前线程 LastError 地址，通用句柄操作显式委托。三个独立旧行为测试和原创 fixture 先提交于 `87ae227`；完整 release 650、debug 496、native 7、工具 30 及静态检查/build 通过，302 个 src Rust 文件核对一致，binary +9,536 bytes。

17 条协议、162 次执行和三对 Alice 前缀完成验收。token 查询 CPU −0.639%，SSE/thread-id 初筛波动经 900M 收敛；保留 PE32 integer/IR 六对长样本 +3.320% 的局部成本、同哈希漂移与全部慢轮。两端分别两次采样、执行计数及六个执行器助记符核对完成，具体机制尚未确证，没有三对均超过 5% 的持续回退。Alice CPU/wall −3.611%/−3.871%，保持原预算/隐藏窗口/零 Draw/Present。累计仍为 706；详见[完整验收](refactoring-token-owner-2026-10-19.md)。

## R3 Win64 Toolhelp 快照

私有 Snapshots 拥有记录/句柄/flags/游标，组合根仅提供 guest 文件名和线程值；原 codec 除可见性外逐字保持。三个独立旧行为测试及 fixture 先提交于 `0962939`，完整 release 653、debug 499、native 7、工具 31 及静态检查/build 通过。302 个 src Rust 文件核对一致，binary 较 token −18,896 bytes。

19 条协议、180 次执行及三对 Alice 前缀验收完成。保留 data-alias 900M +2.959% 与 raw 六对长样本 +2.391% 的局部成本；初筛及 raw 首轮 +10.789% 的同哈希漂移、静态代码生成差异和补测全部记录。另以 ETW 直接比较最终冻结，三对 900M PE32 integer/IR 为 −0.061%，不将 token 阶段的成本追认为收益。Alice CPU/wall −0.247%/−0.598%，保持原预算/隐藏窗口/零 Draw/Present。累计仍为 706；详见[完整验收](refactoring-snapshot-owner-2026-10-19.md)。

## R2 Toolhelp 七项契约

七项 provider/ABI/typed ID 收进唯一声明，Win64 owner 枚举不再解析 API 名称；PE32 保持 kernel32 ABI-only 及未知 DLL 的零清栈策略。独立旧行为基线 `5db0070`，完整 release 655、debug 501、native 7、工具 31 和静态检查/build 通过。303 个 src Rust 文件核对一致，binary −9,968 bytes，累计 **713 exports**。

17 个计时协议、162 次执行和三对 Alice 前缀通过，Win64 copyout −12.007%，PE32 ABI-only 快捷路径 IR/raw −82.834%/−77.516%，四份完整日志逐字节一致。code-alias 900M 保留 +4.286% 局部成本，已扩样、采样并核对实际 CLI/cache 热区操作数；具体机制尚未确证。raw 六对 900M 汇总 −0.745%。Alice CPU/wall −1.008%/−0.404%，保持原预算/隐藏窗口/零 Draw/Present。详见[契约验收](refactoring-snapshot-contract-2026-10-19.md)。

## R2 Token 三项契约与验证粒度调整

用户要求先评估性能验收是否必要，减少普通结构迁移的重复矩阵，不因小幅波动停滞重构。当前[验证策略](../architecture/validation-tools.md)据此更新，原始计划文件和历史性能证据保留。

三项 TokenApi 统一 provider、arity 与 DWORD，删除重复手写 ABI/支持列表和字符串分支。独立旧行为基线 `00966ee`，保留 Win64 fresh provider 动态不可见、已注册 gateway fallback 以及 PE32 的 provider-specific 清栈。原 token owner/SID codec 逐字不变，release 657、debug 定向 7、工具 31、fmt/strict Clippy/build 通过，304 个 src Rust 文件核对一致。六次小型功能重放核对结果/计数，四份 PE32 完整 trace 逐字节相同。本批评估为低风险，不跑性能或游戏/native 矩阵。binary −10,960 bytes，累计 **716 exports**；详见[契约验收](refactoring-token-contract-2026-10-19.md)。

## R2 SID 九项契约与适配器

独立基线 `1df4813` 固定 provider/ABI、BYTE trace、DWORD index、高位指针、header-only 初始化及两种 guard/fault/error 提交顺序。九项 SidApi 及无状态受检内存逻辑迁出 Runtime64，token owner 删除 codec 后逐字保持，累计 **725 exports**。release 定向 5、debug AMD64 定向 4、catalog 一致性 1、fmt/strict Clippy/build 通过，306 个 src Rust SHA 与 executable SHA 记录完整。按低风险评估不跑性能或游戏/native 矩阵；详见[验收](refactoring-sid-contract-2026-10-20.md)。

## R2 ETW 八项契约

独立基线 `29d2a57` 固定 PE32 REGHANDLE/keyword 双 DWORD slot、Win64 BYTE/U64 trace、DWORD 截断和静态/动态 provider 差异。八项 EtwApi 统一元数据，原 owner 的资源/检查/写出顺序经源码替换核对保持，累计 **733 exports**。所属 release 7、debug AMD64 6、fmt/strict Clippy/build 和短八接口 fixture 通过；token/SID/ETW 三批合并后仅做一次 release 全量检查，664 passed / 7 ignored。307 个 src Rust SHA 与 executable SHA 已记录，不跑性能或无关游戏/native 矩阵；见[验收](refactoring-etw-contract-2026-10-20.md)。

## R3 Win64 注册表

独立基线 `71fef81` 固定单调句柄、累计耗尽、完整身份与查询别名/guard 顺序。处理函数迁入私有 Registry，CP932/UTF-16 名称解码复用 text helper，源码归一核对与原 handler 一致。release Win64 260、debug 注册表 3、fmt/strict Clippy/build 通过；不跑性能或无关游戏/native 矩阵。15 项元数据仍为 legacy，累计仍为 733 exports，见[验收](refactoring-registry-owner-2026-10-20.md)。

## R2 注册表 17 项契约

独立基线 `c872fbb` 固定 provider、PE32 受限能力和 Wine fallback ABI、Win64 DWORD trace 及旧式 QueryValue 缺失。统一契约替换双架构重复声明，PE32 受限处理函数移入适配器，Win64 owner 改用 RegistryApi。相关 release 6、catalog 一致性与 fmt/strict Clippy/build 通过，不做性能矩阵；累计 **750 exports**，见[验收](refactoring-registry-contract-2026-10-20.md)。

## R2/R3 BCrypt RNG

独立基线 `19517df` 固定 provider、DWORD trace、全宽复用句柄和容量/输出故障顺序。三项契约与私有 RNG owner 接入，原 OS entropy/handler 经归一核对保持；catalog 的第 64 族触发类型递归上限后改为静态分组迭代，原家族顺序保持。相关 release 3、catalog 一致性、fmt/strict Clippy/build 通过，公共 catalog 另做一次整合正确性回归；不跑性能矩阵。累计 **753 exports**，见[验收](refactoring-bcrypt-2026-10-20.md)。

## R2/R3 SSPI

独立基线 `613383c` 固定 34 项静态/动态 provider 与 gateway fallback。统一 SspiApi、私有 A/W 函数表 owner 和 Gateways 冷注册接入，34 项 arity/DWORD 与原表逐项一致，原函数表主体归一核对保持。release Win64 264、网关单元/catalog 一致性及 fmt/strict Clippy/build 通过，不跑性能或无关游戏/native 矩阵。累计 **787 exports**，见[验收](refactoring-sspi-2026-10-20.md)。

## R2 WinHTTP 五项契约

基线 `1bd3adf` 的既有 proxy/transport 回归已通过，五项声明汇入 WinHttpApi，handler 迁入无状态受检适配器并删除原 runtime 模块。相关 release 1、catalog 一致性和 fmt/strict Clippy/build 通过；按低风险评估不新增重复测试或性能矩阵。累计 **792 exports**，见[边界与验收](../architecture/winhttp.md)。

## R2/R3 Devices 契约与资源所有权

24 项 DeviceApi 统一 HID/SetupAPI/user32 精确 provider、arity 与 DWORD，handler 迁入设备集合与通知订阅 owner。基线 `e32efe3` 的 24 项元数据逐项核对一致，原处理函数经 typed ID、owner、窄借用与格式归一后保持一致。相关 release Win64 2、通知单元 1、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过；既有检查覆盖空设备集合、HID status/输出、通知生命周期与窗口销毁清理。按低风险评估不新增重复测试或性能矩阵。累计 **816 exports**，见[边界与验收](../architecture/devices.md)。

## R2/R3 Winsock 契约与启动状态

20 项声明及九项 Win64 序数归一移入 catalog，启动引用计数及标量 handler 迁入私有 Winsock owner。共用事件和等待/APC/CPU 提交仍由原所有者负责，删除重复 ABI、provider 和字符串分组。元数据、序数、标量 handler 及等待主体对照基线 `4b5e327` 保持一致。相关 release network 7、PE32 ABI-only/未知序数回归 1、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过，不做性能计时。累计 **836 exports**，见[边界与验收](../architecture/winsock.md)。

## R2/R3 进程选项与启动属性

五项 ProcessOptionApi 统一 ABI/provider，进程错误模式和启动属性列表迁入私有 owner；线程创建通过查询继承进程模式。处理函数经 ID、owner、窄借用和整除写法等价整理对照 `b6ac06a` 保持一致，线程创建除此项查询外逐字不变。相关 release 属性列表及真实线程/APC 共 2、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过，不新增重复测试或性能计时。累计 **841 exports**，见[边界与验收](../architecture/process-options.md)。

## R2 系统与临时目录

独立旧行为基线 `dba0edf` 固定目录查询的 A/W 单位、两种参数顺序、容量不足/NULL 查询、返回槽别名、跨页写出故障和 PE32 ABI-only 清栈。十项声明接入，Win64 copyout 迁入无状态适配器，三个默认目录集中于 GuestEnvironment；保留 Win64 ANSI temp absent。双架构定向 release 各 1、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 **851 exports**，见[边界与验收](../architecture/system-paths.md)。

## R2 标量入口补齐

五项 encoding/stack guarantee/file ANSI/processor topology 入口汇入既有家族，保留 provider、架构能力、指针宽度、DWORD 和公开 facade 范围。原 topology codec 归一保持。release 双架构整合 Win64 265、PE32 244、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。累计 **856 exports**，见[合并验收](refactoring-scalar-followup-2026-10-20.md)。

## R2/R3 异常入口与注册元数据

独立旧行为基线 `b014544` 固定 provider/ABI、完整 token/filter、返回帧预检和累计耗尽。十一项 ExceptionApi 汇入唯一契约，Win64 有序 handler/filter 与 function-table 集合成为私有 owner；Process64 继续负责 guest 回调、Raise/Unwind 和 CPU/计数提交。原查找、表项/结果校验、静态注册与 CONTEXT codec 经归一核对保持。release Win64 边界 2、Process64 8、PE32 异常 7、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 **867 exports**，见[边界与验收](../architecture/exceptions.md)。

## R2 Shell 与错误消息

基线 `d034c5b` 固定 shell 静态导入/动态发现差异、PE32 known-folder 分配和 provider 清栈。六项 ShellApi、两项 MessageFormatApi 接入；PE32 旧 NLS shell 主体移入窄借用 adapter，公开 facade 保留；Win64 受检准备与原 heap/local/task 分配协调分离，纯命令行 parser 移入 kernel 算法。原 parser、消息及 shell 主体归一核对保持。相关 release 及单元 31 项、fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 **875 exports**，见[边界与验收](../architecture/shell-and-messages.md)。

## R2/R3 进程与文件 endpoint

基线 `c39a740` 固定 PE32 endpoint ABI 与原生目录查询策略。四项 FileApi、两项 ProcessControlApi 接入；管道创建归 Pipes，目录查询及游标提交归 Files，Win64 child 预检与 CPU/诊断完成分离。根 Win64 ABI 声明仅剩 legacy 图形回退。release Win64 268、PE32 247、library 60 passed / 7 ignored，fmt/release lib/bin strict Clippy/build 通过，覆盖前三批连续修改；不跑性能。累计 **881 exports**，见[边界与验收](../architecture/process-control.md)。

## R2 图形创建入口

基线 `01b714f` 固定七个图形 export 的 provider/arity/DWORD、动态发现及 PE32 未知 ABI。契约保留 kernel 静态别名、exact-case 内部 provider、D3D12 ordinal 和现有 gateway fallback；创建通过 bound ID 分发，COM 方法处理体和槽位保持。release 图形 10、PE32 1、模块 12、catalog 1、真实 GPU swapchain 1 项以及 fmt/release lib/bin strict Clippy/build 通过。累计 **888 exports**，见[边界与验收](../architecture/graphics-contracts.md)。

## R2 图形 COM 方法冷绑定

基线 `7435932`。70 条 DXGI 方法的 arity/DWORD 合为同一槽位声明，D3D11 原表保持并合并查找；BoundMethod 缓存静态声明和家族路由，调用时移除 D3D11 表扫描。groups 位图改为明确 graphics/file_search 字段。槽位、处理体和参数规则核对一致；release 图形 10、真实 GPU 1、文件 17、fmt/release lib/bin strict Clippy/build 通过。COM 内部方法不计作新增 DLL export，累计仍 **888**，见[边界与验收](../architecture/graphics-contracts.md)。

## R3 PE32 kernel 对象

基线 `b5182db`。KernelObjects32 私有拥有原混合 HANDLE 表、命名索引和递增分配位置，ApiState 兼容方法委托；CloseHandle 的对象/名字清理归 owner，FindClose 保留独立类型检查。原变体、分配、同名、gateway 与进程查询主体核对保持。release Win32 248、线程间 owner 迁移 1、fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 export 仍 **888**，见[边界与验收](../architecture/kernel-objects32.md)。

## R3 PE32 文件窄借用

基线 `9090b62`。file_api 保留公开 facade，原处理体进入 win32/x86/files，仅借 GuestMem、KernelObjects32、filesystem 与 LastError；wildcard 进入 kernel，由两种架构直接复用。原文本 codec、文件/path 主体、facade 和纯算法核对保持。release Win32 248、Win64 文件 17、fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 export 仍 **888**，见[文件边界](../architecture/files.md)。

## R2/R7 PE32 ABI 来源收敛

基线 `0459298` 审计根手工表 301 项，270 项与已加载 spec 完全一致，零数值冲突；删除重复项，31 项例外及原因集中到 win32::abi。原 spec 解析、ordinal 准入、领域方法优先级、CRT 零清栈与未知策略保持；native_export 的被覆盖 CRT 白名单归并。release Win32 248、fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 export 仍 **888**，见[ABI 来源](../architecture/abi.md)。

## R3 PE32 scratch 与 CRT 指针

独立基线 `53f620d` 固定懒 argv、errno 和 scratch 对齐/耗尽顺序。Scratch32 与 CrtState32 分别私有拥有游标及 CRT 指针身份，ApiState 入口委托；原数学、callback、data export 和 cdecl 路径保持。分配/初始化/部分提交主体核对一致，release Win32 249、fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 export 仍 **888**，见[CRT 边界](../architecture/crt32.md)。

## R3 PE32 同步与等待窄适配

基线 `847dd25`。event/semaphore/mutex 适配器仅借对象、调度器、内存及错误槽，readiness/acquire/wake 与 DirectSound signal 仅借对象和调度器；命名对象的 LastError 策略由同一适配入口维护。原同步主体、四个对象 helper、命名复用与 wait 控制流程归一核对保持。release 完整 Win32 249、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。累计 export 仍 **888**，证据为 `wire-out/refactoring-2026-10-14/r3-sync32-{source-checks,verification}.json`，见[等待边界](../architecture/waits.md)。

## R3 PE32 CRT 普通语义窄适配

基线 `b28ecc6`。memory/string/time 仅借内存，CRT heap 仅借 allocator 与 process heap，启动 accessor 与 strtoul 由 CrtState32 显式借 scratch/error/线程资源。原普通算法、分配、启动、errno 顺序、FPU/qsort/data export/initializer 和 gateway 主体归一核对保持。release CRT 8、完整 Win32 249、fmt/release lib/bin strict Clippy/build 通过，不跑性能。没有扩大 CRT 能力或新增 catalog 计数，累计 export 仍 **888**，证据为 `wire-out/refactoring-2026-10-14/r3-crt-scalar32-{source-checks,verification}.json`，见[CRT 边界](../architecture/crt32.md)。

## R2 PE32 kernel endpoint 补齐

独立基线 `12ed607` 固定 TerminateProcess、SetHandleCount 和 GetCurrentPackageId 的 provider、ABI、输出不变与退出故障提交。三项接入既有家族，终止/容量主体归一保持，Win64 均维持 absent。release Win32 251、Win64 modules 12、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 **891 exports**，见[进程 endpoint](../architecture/process-control.md)，证据 `wire-out/refactoring-2026-10-14/r2-kernel-endpoints32-{source-checks,verification}.json`。

## R2 PE32 USER/MPR endpoint 补齐

基线 `6cc04bf` 固定两个 station、两个诊断 MessageBox 与两个 MPR 查询。六项进入既有领域契约，MessageBox 仅借 GuestMem，原 synthetic handle、未支持 query、local-drive status、LastError、fault marker 和 facade 范围保持；Win64 均 absent。基线测试首次误认为 MPR ABI unknown，实际已加载 spec，修正为原 16 字节清栈后通过，失败日志保留。release 完整 Win32 252、Win64 modules 13、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 **897 exports**，证据 `wire-out/refactoring-2026-10-14/r2-user-endpoints32-{source-checks,verification}.json`。

## R2/R3 PE32 文件版本资源

独立基线 `32fa8e3` 固定输出预检/部分清零/返回别名和 provider。六项契约接入，原 private version 模块拆为纯 PE 字节 parser 和仅借内存/filesystem/error 的适配器；原处理体归一保持，删除六条重复 ABI override，剩余 25 项例外。release 完整 Win32 253、Win64 modules 13、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过，不跑性能。累计 **903 exports**，见[版本资源边界](../architecture/version-resources.md)，证据 `wire-out/refactoring-2026-10-14/r2-version-resources32-{source-checks,verification}.json`。

## R2 PE32 不可用音频与字体入口

独立基线 `3bf4860` 固定十二项原拒绝行为、provider、ABI、fault 与返回槽别名。音频和字体资源各六项接入 typed 契约，Win64 保持 absent，原错误/输出/LastError/trace 语义保持。release 完整 Win32 255、Win64 modules 13、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过；普通契约迁移不跑性能或游戏矩阵。累计 **915 exports**，见[边界与验收](../architecture/unavailable-endpoints.md)，证据 `wire-out/refactoring-2026-10-14/r2-unavailable-endpoints32-{source-checks,verification}.json`。

## R3 PE32 GDI+ 资源借用

基线 `2760af1`。GdiPlus 的矩阵、allocation 与 startup token 调用只借自身集合、内存和 process heap；公开 facade 保留。原处理体、数学/codec、allocation 回滚及 spec ABI parser 归一核对保持。release GDI+ 4、完整 Win32 255、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计契约仍 **915**。见[GDI+ 边界](../architecture/gdiplus.md)，证据 `wire-out/refactoring-2026-10-14/r3-gdiplus32-{source-checks,verification}.json`。

## R3 PE32 WMI 对象调用

基线 `8aa4165`。对象、refs、cursor 和 vtable 继续由 Wmi 私有拥有，调用只借内存、gateway table、heap 和 BSTR；移除整份 registry 的临时 take。原处理体、创建/输出提交、方法表及 IID 归一核对保持。release com 过滤 30、完整 Win32 255、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计契约仍 **915**。见[WMI 边界](../architecture/wmi.md)，证据 `wire-out/refactoring-2026-10-14/r3-wmi32-{source-checks,verification}.json`。

## R3 PE32 DirectSound 设备借用

基线 `c0d296b`。对象/分配方法显式借 heap、event/window/Presenter 只读前置资源，调用直接借设备，通知 drain 只借 pending/KernelObjects32/Sched。原方法/回滚、buffer codec、混音/native pump 和 ABI parser 核对保持。release audio 4、mixer 1、完整 Win32 255、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计契约仍 **915**。见[DirectSound 边界](../architecture/directsound.md)，证据 `wire-out/refactoring-2026-10-14/r3-dsound32-{source-checks,verification}.json`。

## R3 PE32 D3D9 资源借用

基线 `95b284d`。24 个生命周期/分配/绑定方法只借 heap，设备层再借只读 window/Graphics；普通方法不再接收 ApiState，根入口移除后端 take。原处理体/回滚/诊断归一或格式化后对照保持，raster/GPU 源码未改。初次编译检查误改 facade，修正后行为验收通过，失败日志保留。release 图形 11、library 4 passed / 2 ignored、显式真实 Metal 2、完整 Win32 255、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。累计仍 **915**，见[D3D9 边界](../architecture/d3d9.md)，证据 `wire-out/refactoring-2026-10-14/r3-d3d9-owners32-{source-checks,verification}.json`。

## R3/R4 PE32 线程创建与有序退出

基线 `9993349`。普通创建移入窄资源适配器，保持晚 ID copy-out 与已提交的 thread/handle/TLS；有序退出移入 Process，显式 ExitThread 和 worker EAX 路径共享 DLL→TLS→USER→scheduler 清理，进程退出也共享原通知/halt 流程。原创建及退出主体归一核对保持。release 完整 Win32 255、IR 31、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。累计契约仍 **915**，见[线程边界](../architecture/thread-control.md)，证据 `wire-out/refactoring-2026-10-14/r3-thread-lifecycle32-{source-checks,verification}.json`。

## R3 PE32 USER 窗口与线程退休

基线 `5af8a4b`。remove_window/exit_thread 归入 User32，仅借 Graphics/Menus/Sched；原 pending send 完成/唤醒、窗口图形/菜单、focus、队列、timer 与 cursor/keyboard 清理顺序归一核对保持，旧 ApiState helper 删除。release 完整 Win32 255、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。见[窗口控制边界](../architecture/window-control.md)，证据 `wire-out/refactoring-2026-10-14/r3-user-retirement32-{source-checks,verification}.json`。

## R4 PE32 callback 执行 owner

基线 `1a93e31`。原 USER invoke 归入 Process，qsort/locale/USER/UnhandledExceptionFilter 共享既有 context 恢复规则；DLL/TLS/SEH/initializer 直接调用 Process，保留各自恢复和控制策略。原 helper、八个调用文件、执行循环与 callback frame 对照保持。初次编译漏迁两个 locale 调用，补齐后 release 完整 Win32 255、IR 31、fmt/release lib/bin strict Clippy/build 通过，失败日志保留；不跑性能或游戏矩阵。见[callback 边界](../architecture/callbacks32.md)，证据 `wire-out/refactoring-2026-10-14/r4-callback-owner32-{source-checks,verification}.json`。

## R3 PE32 USER 队列资源操作

基线 `145a271`。timer、post/host 入队、change bits 与 wake 只借 User32/LastError/Sched/start；Get/Peek/Wait 和对象消息等待共用原 timer/paint prepare。两份准备主体和原 clock/timer/host/queue/mask 策略归一核对保持，队列与取样次序未改。release 完整 Win32 255、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。见[消息边界](../architecture/messages.md)，证据 `wire-out/refactoring-2026-10-14/r3-user-queues32-{source-checks,verification}.json`。

## R3 Win64 local-slot owner

基线 `1e3a534`。LocalSlots64 私有拥有 TLS/FLS 原固定数组、槽标记和 callback identity；Runtime64 继续按原 TEB/error/commit 顺序适配，destructor 按原 active index 先全部预检再清 callback 值。普通 local_call 与 hot TLS 源码归一核对保持。release 完整 Win64 269、fmt/release lib/bin strict Clippy/build 通过，不跑性能矩阵，未扩大槽容量或新增分配。见[TLS/FLS 边界](../architecture/locals.md)，证据 `wire-out/refactoring-2026-10-14/r3-locals64-owner-{source-checks,verification}.json`。

## R2/R3/R4 连续 owner 迁移整合

基线 `1155a3e`，覆盖 `2760af1` 至 `1155a3e` 的十个提交。release all-targets 共 690 passed / 7 ignored / 0 failed，21 个测试结果；all-targets/all-features strict Clippy、31 项工具测试及 fmt 通过。未改热循环或 ISA 算法，不重复性能和游戏矩阵；D3D9 两项真实 Metal 已在所属批次显式通过。证据 `wire-out/refactoring-2026-10-14/r3-owner-wave-20261020-verification.json` 记录日志和 executable hash，不复制二进制。状态入口压缩为当前结果，历史逐批记录保留。

## R3 Win64 Heap 资源操作

基线 `1155a3e`。slab/block allocator 从 Runtime64 方法改为 Heap 自身操作；仅借 Pages 显式页资源，通用分配 facade 共用原页算法，保留失败诊断、LastError、清零/回收及 ReAlloc 提交顺序。Heap 主体与页算法核对保持。初次机械提取漏改四个方法调用，修正后 release 完整 Win64 269、fmt/release lib/bin strict Clippy/build 通过，失败日志保留。不跑性能或游戏矩阵，累计契约仍 915。见[Heap 边界](../architecture/heaps.md)，证据 `wire-out/refactoring-2026-10-14/r3-heap64-owner-{source-checks,verification}.json`。

## R3 Win64 Local/Global 资源操作

基线 `f65e0bf`。LocalMemory 私有拥有原 local-block 集合，执行 Local/Global 与 FormatMessage 分配提交，只借内存和 Pages；shell argv 与线程描述保持原 copy-out 后登记。两份算法和两个外部登记点核对保持，release 完整 Win64 269、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计契约仍 915。见[Local/Global 边界](../architecture/local-memory.md)，证据 `wire-out/refactoring-2026-10-14/r3-local-memory64-owner-{source-checks,verification}.json`。

## R3 Win64 BSTR/HSTRING 调用

基线 `97acdf5`。原 allocation/call 直接归 Bstrings/Strings，只借内存和 Pages；原 membership/reference 实现未改。Trace 拥有原 note 服务，组合根、分配器和激活诊断共用。四个调用主体、BSTR source、note 和未改 membership 文件核对通过；release 完整 Win64 269、trace 1、fmt/release lib/bin strict Clippy/build 通过。Clippy 首次发现冗余 borrow，移除后通过，失败日志保留。不跑性能或游戏矩阵，累计契约仍 915。证据 `wire-out/refactoring-2026-10-14/r3-string-calls64-{source-checks,verification}.json`。

## R4 PE32 窗口状态 callback 请求

基线 `d883866`。User32 仅借 tid/LastError，准备 scalar 值、文本 callback 或已提交的所有权通知；Process 执行 callback 并逐条重查 window/WNDPROC。领域不再借 Cpu/ApiState/执行器，原 change_owner、文本验证、getter、错误和通知循环核对保持，frame/run_loop 未改。新增独立 guest fixture 在迁移前通过，覆盖前一回调替换或销毁后续窗口；release 完整 Win32 256、IR 31、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。累计契约仍 915，证据 `wire-out/refactoring-2026-10-14/r4-window-state32-{source-checks,verification}.json`。

## R4 PE32 控件与枚举 callback 请求

基线 `3217b11`。User32 仅借内存/scratch/tid/LastError，准备控件消息与顶层/子窗口原顺序快照，Process 执行回调循环。整数转换只在正常返回后执行，原 NULL、FALSE carrier、输出前缀、已销毁快照项与退出策略保持。child、消息、数字、两种快照/循环和未改 frame/run_loop 核对通过；release 完整 Win32 256、IR 31、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。累计契约仍 915，证据 `wire-out/refactoring-2026-10-14/r4-dialog32-{source-checks,verification}.json`。

## R4 PE32 sent 与 monitor callback 请求

基线 `683f025`。sent selection/processing 与完成/发送方退出/wake 归 User32，Process 执行 WNDPROC；monitor adapter 只准备内存/临时 heap 请求，Process 保持 callback 后先 free 再传播 Result 的顺序。原主体归一或 rustfmt 后核对保持，frame 未改；新增 fault 释放 fixture 在迁移前通过。release 完整 Win32 257、IR 31、fmt/release lib/bin strict Clippy/build 通过。Clippy 的 Option 提前返回写法修正后通过，失败日志保留；不跑性能或游戏矩阵，累计契约仍 915。证据 `wire-out/refactoring-2026-10-14/r4-sent-monitor32-{source-checks,verification}.json`。

## R3 Win64 COM 窄调用

基线 `82a6949`。TaskMemory 执行 CoTaskMemAlloc/Free，仅借 Pages/内存；COM adapter 仅借 Apartment/内存/Trace，不依赖 Runtime64，删除不再使用的只读 apartment facade。原 task allocate/free、init/factory 与私有释放主体核对保持，Apartment 纯状态未改。release 完整 Win64 269、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计契约仍 915。证据 `wire-out/refactoring-2026-10-14/r3-com-calls64-{source-checks,verification}.json`。

## R4 PE32 绘制 callback 请求

基线 `94eadcb`。User32 仅借内存/Graphics/LastError，准备值、立即更新与 BeginPaint 请求；Process 执行擦除，在正常返回后写 PAINTSTRUCT。原 HWND/指针验证、DC/clip、消费区域、callback fault 与输出顺序核对保持，Graphics/frame 未改。release 完整 Win32 257、IR 31、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计契约仍 915。RedrawWindow 的真实 gateway 继续为菜单未实现，公开 facade 差异保持。证据 `wire-out/refactoring-2026-10-14/r4-paint32-{source-checks,verification}.json`。

## R3 Win64 窗口 scalar owner

基线 `e0524d7`。查询、title 与 focus/foreground 操作直接归 User owner，只借内存、当前错误槽和只读 Gateways；coordinator 与 query 共用原 WNDPROC 校验。两份原处理体经明确资源替换后完全一致，保留过程优先校验、HWND/长度/权限/能力/输出顺序与字段宽度。release 完整 Win64 269、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计契约仍 915。证据 `wire-out/refactoring-2026-10-14/r3-user-scalar64-{source-checks,verification}.json`。

## R4 PE32 消息请求与连续迁移整合

基线 `d541e63`。User32 准备 timer/post/filter/MSG 输出/键盘转换、等待期限与 dispatch 请求，不依赖 CPU/ApiState/执行器；Process 执行实际阻塞或 TIMERPROC/WNDPROC，原 sent loop 未改。原 timer/post/filter/output/keyboard、deadline、dispatch 输入和等待/redispatch 主体核对保持。对自 `1155a3e` 后的连续 owner/请求迁移进行阶段整合：release all-targets 692 passed / 7 ignored / 0 failed，21 suites；all-targets/all-features strict Clippy、build 与 fmt 通过。工具未改，前次 31 项结果继续保留；执行循环/frame 未改，不跑性能或游戏矩阵。累计契约仍 915，证据 `wire-out/refactoring-2026-10-14/r4-messages32-{source-checks,verification}.json`。

## R4 PE32 窗口几何与默认位置通知

基线 `c8c735e`。USER 准备 enable/parent/position/update 请求，Process 执行 callback，正常返回后 USER 读取 guest WINDOWPOS 并提交；位置通知保留原过程快照。默认 WM_MOVE/WM_SIZE 分两次准备，后者取得回调后的窗口/过程/style/client。user32_common 只借 User32/内存，删除闲置私有坐标 facade。原 parent、enable precommit、position prepare/commit、update、text/codec 与 callback frame 核对保持；新增过程替换/最小化 fixture 在迁移前通过。release 完整 Win32 258、IR 31、fmt/release lib/bin strict Clippy/build 通过，首次 dead-code 失败日志保留，删除闲置入口后重跑必要构建；不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r4-window-geometry32-{source-checks,verification}.json`。

## R4 PE32 窗口生命周期与同步发送

基线 `2f87673`。USER 准备创建、显示、销毁、发送和默认过程请求，Process 的 window 协调器执行 callback、等待和原 cleanup；内置类注册由组合根协调，class 匹配与持久字符串按明确资源借用。`src/user32/` 内直接 CPU/guest 执行依赖清零，sent 仍封装。创建准备/cleanup、发送完成/入队/wake、show/destroy commit、默认 nonclient/background、class 字符串分配/匹配和 callback frame 核对保持。完整 release Win32 258、IR 31、fmt/release lib/bin strict Clippy/build 通过；字段简写 lint 修正后通过，首次日志保留。不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r4-window-control32-{source-checks,verification}.json`。

## R4 PE32 CRT 执行编排

基线 `790480e`。CRT 的 CPU/FPU 返回、initializer、模块数据与 exit 编排归 Process，qsort 算法原文迁到其私有子模块；普通 strtoul/memory/heap/startup 通过组合根窄借资源。原 provider/CPU、initializer/SEH transfer、exit、scalar 主体与 qsort 全文核对保持，callback frame 未改。新增独立函数表变更/首错误但继续执行 fixture 在迁移前通过。release 完整 Win32 259、IR 31、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r4-crt-process32-{source-checks,verification}.json`。

## R3 Win64 USER cursor/DC/desktop 调用

基线 `af6681c`。三个 adapter 归 User owner，只借内存、tid（需要时）和当前错误槽，不再借 Runtime64；各自纯 owner 与 root frame 不变。原主体经资源替换和 rustfmt 规范化后完全一致；六处内部 tracking 调用改为借 owner，期望值不变。release 完整 Win64 269、library 61 passed / 7 ignored、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r3-user-services64-{source-checks,verification}.json`。

## R3 Win64 class/image 与资源读取

基线 `09a5dfc`。class/image 调用归 User owner，仅借内存、当前错误槽和 image 所需只读 Modules；资源 key/PE tree 读取移除 Runtime64 依赖。原 class/image、resource key/traversal 经资源替换与 rustfmt 对照一致，entry 原文未改；内部 native fixture 的 class 调用只适配签名。release 完整 Win64 269、library 61 passed / 7 ignored、fmt/release lib/bin strict Clippy/build 通过。未改图形 backend，不跑 native/游戏或性能矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r3-user-resources64-{source-checks,verification}.json`。

## R3 Win64 消息与 host USER owner

基线 `c551314`。User 准备普通消息结果、错误与等待，组合根按需写 LastError 或阻塞；队列 ready 和 host 窗口快照/事件入队也归 owner。原消息、host、窗口协调器和 scheduler 对照保持，内部 fixture 原文未改。首次全目标测试抓到错误槽提前读取，改用显式错误结果后通过 release 全目标 694 passed / 7 ignored、21 suites；all-targets/all-features strict Clippy、build 与 fmt 通过。首次失败日志保留。没有修改执行循环、frame 或 backend，不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r3-user-messages64-{source-checks,verification}.json`。

## R3 Win64 键盘与窗口标量处理

基线 `8f5b3d6`。键盘状态/布局和默认过程标量处理归 User，只借内存与只读 Threads；TEB 地址查询归 Threads，错误分支才读取。原 handler、线程地址读取及 window start/resume/退休/frame 检查对照保持；内部调用仅调整资源借用。完整 release Win64 269、library 61 passed / 7 ignored、fmt/release lib/bin strict Clippy/build 通过。不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r3-user-keyboard64-{source-checks,verification}.json`。

## R4 PE32 异常执行与 CONTEXT 适配

基线 `c42be9b`。异常搜索/展开和普通 filter 执行归 Process，CONTEXT 编解码独立只借内存/ThreadCtx；API 直接调用 Process，公开 seh 入口重导出。两个 codec、raise/unwind 主体经调用路径替换后原文一致，callback helper 未改；普通 filter 使用原 caller-context 策略，SEH 保持异常入口上下文恢复和非局部转移。完整 release Win32 259、IR 31、fmt/release lib/bin strict Clippy/build 通过。不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r4-exceptions32-{source-checks,verification}.json`。

## R4 PE32 DLL/TLS 通知执行

基线 `6e13fe9`。TLS、线程和进程退出通知由 Process 执行，Modules 提供私有 metadata 的快照/重查，公开 modules 通知入口重导出。五个执行主体及八个 owner 操作对照保持，link/resolve/initialize/unload/rollback/call 未改；回调顺序、直接 call_guest 的恢复策略和重入后的重查保持。完整 release Win32 259、IR 31、fmt/release lib/bin strict Clippy/build 通过。不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r4-module-notifications32-{source-checks,verification}.json`。

## R4 PE32 DLL 初始化与卸载

基线 `db3c4fe`。Process 执行 sidecar IR、依赖初始化、TLS/DllMain 和卸载后的 CPU IR 移除；Modules 拥有 phase、引用和资源退休的准备/提交。初始化/卸载与四个 owner 操作经拆分对照保持，link/resolve/rollback/call 和通知未改；FALSE attach、部分资源失败及非局部转移的原顺序保持。完整 release Win32 259、IR 31、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r4-module-lifecycle32-{source-checks,verification}.json`。

## R4 PE32 模块装载入口与失败清理整合

基线 `1e6eacc`。typed 模块 API 与 expected/fatal/control 清理归 Process，公开 modules::dispatch 重导出；领域操作管理 disable/pin/query、引用回滚与逐个 image 退休，不再借 CPU/scheduler/logger 或执行 guest。call/dispatch、回滚及 owner 操作对照保持，link/resolve/acquire/abandon/生命周期方法未改。连续 Process/USER 重构阶段整合通过 release 全目标 694 passed / 7 ignored、21 suites，all-targets/all-features strict Clippy、build 与 fmt。原映射/IR 逐个清理、错误和非局部转移顺序保持；不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r4-module-loading32-{source-checks,verification}.json`。

## R3 PE32 链接器明确资源借用

基线 `8707f16`。Linker 在映射/解析期间只借 Modules、ABI、USER/scratch/error 和只读 filesystem/EXE path/cmdline，不再借 ApiState；模块依赖引用/去重/abandon 归 Modules。系统类注册主体归 User32，原 Desktop 兼容入口保持。link/resolve、三个 owner 操作、系统类/字符串和 Process 调用对照保持；生命周期/回滚/TLS 未改。首次编译遗漏兼容入口，恢复后完整 release Win32 259、IR 31、all-targets/all-features strict Clippy、build/fmt 通过；失败日志保留。不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r3-module-linker32-{source-checks,verification}.json`。

## R3 PE32 类调用归属

基线 `9e74aa0`。User32 执行类注册/查询/字段、comctl32 初始化和隐式系统类注册，只借实际内存、gateway、Modules、scratch 和错误槽；ApiState 组合资源，公开 find_class facade 保留。四个调用主体经资源替换后对照一致，持久字符串、查类和系统类注册算法原文保持；Desktop/创建前注册顺序保持。完整 release Win32 259、IR 31、all-targets/all-features strict Clippy、build/fmt 通过。不跑性能或游戏矩阵，累计仍 915。证据 `wire-out/refactoring-2026-10-14/r3-user-classes32-{source-checks,verification}.json`。
