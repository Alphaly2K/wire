# AMD64 解释器优化实施记录

2026-10-09。依据 `wire-out/interpreter-audit-2026-10-09/audit-and-plan.md` 自主实施、测试、采样与测量。实现仍解释 guest 数据，不生成可执行宿主代码。

## 工作边界

- 起点：`783c01b7d0da55bf1d78c3e7e99a4ff2b4adef4a`。
- 隔离工作区：`/Users/alphaly/.codex/worktrees/interpreter-plan/wire`，分支 `codex/interpreter-plan`。
- 独立构建目录：`/private/tmp/wire-interpreter-target`。
- 不修改另一任务的 `tools/mono`、原生 Mono 构建、运行期接入或文档。
- 测量输出：`/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/`。下文输出目录均相对于这里。
- 冻结原始 release 的 SHA-256：`54c0f91ebe181ee0cfc3f8887ea9942ab732651615e8fc228e66cb4494d70ca7`。
- 原版可复现副本在输出目录 `baseline/wire`，最终程序在 `final-verified/wire`；`final-code-attestation.json` 验证 149 个源码/测试/Cargo 文件仍与冻结内容一致。

## 实现范围

| 批次 | 实际实现 |
| --- | --- |
| P0a | 无分配的 GPR/XMM 分类；非法寄存器保留原诊断。独立提交 `41f0946`。 |
| P0b | 独立 `--cpu-diagnostics`：首次翻译、驱逐、依赖页变化后的同字节重译、真实字节变化、执行页写入、fallback Code 与模块来源。私有代码明确标记 `private-or-unknown`。 |
| P1a | 48B 热 PC/epoch/IR 记录，iced 指令和页版本在冷数组。比较了 16K direct、16K 四路、32K/64K/128K direct；布局由编译时选择，不在热路径动态分支。 |
| P1b | 固定 24B IR：移位/旋转、二/三操作数 IMUL、隐式符号扩展、BSWAP、BSF/BSR、非 FP SSE move/transfer/logic/packed integer add/sub/compare。先计算后局部提交，不复制整个 CPU。 |
| P1c | 注册/替换时预绑定 gateway 的符号、支持状态、arity、DWORD 位置、分发组和热 handler；固定参数缓冲，跨调度 Control 拥有参数；常见计时/ID/handle/TLS/LastError 局部提交；最多 4096 个结构化 owned trace records，显示语法保持一致。 |
| P2a | typed STOS/MOVS；普通 step 仍一个 work，预算接口可执行最多 64 元素的安全 REP chunk。限 forward/address64、各自单页、不重叠、无执行历史或 guard 的目的页；其余精确单元素。 |
| P2b | 每页 generation，跨页指令依赖两页；通常命中只比较 validation epoch。NX 执行历史保留，NX 写入也失效；映射/guard 清权限 proof；重映射/merge 新身份；generation 回绕清缓存。 |
| P3 | 同页 4–16 条非故障寄存器段、同页受版本保护的 next/branch links、段内保守 FLAGS liveness。复用原 ScalarIr ISA，保留所有入口，受 budget/UI/quantum/gateway/sentinel/page 边界限制；外部观察时 FLAGS 完整。 |

另实现受检二元内存 RMW 的地址/页槽复用，顺序始终为 READ → 计算 → WRITE → 提交。跨页保留原 all-or-nothing 写入，存储固定为 1/2/4/8 字节；标量故障回滚 CPU。依据访存 profile，内联三个小型受检 helper：`Memory::range`、`Memory::check_page`、`Pages::get_mut`，保留全部范围、权限和 guard 检查。

新增整数操作在普通路径使用独立 handler，保持旧标量热分发紧凑；安全寄存器段仍内联同一份实现。未支持的 MMX、FP、单操作数乘除和其他形式保留原事务 fallback；完整预验证 fallback descriptor 尚未实现。

## 正确性与复核

完整 `cargo test --release`：358 passed，7 ignored（原有 native 平台忽略项）。fmt、strict Clippy all targets、diff check 通过；Clippy 最初的测试 Vec lint 已修，原日志保留。普通受限沙箱中 Unicorn 会 SIGILL，因此参照执行在允许的环境运行；发布程序不使用 Unicorn/JIT。

