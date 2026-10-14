# R2 模块 API 契约

基线为 `c1af099`，冻结 executable `ir-layout-checked-wire/wire`，SHA-256 `6832b3042699771ead13f04b08766061468e102c06c947c4ceaf3271f0478142`。本批在已验收的模块 owner 上收敛 API 声明；原始 guest Mono 作为普通 DLL 执行，未恢复原生 Mono，也未修改 IL2CPP。

## 实现与行为范围

`catalog/modules.rs` 集中 31 个 export：17 个模块/目录 API，以及 7 个 PSAPI 查询及其 K32 名称。PE32 的核心 13 个为 typed route，目录与 K32 的 11 个继续 ABI-only/legacy-unimplemented，7 个无前缀查询仍 absent。Win64 保留全部既有子集；Pointer 返回约定与 DWORD 参数宽度明确区分。

删除两架构的重复清栈、arity、DWORD、支持和名称路由声明。PE32 根 gateway 直接消费绑定 ID，公开 `modules::dispatch` 保留冷解析 facade；Win64 scalar/query handler 消费 enum，不再逐调用判断名称或剥 K32 前缀。模块控制 adapter 返回具名 Value/Returned/Load 请求，根 gateway 保存 CPU frame、API count、trace、unblock 和 LoadModule 提交顺序，包括原有提交后搜索信息错误的顺序。

静态导入 provider 与 fresh-provider 动态 lookup provider 分开声明，自动验证 lookup 是 import providers 的子集且不重复。Win64 历史静态查询接受 kernel32/kernelbase/psapi 两种名称；未绑定对应 gateway 时，动态 lookup 仅公开 kernel 的 K32 名称，其余返回 0/LastError 127。已有静态绑定或公开注册 gateway 的 registry fallback 继续返回原地址。独立测试同时覆盖这两个阶段，没有把差异合并为新的成功策略。PE32 Wine/native-export 动态解析保持原路径。

新增三项独立行为回归，覆盖 PE32 wrong DLL、零清栈与文件名输出覆盖返回槽；Win64 高 DWORD、保存返回 frame、wrong DLL；PSAPI 静态导入/fresh lookup/已有 gateway fallback。现有 DLL/TLS、搜索、引用/pin、输出 fault 与 provider 寿命测试继续执行。

## 拒绝候选与调查

最初 `r2-module-wire` 正确性通过，但 AMD64 bit-test 2.1B work 的 CPU 中位数回退 +6.03%，三对均超过 5%，拒绝。抽出 module control 后的候选仍未满足门槛；全部冻结源码、二进制和轮次保留。

随后将布局问题独立处理，先验收寄存器段，再验收受检段及正确 Mach-O 代码属性，见 [布局记录](refactoring-ir-layout-2026-10-14.md)。两项均独立提交，本批以新冻结基线重新验收。早期组合结果不能代替本批结果。

scalar route 保持优先于 module control guard，避免其它已迁移 scalar API 每调用多一次检查。旧优先级在 GUID 700M work 中为 +4.05%，调整后旧基线复测约 +0.85%；这些中间结果仍保留，但正式结果使用 `c1af099` 基线。

## 本批验收

候选为 `r2-module-current-wire/wire`，SHA-256 `03e5b2a3a2c26a3e3ae9f55737a1a244ba07e7b5059da1b9aae1943818daa8b3`。binary 5,907,432 bytes，比基线增加 21,872 bytes（0.37%）；IR/cache 容量与资源上限没有增加。source、diff、toolchain 和输入 hash 位于 `wire-out/refactoring-2026-10-14/` 对应 manifest。

完整 release 406 passed / 0 failed / 7 ignored，debug Win32/AMD64/FPU/Unicorn 291 passed；fmt 与 strict all-targets/all-features Clippy 通过。日志为 `r2-module-current-{all-tests,debug-tests,clippy,fmt-final}.txt`。恢复新文件时一次 fmt 检查发现末尾空行，修正后的检查单独保留。

性能矩阵覆盖 PE32 integer/SSE/FP/bit/GUID/module/event 的 raw/IR，AMD64 integer/memory/SSE/thread-ID/GUID/version/module/file-type，以及 V2 FP/bit/code-alias/data-alias，共 26 个 case/path，各三对 AB/BA；所有 work、retired、checksum 和对应 API count 校验通过。常规项 100M work，V2 FP/alias 700M，bit-test 2.1B；GUID/memory 初筛直接使用 700M。超过调查线的初筛与扩样均保留。

独立 memory diagnostics 两版均 IR=12008、fallback=0、translations=16、invalidations=0，cold=16，epoch/byte-change/mapping/eviction 均为 0。反汇编核对：CheckedSegment 为 2156 条、RegisterSegment 为 1968 条，两者前后入口分别固定在 `0x1002da000`、`0x1002dd000`，opcode 序列一致；mixed executor 为 2925 条，opcode 序列一致，但入口由 `0x100102c1c` 移至 `0x10010248c`。代码/常量重定位有变化，没有由这些证据推断访存成本的唯一原因。原始 asm、计数和分析在 `r2-module-current-{before,after}-*.asm`、`r2-module-current-code-analysis.json` 和 `r2-module-current-memory-diagnostics`。

PE32 integer/IR 初筛 +3.06%，700M 扩样后 −0.10%，raw −0.26%。memory 的 700M 初筛为 +4.80%，各对 +4.32%/+4.81%/+4.31%；扩大到 2.1B 后 +2.31%，各对 +2.46%/+2.34%/+2.31%。data-alias 的 700M 初筛为 +3.85%，各对 +4.23%/+4.88%/+1.48%；2.1B 后 −0.72%，各对 −1.64%/+2.21%/−0.72%。data-alias diagnostics 两版均 IR=12007、fallback=0、translations=13、invalidations=0，其它失效原因均为 0。bit-test 2.1B 为 +0.13%；GUID 700M 为 +0.71%。完整矩阵扩样后的最大 CPU 中位数回退 +2.31%，RSS 中位数最多增加 96KiB；没有三对均超过 5% 的回退。

全部 180 轮初筛/扩样保留于 `r2-module-current-{pe32,amd64,v2}-timing`、`*-preflight`、`*-investigation`。PE32 runner 原本只自动校验退休数和 EAX，本批另按原创循环的迭代数独立核对 36 个 API 密集报告的调用次数，全部符合预期。最终结果来源和调用计数核对保存于 `r2-module-current-validation-summary.json`，没有修改 fixture 来迎合候选。

正式计时后，native GPU/Metal 7 项显式执行通过，复用已有输入，日志为 `r2-module-current-native-tests.txt`。Alice 同目录 200M work 启动 smoke 按预算停止：retired 199588618、API calls 390479，隐藏窗口 client 314×211，draw/present 0，无新增 API 或访存错误。日志为 `r2-module-current-alice-smoke.txt`；没有复制或硬链接游戏，没有据此宣称完整游戏渲染/FPS 改善。
