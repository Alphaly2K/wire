# 路径辅助与通用句柄契约

基线为 `cfe475d`，运行时与目录枚举契约 `240e4ac` 相同。两组独立回归分别在旧实现通过并提交：路径辅助为 `3bb016f`，句柄为 `cfe475d`。本批收敛 19 个 export 的 provider、参数与执行策略、绑定 ID；保留原资源 owner、路径算法、故障提交和架构能力范围。

## 范围

| 家族 | export 数 | PE32 | Win64 |
| --- | ---: | --- | --- |
| FileApi（已有） | 45 | 32 Typed / 13 ABI-only | 39 Typed / 6 Absent |
| PathApi | 13 | 13 Typed | 13 Absent |
| HandleApi | 6 | 2 Typed / 4 ABI-only | 6 Typed |

路径辅助包含 SHLWAPI 的 Exists、IsDirectory、RemoveFileSpec、RenameExtension、AddExtension、UnquoteSpaces A/W 与 imagehlp 的 MakeSureDirectoryPathExists。主 PE32 gateway 直接执行绑定 ID；file_api 的公开冷 facade 保持原 provider 大小写与 GetFileType 排除范围。互斥 Core/Helper ID 代替 Option<FileApi> 与名称组合，A/W、Add/Rename、Exists/Directory 由 ID 选择。NULL、CP932、UTF-16、root/prefix、MAX_PATH、checked output、LastError 和 trace 算法保持；原 Hangover/Wine LGPL 来源和版权完整保留。

句柄包含 GetStdHandle、SetStdHandle、CloseHandle、DuplicateHandle、GetHandleInformation、SetHandleInformation。PE32 仅 GetStdHandle/CloseHandle Typed，其余保留 ABI-only 的零返回、LastError=120、输出与资源不变；provider 仅 kernel32。Win64 六项均 Typed，provider 为 kernel32/kernelbase。DWORD selector、access、BOOL/options、mask/flags 在原参数位置截断，HANDLE 与输出地址保留完整宽度。Wine 10.0 spec 用于核对参数数量；句柄宽度继续按原 Win64 adapter 与 Windows HANDLE 语义表达，不能将 spec 的 long 一律视为 DWORD。

typed HandleApi 在原 handler 位置选择分支，保留各资源的查询顺序、保护位、错误码、提前完成及 unblock 差异。PE32 CloseHandle 先关闭资源、更新 named object、写 EAX，再读取返回帧；返回槽 fault 时已有资源提交。Win64 则先保存返回 frame，再执行 handler；输出覆盖返回槽仍使用保存的 frame。没有抽取通用完成框架、统一 handle arena 或改变 CPU 控制结果。

Unquote 的返回元数据为 Void，保留现有零 EAX carrier。已删除本批的重复 PE32 手写 ABI 与 Win64 arity/DWORD 列表；Wine spec 继续为其余 legacy API 提供 ABI。普通调用只用 cold-bound ID，未增加逐调用字符串注册解析。

## 验证进行中

路径、句柄、Win64 absent 与 catalog 唯一性专项回归通过。完整 release 为 425 passed / 7 ignored（包含 Unicorn 参照），debug Win32/AMD64/FPU 为 273 passed，显式 native GPU/Metal 为 7 passed；fmt、strict all-target/all-feature Clippy、release build、工具 12 项通过。新增 PathRemoveFileSpecA(NULL) 与 GetStdHandle 的 PE32 raw/IR、AMD64 原创 fixture 均通过旧/新 binary 的 12K smoke，默认 case 集合保持。

before 复用 `r2-file-search-cold-route-wire/wire`，SHA256 `927662997fbc71b5102113ab1b5ca2f8f9f5c86d723010d177d7f56be659438f`；候选冻结在 `r2-paths-handles-bound-wire/`，SHA256 `4bb34fe205cc28376b9f0087f7e9ab0cb85271ad450cc78b1b469eda455edde9`，5,832,280 bytes。thread-id 与 PE32 SSE/IR 的 700M 三标签对照已通过，中位数最大 CPU 成本 +1.74%；SSE 首对同二进制 control 也明显波动，全部轮次保留。

后续按 42 个 case/path 执行三对 AB/BA：AMD64 integer/memory/SSE 和 PE32 integer/bit IR 为 700M；PE32 raw/FP IR、AMD64 gateway 为 100M；AMD64 FP/bit/两类 alias 为 300M；PE32 九项 gateway 的 raw/IR 为 50M。以上连同两项初筛构成完整矩阵。所有计时串行，与 Cargo、sample、runtime diagnostics 分开；PE32 使用 utility QoS clamp 与 null API log。

42 条路径现已全部通过 work/retired/返回值/API count 校验。memory、PE32 integer/bit IR、Win64 version/file-type/search 的初筛中位数超过调查阈值；六项正在追加相同 work 的 before/after/control 三标签对照，按所有六对合并，不选择性丢弃原始慢轮次。后续文件 section 的四项基线草稿尚未注册 test target，未计入本批 425/273 测试数。

六项补样已完成。整数、bit 与 search 的成本消退；memory 六对合并仍为 +5.93%，file-type 为 +11.09%，version 为 +3.98%。该版未接受，全部运行时代码暂时撤出，完整补丁和两个 catalog 源码已保存，等待独立的访存 helper 布局基线后再恢复。没有用正确性通过替代性能验收，也没有继续执行本版 Alice smoke。

独立 memory diagnostics 的 IR=12008、fallback=0、translations=16、invalidations=0 一致。CheckedSegment 的 2156 条指令与规范化操作数全部相同；相应文件为 `r2-paths-handles-{checked.asm,checked-code-summary.json}` 与 `r2-paths-handles-memory-diagnostics/`。常用 checked_access/read_int/write_int 的入口地址和缓存行偏移发生变化，这作为布局隔离的调查线索；尚未将它认定为唯一原因。