新增验证覆盖：原事务 fallback 成功/故障差分；独立 Unicorn 高 XMM/alias/packed integer/乘法/隐式扩展；所有宽度 shifts、flags、SSE；RMW 权限/guard/宽度/跨页/回滚；REP 每个 budget、DF/address32/count=0/重叠/guard/部分完成/代码写入；缓存两页依赖、无关写入、NX 写入、保护、重映射、merge、generation wrap；段每个入口、16 位 FLAGS mask、高位依赖、预算和精确 fault；gateway 替换、返回帧/output alias、LastError/TLS/IAT/delay hook/callback 与结构化 trace 所有权。

复核修复了两处实验错误：16 位 BSWAP 原 fallback 拒绝，早期 IR 曾接受；inclusive gateway 查询的快速范围检查曾排除 `start == last`。最终测试包括先执行 16 个 NOP，再链接到已缓存的 singleton gateway，验证仍调用 API。早期二进制保留供追溯，不能作为交付程序。

证明基于当前合作式宿主 CPU 和 Memory 受检借用。Mono prototype 尚未接入 Process；将来若宿主线程异步写 guest 状态，需要重新建立同步、失效与批量访存观察协议。

源码与工具已合入 main：`cdd75f4`，保留另一条路线的 `68d24be`、`59ca3d7` 和最新 handoff `462d838`，没有修改 `tools/mono`、`tests/mono` 或 Mono 文档。Git 非交互 GPG pinentry 无法签名，提交仅对本次命令关闭签名，未改用户 Git 配置。

合入后再次完整 release 回归：358 passed、7 ignored，fmt 与 strict Clippy all targets 通过。[合入后回归](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/integrated-tests.txt)、[合入后 Clippy](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/integrated-clippy.txt)。另在独立输出目录复跑现有 Mono M2 probe：PE → native IL → interpreted x64 callback checksum=19，guest register/stack root 保活后回收均通过，noexec guard `checks=51 denied=0`。[桥接与 GC 回归](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/native-mono-m2-regression/results.json)。这只验证现有原型，没有接入真实游戏原生后端、共享 backing 或 unsafe IL。

## 测量与被拒绝候选

冻结二进制/源码/构建配置/hash；正式计时关闭 stats/sample，至少三轮 AB/BA，串行运行并校验 checksum、精确 retired/work/API calls。统计和采样独立；保存全部噪声、失败和拒绝轮次，不用最快轮次代替中位数。真实 host timer 会改变轮询、调度和 guest counts，同 endpoint 不等于逐指令相同路径。

独立 P0a transfer（100M work）wall 中位数 2.390 → 1.463 秒，正常 xmm_index/format 热栈消失。独立同缓存 P1b（100M work）：SSE register 3.083 → 1.370、transfer 1.529 → 0.761、shifts 2.253 → 0.752、mixed 2.579 → 1.236 秒。它们用于批次归因，不能当作最终 combined 收益。

独立 P1c 保持旧 16K cache、旧 IR 和普通逐条循环，仅加入 gateway binding/参数缓冲/结构化 trace。完整回归 339 passed、7 ignored。100M work 的三对 CPU 中位数 22.712 → 1.467 秒，wall 23.222 → 1.474 秒，所有返回值、retired/work/API 数一致。独立采样中重复 arity 分类、trace 格式化栈消失；采样与编译重叠，不能当作计时。原版分类 inclusive 51.07%、format 16.23%，分母是 Process64::run 的样本；这些项重叠，不能相加。证据在 `gateway-only-timing/` 和 `gateway-only-profile/`。

runtime layout 分支、大型 hit 返回 tuple、把页版本放热记录、未内联权限/budget wrapper 均出现标量回退，已撤销/移至冷路径。四路命中/替换成本不合适，选择 direct；八条段会切开九条整数循环，改为十六条并过滤短段。全部新增整数统一移出热分发改善访存但拖慢移位，改为仅普通路径独立 handler。RMW 关闭对照未改善访存。

