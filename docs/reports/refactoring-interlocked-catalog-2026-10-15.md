# Interlocked / SLIST 契约迁移

基线为文件 section 契约 `619957f`。独立行为回归与原创 SLIST fixture 已先在旧 handler 上通过并提交为 `90658e2`；本批六项声明和执行边界已完成验收并接受，保留下述局部性能成本。

## 代码与行为

`win32/catalog/interlocked.rs` 集中 provider、参数、返回约定和架构执行策略。PE32 六项 Typed；Win64 仅 InitializeSListHead Typed，其余五项保持 Absent。删除 PE32 重复清栈表和旧 handler，以及 Win64 重复 arity 与旧字符串 handler。两端使用冷绑定 ID，适配器只借用参数和架构内存。

保留 LONG 回绕、CompareExchange 比较失败只需可读权限、SLIST 对齐优先及访问顺序、LastError 和故障提交。PE32 输出覆盖返回槽仍影响随后读取的返回地址；Win64 仍先保存返回 frame。InitializeSListHead 的返回约定为 Void，实际零 EAX/RAX trace carrier 保持。新 enum variant 追加到末尾，保留原有 variant 顺序。合作式 guest 调度的原子边界和 8/16 字节 header 差异见 [架构说明](../architecture/interlocked.md)。

完整 release **432 passed / 7 ignored**、debug Win32/AMD64/FPU **280 passed**、外部 native GPU/Metal **7 passed**；fmt、strict all-target/all-feature Clippy、工具 12 项和 release build 全部通过。新增 gateway-slist 的 PE32 raw/IR 与 AMD64 12K smoke 在两版均校验通过。

before 为 `r2-section-recheck-wire/wire`，SHA256 `920df7a11e17ef25b6411809dcc51b5ab83bc3a993261d24e6d7ed4554423810`。after 冻结为 `r2-interlocked-verify-wire/wire`，SHA256 `983447a76ef302526b79f76568dabe03614f117aff7be77d4a49e10bcb331780`，5,918,056 bytes，减少 10,080 bytes。证据均位于 `wire-out/refactoring-2026-10-14/`。

## SSE 初筛与调查

三组顺序轮换的初筛同时包含 before、after 和相同 before binary/hash 的 control。PE32 raw 整数为 300M，PE32 SSE/IR 与 AMD64 memory 为 700M。guest 结果全部校验；初筛 SSE/IR 的 CPU 中位数约 +17.1%，after/control 约 +14.9%，因此先调查再扩展矩阵。

计时结束后独立反汇编五个 PE32 执行节：ir32 2203、mixed 502、raw32 2238、0f32 6213 条指令的 opcode 序列保持。SIMD 从 1734 变为 1664 条，内联 packed 运算的寄存器分配与 spill 形态发生变化。静态代码变小不作为性能通过依据；完整原始汇编和摘要保存在 `r2-interlocked-{before,after}-__wire_*.asm`、`r2-interlocked-pe32-code-summary.json`。

随后单独用 `/usr/bin/time -l` 获取三组轮换的宿主指令数和周期，使用同一 700M 输入与 utility QoS，未与正式计时重叠。两版宿主指令中位数约 139.95B，差异 +0.00194%；周期中位数 24.637B → 24.381B（−1.039%）。同一 before 的 CPU 用时从 7.285s 到 8.991s，说明这批 CPU 秒受宿主执行条件明显影响；这些硬件计数作为调查证据，单独保存在 `r2-interlocked-sse-hardware/`，不混入正式吞吐量。

相同 work/QoS 另补三组顺序轮换 SSE/IR，与初筛全部六组共同汇总：after/before CPU 中位数 +1.949%，after/control +1.291%。没有删除初筛慢轮次，也没有修改 ISA/SIMD 源码。

## 完整矩阵与接受决定

25 条路径各至少三组，全部验证 work、retired、返回值和适用的 API count。包含 AMD64 integer/memory/SSE/FP/bit/code-alias/data-alias、thread-ID/GUID/SLIST/section 网关，以及 PE32 integer/SSE/FP/bit 和三项网关的 raw/IR。AMD64 integer/SSE/memory 和 PE32 SSE/IR 使用 700M；V2 为 300M，PE32 其它 IR 为 300M、raw 为 100M，AMD64 网关为 100M、PE32 网关为 50M；raw 整数的独立初筛为 300M。所有计时与 Cargo、采样及 runtime diagnostics 串行。

PE32 integer/IR 300M 初筛 +7.171%，AMD64 thread-ID 100M 初筛 +4.242%，两项均追加 700M 的三组顺序轮换及相同 before/hash 的 control。不同规模单独汇总，短样本和 SSE 全部原始轮次继续保留于 `r2-interlocked-initial-metrics.json`，最终汇总为 `r2-interlocked-final-metrics.json`，可用同目录的 `summarize_interlocked.py` 重建。

| 调查路径 | 最终 after/before CPU 中位数 | after/control |
| --- | ---: | ---: |
| PE32 SSE/IR，700M 六组 | +1.949% | +1.291% |
| PE32 integer/IR，700M 三组 | +2.732% | +0.102% |
| AMD64 thread-ID，700M 三组 | +4.557% | +4.664% |

thread-ID 三组分别 +4.557%/+5.450%/+4.047%，control 相对 before 的中位数仅 −0.103%，因此明确保留这项局部成本，没有把它归为已消除的噪声。独立 12K diagnostics 的 IR=9610、fallback=0、translations=14、invalidations=0、cold=14 及其它失效原因一致，HotImport 专用路由与 guest 语义保持。按计划调查后接受；未出现三组均超过 5% 的默认拒绝条件，也没有用 SLIST 的改善抵消 thread-ID 成本。

其它最终路径最大 CPU 中位数回退 +2.732%，RSS 中位数最多增加 64KiB。Win64 SLIST 网关的相同 100M work 为 2.0420s → 1.4736s（−27.837%）；PE32 SLIST 的 IR/raw 分别 +0.131%/−2.296%。这些是固定 API 负载的结果，不推断完整游戏 FPS。

Alice 200M 启动前缀三对全部保持预算出口、隐藏的 314×211 bootstrap 窗口和 Draw/Present=0。CPU 中位数 2.6177s → 2.6143s（−0.127%），wall 2.7209s → 2.6998s；RSS +80KiB。复用现有 runtime 和原始 guest Mono，无复制游戏或素材；输入哈希、命令及所有输出位于 `r2-interlocked-alice/`。该前缀仍是启动 smoke。
