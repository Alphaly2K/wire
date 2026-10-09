# 解释器 V2 实施与验收（2026-10-09）

本轮以 `b25c4dc` 主线和上一轮最终冻结二进制为基线，独立推进 AMD64 V2 计划，并扩展 PE32。最终采用 `ac13119` 的独占寄存器段容器和 Arc 受检内存段；核心 V2 与 PE32 优化进入默认配置。真实启动三对 CPU 全部改善，中位数 71.015→65.985 秒（约 -7.1%），所有失败候选保留。代码在 `codex/interpreter-v2` 隔离 worktree 中开发；没有编辑其他 agent 的 Mono checkout。正确性优先：所有候选先验证指令语义、预算和故障边界，再进行关闭计数/采样的冻结二进制串行 AB/BA 测量。

## 实现范围

- V2-0：编译期隔离的段覆盖、链接、翻译请求与数据访存诊断。保留四个 262,144 项有序请求窗口，含 hit、segment/link access 和 eviction；窗口外不做昂贵的历史查询。诊断构建不能作为正式性能数据。
- V2-1：8/16/32/64 位固定 lane 的 packed integer 运算，以及精确 4/8/16B 源读取。热 packed 路径不再逐 lane 做三次变长 memcpy。
- V2-2：标量 FP、转换、COMI/UCOMI、sqrt/predicate 的 typed IR，继续使用原有 APFloat 语义；只保存 MXCSR/FLAGS 等小状态并提交实际目的寄存器。源内存故障不提交 FP 状态，#XM 只提交 sticky MXCSR。BT/BTS/BTR/BTC 独立降低，保持 signed memory offset、立即数规则、LOCK 和受检 RMW。
- V2-3：保留直接映射 128K 热缓存，miss 时使用独立 hash 的 4096 项有界 victim store，复用 iced/IR/段；两页 generation、条目身份和链接版本共同防止旧条目与 ABA。权限/guard proof 独立复核。保存链接时防止目标 promotion 把源槽驱逐后写错链接。
- V2-4：AMD64 整数与非 FP SSE 混合寄存器段复用同一语义 handler；只在完整无故障段中合并 RIP 更新。短受检内存段逐条执行同一个 scalar rollback/commit，不跨潜在故障删除 FLAGS；当前访存仅接纳单页、自有且从未执行过的数据页，代码页、跨页和 shared backing 退出到普通路径。cache/page 借用在访存前释放，不缓存裸指针。受检段默认启用，只接纳严格的 owned 数据页子集，`WIRE_X64_NO_CHECKED_SEGMENTS=1` 可编译关闭。
- V2-5：先采用 Jcc/CMOV/SETcc 的精确 FLAGS 读取掩码，保留 ADC/SBB 的 CF、部分写入及运行期 shift 依赖。本轮不引入跨异常/上下文边界的全局 deferred recipe。

PE32 保持 `.wir v3` 格式不变：绑定验证原字节后添加进程内 SSE/bit descriptor，并支持安全混合寄存器段。FP 共用精确 APFloat helper；非观察指令不提前 materialize lazy FLAGS，COMI 成功时才丢弃旧 recipe。raw descriptor cache 为 Memory 所有，256 项、完整 PC 与两页代码版本；warm hit 仍顺序 fetch 字节检查当前 execute/guard。更换 Memory、代码写入、DEP 和 guard 均有回归。混合段排除 FP、内存和 ESP 写入。

## 最终冻结性能与源码核对

最终 binary SHA-256：`053c62bc5da7a301392e6caf4fdfb5a332eff5b98e0e03e970e5bed290fedc8b`。[160 项编译源码与测试核对](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-v2-2026-10-09/accepted-source-attestation.json) 全部一致，外部游戏/graphics/MoltenVK 输入也未变化。实现报告在冻结后完善，未算作编译输入。最终普通构建关闭 V2 counters 与采样。