`p3-rmw-memory-timing` 被外部两个 rustc 严重干扰，不能验收，归因保存在 `contamination.json`。其他实验存在 mediaanalysisd、系统索引与明显 CPU/wall 波动，保留 load snapshots 和所有轮次；小差异不能自动解释为收益。

| 同一 SRW 终点的独立统计 | translations | retired | invalidations |
| --- | ---: | ---: | ---: |
| 32K / diagnostics | 212,201,334 | 5,025,636,611 | 422,607 |
| 64K / stats | 114,367,040 | 4,800,142,269 | 422,607 |
| 128K / stats | 55,089,605 | 4,642,511,155 | 422,607 |

32K 页版本诊断的同字节重译为 323,205，仍有 210,640,270 次驱逐。旧诊断按全局 epoch 归因，会把无关页写入后的驱逐算为重用；最终按指令两页依赖版本分类，因此 cause 百分比不能严格等口径比较，翻译总数不受修正影响。64K stats 与构建重叠，耗时不用作验收。128K 因真实翻译次数减少而进入最终验收，同时测量 RSS。

## 最终冻结与对照

最终采用 `final-verified/`；manifest 包含全部源文件 hash、源码 diff、构建配置与二进制 SHA-256：`16dd112306cd0b2fe2b68d4f2c1a5b8befde2abc9827abd6726349278e7f8be0`。源码实现提交为 `41f0946` 与 `cc76ecb`；冻结后执行源码未改变，本文结果在测量后补入。最终完整测试、strict Clippy 和构建日志为 `final-verified-{tests,clippy,build}.txt`。

最终十类 100M work 微基准三对 AB/BA，全部 60 次运行通过返回值及精确 retired/work/API 校验。正式计时不采样、不计 stats、没有自己的并行构建。CPU 与 wall 中位数如下（秒）；全部轮次、RSS、输入/hash 和命令在 `final-verified-timing/compare.json` 与 manifest。

| 示例 | CPU 原版 → 最终 | wall 原版 → 最终 |
| --- | ---: | ---: |
| integer | 0.651 → 0.291 | 0.652 → 0.293 |
| memory | 0.937 → 0.915 | 0.938 → 0.917 |
| simd-mixed | 2.754 → 1.085 | 2.756 → 1.087 |
| large-code | 2.279 → 0.268 | 2.281 → 0.270 |
| indirect-call | 0.685 → 0.667 | 0.687 → 0.669 |
| sse-register | 2.988 → 1.458 | 2.990 → 1.460 |
| sse-gpr-transfer | 2.397 → 0.563 | 2.400 → 0.564 |
| shift-register | 2.221 → 0.320 | 2.223 → 0.322 |
| rep-memory | 3.097 → 0.142 | 3.100 → 0.144 |
| gateway-thread-id | 13.662 → 1.053 | 13.674 → 1.054 |

小示例 RSS 中位数约 11.2 → 22.1 MB，large-code 12.0 → 25.8 MB，gateway 12.5 → 23.3 MB（十进制）。其中 integer 首次最终运行 wall 1.670、CPU 0.310 秒，保留在全部轮次中；不挑最低值。memory/indirect 这轮只有约 2%–3% 改善，前面的长运行存在明显波动，不能据此承诺稳定小收益。

旧 `final-candidate/` 未含最后三个 helper 内联属性，300M 全矩阵在 `final-timing/`；它的 memory CPU 中位数曾从 3.431 升到 4.411 秒，原版自身范围 2.757–4.366 秒。随后 500M、utility QoS 的三版本对照（`rmw-helper-timing/`）memory CPU 原版/旧候选/helper 为 7.126/7.461/6.669 秒，每轮 helper 都比旧候选改善。indirect 为 5.555/5.144/5.770 秒，且候选范围很宽，不能宣称所有负载稳定加速。QoS clamp 只统一调度类别，不能固定 core/频率；正式最终十类矩阵未使用 clamp。