单独路径首版、网关边界三个候选、页缓存命中内联候选均因持续性能成本未接受，相应运行时代码已撤出本批。原始结果分别见 [路径首版](refactoring-path-helper-catalog-2026-10-15.md)、[网关边界](refactoring-gateway-boundary-2026-10-15.md)、[页缓存候选](refactoring-page-cache-inline-2026-10-15.md)，不能沿用它们的性能作为本批通过证据。

后续独立的四个访存 helper 布局候选也因 PE32 SSE 三对持续回退而拒绝，见 [helper 布局记录](refactoring-memory-helper-layout-2026-10-15.md)。本批 19 项迁移继续暂缓，架构说明恢复为已合入的实际状态；完整补丁、契约源码和测试保留。先推进可独立验收的八项文件 section 契约，后续再调整本批的接入结构。

## section / Interlocked 新基线上的接入

以 `9f7a0ac` 的运行时为基线重新接入这 19 项。文件契约已为 53 项，另有六项 Interlocked/SLIST；新增 Handle/Path variant 追加到现有 ApiId 末尾，保留已合入家族的顺序，复用现有 Void 返回约定。R7 离线报告工具随后独立提交为 `537b1b8`，没有改变运行时或性能 fixture。

本版完整 release **432 passed / 7 ignored**、debug Win32/AMD64/FPU **280 passed**、外部 native GPU/Metal **7 passed**；fmt、strict all-target/all-feature Clippy、工具 12 项与 release build 通过。后续新增的性能汇总工具另有 16 项工具测试全部通过。下一批时钟测试和 fixture 正在准备，尚未注册，不计入本版 432/280 结果。

before 为 `r2-interlocked-verify-wire/wire`，SHA256 `983447a76ef302526b79f76568dabe03614f117aff7be77d4a49e10bcb331780`；after 为 `r2-paths-handles-v2-wire/wire`，SHA256 `fe24dec579585793f92866a893b500fbd53447a53297e72236eb40fe50cfd074`，5,842,536 bytes，减少 75,520 bytes。路径辅助与标准句柄的双架构 12K smoke 均通过；复测使用这一版 binary，不沿用旧候选的通过计时。

先完成六条敏感路径的 700M 三组轮换和同二进制 control：AMD64 memory −0.467%、thread-ID +1.901%、version −0.051%、file-type +0.240%。PE32 SSE/IR 初筛中位数 +5.789%，control 也比同一 before 慢 11.154%；追加相同规模三组后，六组完整合并为 −7.759%。保留全部离散轮次，不将这个变化宣称为 SSE 算法改善。integer/IR 的初筛为 −5.956%，after/control +0.289%。汇总位于 `r2-paths-handles-v2-sensitive{,-final}-metrics.json`。

正式计时结束后独立检查五个 PE32 执行节。before/after 的静态指令数为 ir32 2203/2207、mixed 502/502、SIMD 1664/1666、raw 2238/2238、0f 6213/6212；仅 raw 的 opcode 序列完全相同，其余节有代码生成差异。ISA、SIMD、IR、cache 源码均未修改；原始汇编和摘要位于 `r2-paths-handles-v2-{before,after}-__wire_*.asm` 与 `r2-paths-handles-v2-pe32-code-summary.json`。

其余 40 条路径已完成三对 AB/BA，完整矩阵共 46 条不同 case/path，全部通过 work、retired、返回值和适用的 API count 校验。SSE/raw 的 100M 初筛 +4.757%、module/raw 的 50M 初筛 +6.070%、section/IR 的 50M 初筛 +5.100%，分别追加 300M/210M/210M 三组顺序轮换和相同 before/hash 的 control。不同规模不混合中位数，原始初筛继续保留。

| 补样路径 | after/before CPU 中位数 | 样本 |
| --- | ---: | --- |
| PE32 SSE/raw | +1.554% | 300M，三组轮换 |
| PE32 module/raw | +1.679% | 210M，三组轮换 |
| PE32 section/IR | +3.078% | 210M，三组轮换 |

section/IR 相对 control 为 +3.178%，三个 paired 变化为 −21.029%/+4.407%/+2.932%。首组 before 明显偏慢，全部样本保留。长样本仍有约 3.1% 局部成本，按计划调查后接受，没有三组均超过 5%。计时结束后独立 12K diagnostics 的 IR=12007、fallback=1、CALL_NATIVE=2000、compact operations=10007、API count=2000 完全相同，结果为零；此前静态执行节调查显示生成代码有变化，ISA/cache 源码未修改。诊断单独存于 `r2-paths-handles-v2-section-diagnostics/`。

离线汇总工具读取全部 24 个计时目录，形成 49 个按工作量区分的组、46 条不同 case/path；完整来源、min/median/max、paired 轮次和 control 位于 `r2-paths-handles-v2-complete-metrics.{json,txt}`。采用上述长样本后的最大 CPU 中位数成本为 section/IR 的 +3.078%，最大 RSS 中位数增加 96KiB。没有用其它路径的下降抵消该成本，也不将离散的 IR/SSE 结果称为算法提速。

Alice 200M 启动前缀三对全部保持原预算出口、隐藏的 314×211 bootstrap 窗口和 Draw/Present=0，无新增 API/访存错误。CPU 中位数 2.6205s → 2.5974s（−0.881%），wall 2.6801s → 2.6633s，RSS +176KiB；输入哈希与输出在 `r2-paths-handles-v2-alice/`。继续复用已有 runtime 与原始 guest Mono，未复制或硬链接游戏。当前六个迁移源码的 SHA 与冻结候选逐项一致。本版 19 项正式接受；下一批时钟草稿未注册，也未混入本批验证。