| 真实三对 | before CPU | 最终 CPU | CPU 变化 |
| --- | ---: | ---: | ---: |
| 1 | 73.178s | 69.985s | -4.4% |
| 2 | 71.015s | 65.985s | -7.1% |
| 3 | 70.742s | 63.848s | -9.7% |

CPU 中位数 71.015→65.985 秒（-7.1%），wall 72.724→67.791 秒（-6.8%）。三对 guest work 对应减少约 0.17%、0.20%、0.30%；结果代表相同终点的启动前缀，不是逐指令固定路径。真实 peak RSS 中位数 +16.1MiB，含真实路径分配波动；integer 小进程 RSS +1.06MiB。机器码 `__text` 为 2,880,512→2,955,596B（+2.6%）。

最终 100M work、三对 AB/BA 的 CPU 中位数：

| AMD64 用例 | V1 before | 最终 | 变化 |
| --- | ---: | ---: | ---: |
| integer | 0.2806s | 0.2787s | -0.7% |
| memory | 0.9255s | 0.8676s | -6.3% |
| simd-mixed | 1.2154s | 0.7039s | -42.1% |
| large-code | 0.2675s | 0.2728s | +2.0% |
| indirect-call | 0.6602s | 0.6331s | -4.1% |
| sse-register | 1.4579s | 0.3824s | -73.8% |
| shift-register | 0.3222s | 0.3357s | +4.2% |
| rep-memory | 0.1399s | 0.1408s | +0.6% |
| gateway-thread-id | 1.0554s | 1.0990s | +4.1% |
| fp-scalar | 4.3896s | 1.9266s | -56.1% |
| bit-test | 2.4613s | 0.2790s | -88.7% |
| code-alias | 4.2418s | 2.1993s | -48.2% |
| code-control | 0.9394s | 0.9446s | +0.6% |

保留全部轮次与回退：large-code 约 +2%、shift 与 gateway 约 +4%；REP/control 约持平。未要求每个 synthetic 都变快；最终选择同时依据完整正确性、主要覆盖收益和三对真实出口。integer 第一对 owned 有明显调度等待，control 第一对 before 波动较大，均未剔除。

| PE32 用例/路径 | V1 before | 最终 | 变化 |
| --- | ---: | ---: | ---: |
| sse-register/ir | 2.0753s | 0.7115s | -65.7% |
| sse-register/raw | 2.4641s | 2.3782s | -3.5% |
| fp-scalar/ir | 1.9419s | 1.7094s | -12.0% |
| fp-scalar/raw | 2.3648s | 2.9328s | +24.0% |
| bit-test/ir | 1.7139s | 0.5529s | -67.7% |
| bit-test/raw | 2.1081s | 2.0552s | -2.5% |
| integer/ir | 0.3441s | 0.3276s | -4.8% |
| integer/raw | 2.0074s | 1.8459s | -8.0% |

PE32 raw 标量 FP 的 +24.0% 是使用精确 APFloat、MXCSR 和异常提交的成本。保留该正确性改进，并明确列出回退；静态 FP IR、SSE/bit 与 raw integer 的收益没有用来掩盖它。

最终原始数据：[真实全部轮次](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-v2-2026-10-09/owned-real/compare.json)、[AMD64 主要负载](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-v2-2026-10-09/owned-micro/compare.json)、[FP/bit/cache](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-v2-2026-10-09/owned-v2-micro/compare.json)、[PE32](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-v2-2026-10-09/owned-pe32/compare.json)、[汇总](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-v2-2026-10-09/accepted-summary.json)。

## 最终独立诊断与采样

与最终源码一致的独立诊断记录 IR 4,895,338,604、fallback 18,597,563；fallback 占已记录 IR+fallback work 约 0.378%，该分母不含所有 API/等待调度 work。translations 15,689,295，reuse promotions 25,667,088，register segment work 661,504,285，checked segment work 1,469,594,175。段 work 合计占上述记录约 43.4%。V1 独立诊断 translations 为 69,896,398，但两次真实 work 和调度不同，不能把差值当作相同请求流上的精确缓存收益或 CPU 倍率。