真实 Alice 启动三对严格串行，顺序 AB/BA/AB，每轮重置相同 Users seed，使用同一 EXE、UnityPlayer、Mono、graphics backend 和 MoltenVK。没有自己的并行构建或采样；系统负载快照保留。全部六次在 `thread 29 entry`、RIP `0x7ffe000007c0` 到达相同 SRW 错误，非 Users 输入文件变化均为空。

| 对照轮 | wall 原版 → 最终（秒） | CPU 原版 → 最终（秒） | 峰值 RSS 原版 → 最终（MB，十进制） |
| --- | ---: | ---: | ---: |
| 0，AB | 108.867 → 75.622 | 106.165 → 73.244 | 484.9 → 517.1 |
| 1，BA | 103.745 → 79.563 | 101.954 → 77.696 | 496.3 → 503.5 |
| 2，AB | 110.150 → 72.564 | 108.366 → 70.872 | 490.4 → 511.3 |
| 中位数 | 108.867 → 75.622 | 106.165 → 73.244 | 490.4 → 511.3 |

wall 减少 **30.5%**，process CPU 减少 **31.0%**；峰值 RSS 中位数增加 20,922,368 字节（约 4.3%）。retired/work/API 中位数分别为原版 4,644,123,413 / 4,652,501,343 / 7,707,037，最终 4,586,041,647 / 4,594,345,853 / 7,639,588。真实 host timer 会改变轮询与调度，不能要求游戏计数相同或把少量计数差异当作漏执行；确定性微基准要求并通过精确计数相同。六份 Player.log 仅替换程序集加载、域重置、UnloadTime 三处耗时后内容完全一致。

共同终点仍为 `kernel32.dll!SleepConditionVariableSRW` 未实现，仅衡量这个启动前缀。没有标题/可玩/帧时间验收；draw/present 数（383–421 / 138–150）不等于 FPS。该问题解除后需要新的可玩路径基线，不能把这里的收益直接外推到帧率。[全部六次计时](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-alice-timing/compare.json)、[终点/日志/输入验收](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-alice-timing/acceptance.json)、[真实输入和命令 manifest](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-alice-timing/manifest.json)。

## 最终热点与后续边界

最终冻结版本独立从第 10 秒起采样 60 秒，sample 成功、同一 SRW 终点和输入检查通过；未与自己的编译或正式计时重叠。Process64::run 的 leaf-weighted 分母为 44,737 个样本。`Process64::run` self 49.3%，其中包含内联 IR、缓存查询、权限与 FLAGS 处理，不能全称作外层调度。inclusive fallback 19.08%、decode 7.36%、checked read_int 6.12%、gateway bound dispatch 4.14%；iced decode union 4.30%、lower 2.27%、直接位于 fallback 下的复制 1.96%。inclusive 项相互重叠，不能相加；这个窗口与旧 profile 的 guest 相位不完全相同，比例不能当作绝对时间降幅。

[完整热点汇总与火焰图](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-alice-profile/analysis/summary.json)、[具体 decode/复制归因](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-alice-profile/audit-groups-summary.json)、[交互 hover 火焰图](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-alice-profile/analysis/flame.svg)。

最终独立 diagnostics 也到达同一终点，且游戏输入未变。IR work 4,523,952,312、fallback 119,097,931，fallback 占已记录 guest work 2.565%；translations 69,896,398、invalidations 422,607。翻译原因 cold 1,231,591、evicted 68,335,409、依赖页同字节重译 323,155、真实字节变化 6,243，四类之和与翻译总数严格一致。804 个执行历史页共记录 422,587 个页写入事件，其中 376,211 个改变字节；跨页写入分别计入两个页。work 还包含 API 调度/等待等轮次，不能直接与 IR/fallback 计数等同。这个诊断运行与合入后编译重叠，耗时和 RSS 均不用于性能验收。

剩余 fallback 前列为 MULSS 15.48M、MULSD 14.81M、ADDSD 12.26M、CVTSS2SD 11.34M、CVTTSS2SI 9.99M、CVTSD2SS 8.99M；BTR/BTS/BT 和乘除也仍回退。按来源有 UnityPlayer 41.06M、Mono DLL 54.10M，其余私有代码保持未知归因。不能把 DLL 区间或私有页计数当成 Mono 方法计数。[最终诊断汇总](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-alice-diagnostics/summary.json)、[原始诊断](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-alice-diagnostics/pair-0-final/report.txt)。

