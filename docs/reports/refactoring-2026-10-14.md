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
| R2 分发收敛 | GUID + 6 个 identity export 已迁移；locale/version/file 等仍 legacy |
| R3 状态归属 | identity/profile 与可变环境变量已形成独立边界；module/file/thread/COM 待迁移 |
| R4 PE32 Process | 待调用边界回归稳定 |
| R5 host/graphics | 待 Process/状态边界形成 |
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
