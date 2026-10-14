# 渐进重构实施记录

按用户提供的 2026-10-11 计划实施，原生 Mono/R6 忽略。用户要求直接在本地 main 分批提交；不创建 worktree，不 push。用户未跟踪设计文档、PDF、IDE 与系统文件保持现状。

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
| R2 分发收敛 | GUID + 6 个 identity + 8 个 version + 42 个 NLS export 已迁移；file/module 等仍 legacy |
| R3 状态归属 | identity/profile 与可变环境变量已形成独立边界；module/file/thread/COM 待迁移 |
| R4 PE32 Process | 已完成装载/执行编排提取；旧 Cpu 方法只保留委托 facade |
| R5 host/graphics | 已完成 owned UI 请求/事件与 GDI adapter 分离，真实 native 验证通过 |
| R6 原生 Mono | 按用户指示跳过 |
| R7 工具/结构 | 先提取测试 harness，旧入口和脚本兼容保留 |

## 测试 harness 提取

PE32 `Vm` 与固定内存布局移入 `tests/support/x86.rs`；PE64 原创 builder、startup 和 Win64 调用 frame helper 移入 `tests/support/x64.rs`。原 integration target 和测试函数名保持不变；全部 389 个测试名逐名一致。Win32/AMD64 239 项行为测试通过，两个 target 的 strict Clippy 和 fmt 通过。没有变更 runtime 代码。

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
