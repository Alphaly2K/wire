# Release 编译单元候选

状态：完整调查后拒绝，Cargo 配置已撤回。

目录枚举契约后的路径/句柄、网关边界、helper 布局及文件 section 候选均出现与局部 API 变更不成比例的 CPU 负载变化。文件 section 首版五个 PE32 执行器的指令数和 opcode 序列均一致，但 raw 整数两个规模的 CPU 中位数仍超过 5%；同一基线 control 也显示明显宿主波动。完整原始数据继续保留于各批报告。

本候选以 `551aff9` 的运行时代码为基础，仅将 Cargo `[profile.release]` 的 `codegen-units` 显式设为 1。八项 section 契约以及此前拒绝的运行时代码均已撤回。目的是独立检查跨模块优化和编译单元划分对解释器代码生成的影响，不预先假定单单元必然更快或能消除地址布局变化。debug profile、guest ISA/FLAGS/fault/预算、IR 格式、cache、Win32 行为与原始 guest Mono 均保持原实现。

本候选先独立完成正确性、native GPU、strict Clippy、冻结二进制及宽性能矩阵。位操作的顺序平衡长样本触发持续回退拒绝条件，因此停止没有必要的 Alice smoke，不将此配置作为后续 API 重构基线。

## 当前验证

完整 release 429 passed / 7 ignored、debug Win32/AMD64/FPU 277 passed、显式 native GPU/Metal 7 passed；fmt、strict all-target/all-feature Clippy、release build 和工具 12 项通过。冻结候选为 `wire-out/refactoring-2026-10-14/release-cgu1-wire/wire`，SHA256 `f124c83d47e6503bae78944fa0fa66f4b9988a59ceaaf6c37506809ea6863846`，5,076,856 bytes，相对原 before 的 5,927,064 bytes 减少约 14.34%。manifest 显式记录 `release codegen-units=1; default interpreter caches`，保留源代码、工具链与所有输入哈希。

700M PE32 raw 整数三组 before/after/control 的返回值和 retired 全部校验；CPU 中位数 −2.829%。首组同基线 before/control 为 15.490/18.784 秒，宿主波动及其 +17.69% 单对离散保留。原始结果位于 `release-cgu1-sensitive-raw/`。

另外三项 700M 三标签敏感路径：AMD64 memory 的三对分别为 −6.30%/−7.84%/−5.82%，CPU 中位数 −6.917%；thread-id 中位数 +2.540%；PE32 SSE/IR 中位数 −16.539%，其首对 +20.86% 与后两对 −6.82%/−16.54% 的明显离散全部保留。

## 宽矩阵与扩大样本

45 条 case/path 全部完成三对初筛，work/retired/checksum/API count 均通过。保留 raw/IR、架构、work 和 QoS 的区别；计时始终与 Cargo、采样及 runtime diagnostics 串行。初筛记录为 `release-cgu1-initial-metrics.json`。PE32 integer/IR、FP/IR、bit/IR 的 CPU 中位数分别为 −33.944%/−23.772%/−12.555%，AMD64 FP 为 −18.255%。这些改善不抵消其它负载的持续回退，也不推断完整游戏增益。

超 3% 的七项扩大到整数/bit 2.1B work、网关 210M work，并加入相同 before 二进制/hash 的 control。整数及 bit 另各做三组顺序轮换：after/before/control、before/control/after、control/after/before。保留最初 AB/BA、长样本和轮换顺序的全部轮次；不同 work 不混合计算中位数。同 work 的补样合并，但每组来源与排列仍保留。

| 扩样路径 | after / before CPU 中位数 | after / control |
| --- | ---: | ---: |
| AMD64 integer，2.1B、六组 | +5.901% | +1.498% |
| AMD64 bit，2.1B、六组 | +6.963% | +5.928% |
| AMD64 file-type，210M | +3.117% | +2.841% |
| AMD64 metadata，210M | +1.408% | +4.839% |
| PE32 GUID/raw，210M | −0.907% | −2.692% |
| PE32 module/IR，210M | +3.273% | +2.514% |
| PE32 event/raw，210M | +1.753% | +1.354% |

整数初组的 before/control 漂移较大，轮换后三组 after/before 为 +3.756%/+2.550%/+5.108%，after/control 为 +3.470%/+3.473%/+4.018%，仍记录局部成本。metadata 长样本三对为 +9.38%/−4.24%/+12.38%，显示较大宿主波动；没有丢弃其中任何一组。只读进程检查确认本任务只有一个 Wire child，没有 Cargo/rustc；同时存在桌面 renderer、视频 renderer 和 WindowServer 负载，未修改这些进程。

bit 的三组顺序轮换 after/before 为 **+7.255%/+5.774%/+6.191%**，相对同一 before 的另一 control 仍为 **+6.376%/+6.427%/+7.023%**。三组均超过 5%，初筛的 +6.508% 在顺序平衡后仍复现，因此拒绝单编译单元配置。不是用中位数中某一组较好的结果覆盖这一成本。最终汇总 `release-cgu1-final-metrics.json` 保留 45 条路径及扩样来源，所有结果校验通过，RSS 中位数最大增加 16KiB。

`Cargo.toml` 已恢复原 release 配置。冻结候选、source snapshot、所有失败/离散样本与测试日志保留于 `wire-out/refactoring-2026-10-14/release-cgu1-*`，不混入文件 section 或其它 API 迁移。后续继续原构建基线下的结构重构。
