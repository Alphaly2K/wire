# 文件 section 契约迁移

基线为独立测试提交 `551aff9`；运行时代码仍与目录枚举契约 `240e4ac` 相同。路径/句柄迁移、网关提取、页槽内联与四 helper 布局候选均因性能门槛未接受，其代码已撤回。本批独立迁移文件映射的八个 export，不依赖这些候选。

## 范围和行为

现有 `FileApi` 增加 CreateFileMappingA/W、OpenFileMappingA/W、MapViewOfFile/Ex、UnmapViewOfFile、FlushViewOfFile；provider、参数类型、返回约定和架构执行策略集中到 `win32/catalog/files.rs`。删除这批重复的 PE32 手写清栈条目，以及 Win64 arity/DWORD 表。Win64 原 handler 位置按冷绑定 ID 选择，MapViewOfFileEx 的可选 base 参数也由 ID 选择。

| 范围 | PE32 | Win64 |
| --- | --- | --- |
| 本批八项 | 8 ABI-only | 6 Typed / 2 Absent |
| 累计 53 个文件 export | 32 Typed / 21 ABI-only | 45 Typed / 8 Absent |

PE32 provider 仍仅 kernel32，保持零返回、LastError=120、清栈、输出与资源不变；返回 frame fault 时 LastError 和 EAX 已提交。Win64 provider 仍为 kernel32/kernelbase，OpenFileMappingA/W 继续 absent。HANDLE、输出地址、view size 和可选 base 保留完整宽度，protection、offset high/low 和 access 等 DWORD 在 gateway 取参时截断。

Win64 的 SECURITY_ATTRIBUTES 读取先于 named/pagefile/protection 拒绝；MapView 的 access 和 64KiB 对齐校验先于 handle 查询。Flush/Unmap 的无效 base 继续返回 487。返回 frame 和参数预先保存，checked fault 不提交 CPU、API count 或 LastError。Mappings 继续拥有 section/view 与保留的文件描述符，文件句柄或 section handle 关闭后 view 的寿命、文件写入 coherence、64 位 offset 和资源上限保持原实现。

## 独立基线

四项行为测试先在旧实现通过并提交，覆盖 PE32 八项清栈、wrong-DLL、ABI-only 输出/资源与返回槽 fault 顺序；Win64 DWORD/HANDLE 宽度、校验先后、两个 absent export 和故障不提交。既有真实 read-only section 测试继续覆盖视图寿命及 guest WriteFile coherence。

新增原创 `gateway-section` 使用 FlushViewOfFile(NULL, 0) 循环，没有文件 I/O、时钟或原生 GPU 依赖。PE32 的期望 retired 为 `6*n+7`、API count 为 n；AMD64 retired 为 `6*n+10`、work 为 `7*n+10`、API count 为 n。两架构返回值均为零。默认 case 集合保持；旧冻结二进制的 PE32 raw/IR 和 AMD64 12K smoke 全部校验，工具 12 项通过。结果位于 `r2-section-baseline-*`。

## 验收状态

完整 release 为 429 passed / 7 ignored，debug Win32/AMD64/FPU 为 277 passed，显式 native GPU/Metal 7 passed；fmt、strict all-target/all-feature Clippy、release build 和工具 12 项通过。候选冻结于 `r2-section-catalog-wire/wire`，SHA256 `920df7a11e17ef25b6411809dcc51b5ab83bc3a993261d24e6d7ed4554423810`，5,928,136 bytes，相对 before 增加 1,072 bytes。

首版性能验收未通过，运行时代码已完整保存并撤回。完整结果继续保存于 `wire-out/refactoring-2026-10-14/r2-section-*`；计时使用冻结二进制并与 Cargo、sample 和 runtime diagnostics 串行。敏感路径包含相同 binary/hash 的 before/control，保留宿主波动与全部轮次。

700M 敏感路径初筛中，memory 三对 CPU 变化为 +6.65%/+1.57%/−6.17%，中位数 −6.17%；thread-id 中位数 −1.80%。PE32 SSE/IR 三对为 +9.88%/+7.01%/+4.30%，中位数 +4.30%，相同基线 control 的中位数也约 +4.2%。这些离散样本全部保留；不据此宣称访存提速。宿主进程检查时，除本批唯一 Wire 进程外，没有其它 Cargo、rustc 或 Wire 任务；同时存在桌面 renderer、视频 renderer 和 WindowServer 负载。

宽矩阵使用 45 个 case/path：AMD64 integer/memory/SSE/thread-id，AMD64 FP/bit/code-alias/data-alias，PE32 integer/SSE/FP/bit 的 raw/IR，以及九项 AMD64 gateway 和十项 PE32 gateway 的 raw/IR。AMD64 主循环和 PE32 integer/SSE/bit IR 为 700M work；其它 CPU 与 gateway 分别使用既有 100M/300M/50M 协议。SSE 另追加相同 700M、相同 utility QoS 与 null API log 的三标签对照，按完整六对合并。

45 条路径现已全部通过结果校验，初筛存于 `r2-section-initial-metrics.json`。SSE/IR 的完整六对合并为 +4.822%，相同基线 control 为 +3.201%，候选相对 control 为 +1.571%；所有初筛和补样均保留。计时结束后单独读取五个 PE32 执行器的代码节：ir32 2203、mixed 502、SIMD 1734、raw32 2238、0f32 6213 条指令，两版的五组 opcode 序列均一致。原始反汇编与摘要为 `r2-section-{before,after}-__wire_*.asm`、`r2-section-pe32-code-summary.json`。本批没有修改这些执行器或其指令语义；仍记录实测 SSE 局部成本。

raw 整数/SSE/bit、Win64 version/search 和 PE32 GUID/path raw 的短样本超过 3%，其中 raw 整数进一步扩大到相同 utility QoS、700M work 的三标签对照。长样本与初筛分别保存，不混合不同 work/QoS 的运行计算同一中位数；`r2-section-final-metrics.json` 引用保留的初筛记录与后续证据。

raw 整数 100M 的中位数为 +6.919%；700M 长样本仍为 +8.221%，三对分别 +30.68%/+3.89%/+10.36%，control 为 +4.364%，候选相对 control 为 +3.695%。第一组 before/control 15.443/19.687 CPU 秒的差异保留为明显宿主波动。候选在两个规模下均未通过性能门槛，因此拒绝本版，停止没有必要的其余扩样与 Alice smoke；没有用 IR 或新 section 网关的局部改善抵消 raw 成本。

八项 API 的独立基线和 fixture 保留在 main，契约/handler 改动及对应架构说明当时暂时撤回。随后独立评估 release 单 codegen unit，因顺序平衡的三组 bit 长样本均回退超过 5% 而拒绝，见 [构建候选记录](refactoring-release-codegen-2026-10-15.md)。当前恢复本批 source，继续使用原构建配置独立复核，重点补足 raw 整数与 SSE 的顺序平衡长样本；尚未接受第二次验收，不改变上述首版失败记录。