数据访问记录精确 1/2/4/8/16B、cross-page、recent-cache hit/miss。正式 binary 不含 V2 计数热路径；诊断与编译允许重叠，诊断 wall/CPU/RSS 全部排除于性能统计。原始四个窗口和有界模拟结果完整保留，没有据小窗口宣称解决所有容量/布局问题。

最终独立 profile 从第 10 秒采样 60 秒，包含 49,633 个 Process64::run 栈样本：decode/segment-build 并集 4.03%，fallback inclusive 4.84%，fallback 下直接复制 0.19%，APFloat 相关 2.64%，typed-vector 1.70%。相比 V1 的 10.24%/19.08%/1.96%，可观察到这些栈权重下降；不同采样窗口与内联归因限制仍在，百分比不是绝对 CPU 降幅。profile 的时间不用于三对正式结果。

[诊断原始报告](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-v2-2026-10-09/owned-real-diagnostics/pair-0-owned/report.txt)、[采样与归因](/Users/alphaly/Projects/wire/wire-out/interpreter-implementation-v2-2026-10-09/owned-real-profile/analysis.json)。

## 分批证据与保留的失败记录

所有原始输入、manifest、source snapshot、日志和全部计时轮次位于 `wire-out/interpreter-implementation-v2-2026-10-09/`。before 的 SHA-256 为 `16dd112306cd0b2fe2b68d4f2c1a5b8befde2abc9827abd6726349278e7f8be0`，不是上一轮优化前的 `783c01b`。

固定 lanes 三对 100M work：SSE-register CPU 中位数 1.4815→0.8899 秒，SIMD-mixed 1.1184→0.8080 秒；integer/memory 接近原水平。对应真实三对 lanes 收益不显著，全部波动保留。ARM64 反汇编确认热 packed 循环不再调用三个动态小拷贝，剩余 memcpy 属于错误格式化路径。

FP/bit 三对 100M work：FP CPU 4.4041→1.9388 秒；bit 2.4582→0.2695 秒。真实三对 FP/bit CPU 都改善，但 host 波动很大：lanes 为 73.905、92.696、78.747 秒，FP/bit 为 67.799、76.828、67.717 秒。一次 graphics 路径拼错的 setup 失败保留，不纳入统计。

V2-0 离线模拟以真实有序请求窗口为依据。1B 窗口 warm half 无复用 decode 14,602，独立 4K store 为 14,070，更大 16K/32K 为 14,068，收益不足以证明应扩大预算。因此只实现 4K。诊断运行与编译曾重叠，不能用于时间对照。

V2-3 alias 测试 20 个入口、20,000 次执行只翻译 20 次。三对 100M work 冲突用例 CPU 中位数 4.3308→1.9259 秒，control 0.9321→0.9368 秒。独立真实三对 CPU：before 67.650、66.688、71.902；reuse 68.538、62.429、66.505 秒。中位数约改善 1.7%，两对改善、一对略慢，不能声称大幅真实收益。RSS 中位数约增加 10.7MB，包含进程真实路径分配波动。

PE32 的初版 raw descriptor 每步重新 decode，导致 2–3 倍回退，已改为 Memory 所有的 bounded cache。修复后 30M work static SSE/bit 显著改善，但 raw scalar FP 比旧宿主 FP 慢约 24%；这是精确 APFloat 路径的实际成本，不能通过退回近似计算掩盖。后续 lazy FLAGS 与 mixed loop 改进另行测量。

混合段初版 PE32 integer/IR 在 100M work 三对中稳定回退约 15%，因此把混合路径移到单独入口，并用既有热条目的一个空闲位记录类型，纯整数段恢复原循环。初版及修正后数据均保留。

一次内存段冻结未先确认构建结束，初始计时也与编译重叠，相关 `*-invalid-freeze` 目录及说明保留且全部排除。重新确认构建完成后冻结并重测。