P0–P2 的上述实现与 P3 的安全首版已完成验收。仍存在缓存驱逐，128K 不是完全消除 miss 的方案；未实现完整预验证 fallback descriptor、跨页/内存块、全局 deferred FLAGS 或持久 AOT。继续扩大这些机制需要新的 fault/观察点证明，不能仅按 opcode 频率跳过事务或改用宿主浮点。当前完整游戏路径受 SRW 未实现阻断，真实 Mono 共享 backing/线程协议也尚未建立；本轮在已验证的范围收敛，保留诊断、冻结程序和测量工具供下一阶段继续。

## 复现与范围边界

主要可复核证据：[最终冻结 manifest](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-verified/manifest.json)、[冻结后源码一致性](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-code-attestation.json)、[最终完整回归](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-verified-tests.txt)、[严格 Clippy](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-verified-clippy.txt)、[最终微基准全部轮次](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/final-verified-timing/compare.json)、[P1c 独立对照](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/gateway-only-timing/compare.json)、[RMW helper 三版本对照](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/rmw-helper-timing/compare.json)。

`tools/freeze_interpreter.py` 保存源码和程序；`tools/interpreter_perf.py` 生成原创 PE 并比较；`tools/interpreter_game_perf.py` 用带 `.wire-interpreter-replay` 标记的隔离 runtime，每轮重置 Users seed，校验所有非 Users 游戏文件，保存 Player.log/report/CPU/RSS/counts。graphics backend/MoltenVK hash 在 manifest 中。

```sh
CARGO_TARGET_DIR=/private/tmp/wire-interpreter-target cargo test --release
CARGO_TARGET_DIR=/private/tmp/wire-interpreter-target cargo clippy --release --all-targets -- -D warnings
python3 tools/freeze_interpreter.py --binary /private/tmp/wire-interpreter-target/release/wire --out FREEZE_DIR
python3 tools/interpreter_perf.py --binary before=BEFORE --binary after=AFTER --out OUTPUT --work 300000000 --pairs 3
```

十类矩阵需显式加 `--cases integer memory simd-mixed large-code indirect-call sse-register sse-gpr-transfer shift-register rep-memory gateway-thread-id`。真实场景复现：

```sh
python3 tools/interpreter_game_perf.py \
  --binary before=BEFORE --binary final=FINAL \
  --runtime /private/tmp/wire-interpreter-alice \
  --users-seed /Users/alphaly/Projects/wire/wire-out/interpreter-implementation-2026-10-09/alice-users-seed \
  --out NEW_OUTPUT --pairs 3 --mode timing \
  --graphics-backend /Users/alphaly/Projects/wire/wire-out/libwire-graphics-resource-perf-2026-10-09.dylib \
  --moltenvk /private/tmp/wire-moltenvk-1.4.2/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib
```

`--mode diagnostics`/`stats`/`profile` 必须另起输出目录，并与正式计时串行；profile 默认从第 10 秒起采样 60 秒。runtime 是由原游戏复制的隔离 APFS clone，不能使用目录 symlink，GuestFs 会校验 canonical root。

默认 direct128K。实验需独立构建：`WIRE_X64_CACHE_16K=1`、`WIRE_X64_CACHE_32K=1`、`WIRE_X64_CACHE_64K=1`、`WIRE_X64_CACHE_FOUR_WAY=1`。`--translation-cache` 仅验证已编译布局，不能重解释已初始化缓存；关闭段/links/RMW 的开关只用于编译时对照。

P3 限于同页寄存器段和段内 dead FLAGS；不包含内存块、跨页链接、全局 deferred FLAGS recipe 或持久 AOT。更多 FP IR 需要独立验证 APFloat、rounding、NaN、FTZ/DAZ、MXCSR sticky/#XM，不以宿主 f64 替代。剩余热点依据最终真实 profile 排序；SRW 兼容性解除后必须重建可玩路径的性能基线。