## 完整候选的微基准

三对 100M work 的 CPU 中位数，`v2-5-micro` 候选包含受检内存段，后续未被选为最终默认配置：

| 用例 | V1 final before | 完整候选 | 变化 |
| --- | ---: | ---: | ---: |
| integer | 0.2834s | 0.2970s | +4.8% |
| memory | 0.9251s | 0.8623s | -6.8% |
| simd-mixed | 1.0888s | 0.6809s | -37.5% |
| large-code | 0.2693s | 0.2935s | +9.0% |
| indirect-call | 0.6594s | 0.6316s | -4.2% |
| sse-register | 1.4676s | 0.3735s | -74.6% |
| shift-register | 0.3299s | 0.3637s | +10.3% |
| rep-memory | 0.1431s | 0.1442s | +0.7% |
| gateway-thread-id | 1.5990s | 1.6676s | +4.3% |
| fp-scalar | 4.4901s | 1.9192s | -57.3% |
| bit-test | 2.4617s | 0.2821s | -88.5% |
| code-alias | 4.1967s | 2.2084s | -47.4% |
| code-control | 0.9105s | 0.9267s | +1.8% |

关闭计数/采样，保留全部轮次。对照 refined 的独立 FLAGS mask 批次，integer CPU 0.3089→0.2970 秒；未引入全局 recipe。

## 最终选择与保留的失败

common payload 候选 `dd3758b` 通过正确性测试，但 `final-real` 三对 CPU 分别为 75.928、81.162、84.190 秒，同期 before 为 73.275、72.040、72.877 秒。三对全部变慢，中位数 +11.4%，不能以背景负载解释后直接合入。`60f9d1d` 撤回该变更；`f1c084c` 把受检内存段改为编译期实验，默认消除其执行入口。早期 `overall-v2-real` 的约 9.2% 中位数改善不能替代最终候选验收。

8B Arc owner 的候选也未通过后续真实性能验收，最终不保留该压缩容器。首版 Rc 曾破坏 Memory: Send，默认回归通过但 Mono 线程移动测试编译失败；改为不可变 Arc，并新增带热翻译/段缓存跨宿主线程移动、返回后继续执行的回归。失败日志保留。默认与实验配置的测试按完整 guest work 验证，不假定一次调用恰好执行四条。

默认收缩后的 `selected-micro` 仍显示纯整数 +6.2%、大代码 +10.4%、移位 +13.3%，因此未直接合入。`9ef6c4f` 在冷构建阶段确定段是否混合向量，纯标量段恢复专用循环；逐条分类仅留在 mixed loop。两条循环复用相同 ISA handler、FLAGS mask 和 RIP/预算提交。`separated-default` 与 `separated-checked` 已独立冻结并对照，该批真实结果中 default CPU 为 106.430、80.727、111.274 秒；checked 为 84.123、96.454、76.045 秒；before 为 77.564、80.709、81.985 秒。中位数分别回退 31.9% 和 4.2%，两个候选均拒绝。

`ac13119` 恢复此前 `8598896` 的显式段状态：寄存器段由 Box 独占，受检段由 Arc 共享，保留 Memory: Send 与 cache/page 借用释放。已恢复的纯标量专用循环、4K reuse 和 IR 负构建准入位继续保留。该版本默认启用受检段，`WIRE_X64_NO_CHECKED_SEGMENTS=1` 可编译关闭；`owned-real` 三对 CPU 为 69.985、65.985、63.848 秒，同期 before 为 73.178、71.015、70.742 秒，三对全部改善，因此选用该 exact build。压缩段容器的收益未获验证，最终恢复显式 16B 状态，不保留 8B owner。

回退版本独立 profile 已保留；其约 49,440 个 Process64::run 栈样本中 decode/segment-build 并集 7.94%、fallback inclusive 8.20%、fallback 下直接复制 0.19%。采样运行本身 CPU 68.55 秒，明显低于同 binary 的正式轮次，进一步说明宿主/采样条件存在差异；不把采样时间当正式性能验收，也不以这份 profile 推翻已保留的失败轮次。

## 最终正确性与 Mono 集成

`owned-default-tests.txt`：`cargo test --release --all-targets` 373 passed、7 ignored；`owned-clippy.txt` 无警告，fmt 检查通过。包括 Unicorn、FP 状态差分、全部 budget/interior entries、guard/permissions/remap/code write、lazy FLAGS、每条故障提交与跨宿主线程移动。既有需要实际 Metal/native graphics 资源的 7 个 ignored 测试保留，不声称已执行。

独立集成固定 Mono 提交 `1e9918728e9360aee876bd17fef538255cc4a02c`，初始合流 `9936145`，最终快照 `8d43da6`。唯一冲突是交接文档，保留 Mono 版本，运行时代码自动合并。新增 shared backing 段退出、guard、撤销 lease 测试；最终 `owned-mono-memory-tests.txt` 为 391 passed、7 ignored，`owned-mono-backend-tests.txt` 为 413 passed、7 ignored，均默认启用受检段。之前关闭/实验配置的各批测试与失败日志同样保留。

集成快照不包含其他 agent 的未提交改动，不用于正式性能或后端准入。只读复核其 CPU/Memory/shared-memory/Cargo 接口相对固定点没有新变化；持续开发的 startup/services 等不在本轮验证范围。没有合入该 Mono 分支的代码。

## 被拒绝候选的冻结对照

三方三对、每例 100M work、CPU 中位数。default 为内存段关闭的 `separated-default`，checked 为内存段启用的 `separated-checked`；均已恢复专用标量循环。

| 用例 | V1 before | default | checked | checked 相对 V1 |
| --- | ---: | ---: | ---: | ---: |
| integer | 0.2878s | 0.2798s | 0.2801s | -2.7% |
| memory | 0.9327s | 0.9334s | 0.8383s | -10.1% |
| simd-mixed | 1.0939s | 0.7015s | 0.6995s | -36.1% |
| large-code | 0.2671s | 0.2646s | 0.2621s | -1.9% |
| indirect-call | 0.6739s | 0.7010s | 0.6270s | -7.0% |
| sse-register | 1.4939s | 0.3969s | 0.3917s | -73.8% |
| shift-register | 0.3564s | 0.3737s | 0.3588s | +0.7% |
| rep-memory | 0.1411s | 0.1409s | 0.1416s | +0.4% |
| gateway-thread-id | 1.0791s | 1.0795s | 1.1167s | +3.5% |
| fp-scalar | 4.4252s | 1.9579s | 1.9540s | -55.8% |
| bit-test | 2.4916s | 0.2971s | 0.2990s | -88.0% |
| code-alias | 4.2406s | 2.0619s | 2.0077s | -52.7% |
| code-control | 0.9155s | 0.9195s | 0.9191s | +0.4% |

integer 第一对 default/checked 均出现明显调度等待，保留在完整轮次中；全三对取中位数，不剔除该轮。此前 `selected-*` 的高背景负载与全部数据同样保留。微基准不能代替真实启动出口，该批真实对照未通过，两个候选均保留为失败记录。

## 边界

真实场景仅为原解释器后端的 Unity 启动前缀，仍止于 `kernel32.dll!SleepConditionVariableSRW` 未实现。真实 retired/work/API 数存在调度差异；检查相同文件、Users seed、非 Users 输入无修改及相同正确终点，但不把它称为逐指令相同路径、完整加载、可玩或 FPS 测量。

宿主还有系统后台任务；正式计时没有与本任务编译、采样、诊断重叠，但无法保证整机没有其他负载。全部轮次用于中位数，明显噪声与失败不丢弃。后续 packed FP、多操作数乘除、全局 FLAGS recipe、shared backing 段内访问和更大的缓存需独立证据，不是本轮已完成的功能。
