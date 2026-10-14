# IR 执行器布局隔离

行为基线为 `bd24ddb`，其运行时代码仍为 `4500aaf`；性能 before 使用该批已冻结的 `r3-files-inline-wire/wire`。本项作为独立布局调整提交，模块 API catalog 候选另行验收，Mono/R6 忽略。

## 触发与证据

31 个模块 export 的 catalog 候选通过 406 项 release 测试，但 2.1B AMD64 bit-test 的三对 CPU 成本为 +7.60%/+6.45%/+5.33%，中位数 +6.03%，因此没有接受该候选。进一步抽取 Win64 模块控制 adapter 后，中位数仍为 +8.15%；保留全部失败样本。

两版 `RegisterSegment::execute` 均有 1968 条指令，操作码序列相同。内联 scalar match 的两张 u16 跳转表各有 32 个条目，内容相同，地址移动 48 bytes；热条目读取未跨 cache line。函数入口从 `0x100108250` 移至 `0x10010756c`，所有相对 dispatch 目标也随之改变指令行内位置。这些证据定位到代码布局差异，未将跳转表位置单独认定为回退原因。

macOS 的寄存器段执行器现放在 `__TEXT,__wire_ir`，由 Cargo 的 linker 参数设置 4KiB 对齐，使入口及相对指令位置独立于相邻 Win32 handler 的长度。未修改执行器 body、FLAGS/fault/预算语义、IR 格式、cache 容量或 opcode。其它宿主平台保持原布局；外部单独链接 wire 库的程序需自行设置对应节的对齐，当前配置适用于本 crate 的 Cargo 可执行目标。

独立基线版与 module 组合候选的该函数均位于 `0x1003ba000`，均有 1968 条指令，opcode SHA-256 均为 `94693f2ae70965dbc3fa78f7959a9e2c0b6ee1dcc67194568254c760ab2859c5`。常量和冷调用 relocation 仍由 linker 解析。对齐隔离改善了已观察到的布局敏感性；未来其它源码变更仍需按矩阵验收。

## 独立验收

恢复 module catalog 改动之前，单独构建和冻结布局调整版：`ir-layout-baseline-wire/wire`，SHA-256 `4b9918392ce860b6257552b171d737b04a54d2aafe8a53dfecb6873655a7037f`，5,868,984 bytes，比性能 before 减少 64 bytes。源码、diff 和工具链由该目录 manifest 保存。

- 完整 release：403 passed / 0 failed / 7 ignored；debug Win32/AMD64/FPU/Unicorn：288 passed；fmt、strict all-targets/all-features Clippy 通过。
- 26 个 case/path，各三对串行 AB/BA，全部 guest state/checksum/work/retired/API count 校验通过。V2 FP、bit、code-alias、data-alias 使用 700M work；其它 PE32 raw/IR 与 AMD64 CPU/gateway 使用 100M work。
- 最大 CPU 中位数回退 +2.12%（PE32 event/raw）；700M FP +0.52%、bit +0.71%、code-alias −0.37%、data-alias −1.12%。RSS 中位数最大增加 48KiB。原来的资源与 cache 上限保持。
- 计时完成后，提供已冻结 native graphics/MoltenVK/GS/DXBC 输入，7 项外部 GPU/Metal 测试显式执行并通过。
- Alice 复用现有软链接隔离目录，200M work 按预算停止，retired 199588574、API calls 390526，隐藏窗口 client 314×211，draw/present 0；与 before 的启动边界相同。没有复制游戏，也没有推断完整游戏 FPS。

原始数据位于 `wire-out/refactoring-2026-10-14/ir-layout-baseline-*`；失败候选为 `r2-module-bit-investigation`、`r2-module-control-bit-preflight`。机器码与跳转表分析记录为 `ir-layout-machine-code.json`、`r2-module-jump-table-analysis.json`。普通 objdump 默认只选 `__text`，读取本函数须显式使用 `--section=__wire_ir --disassemble`。

## 受检段的独立补充

以 `ff7495a` 为基线，module 组合候选恢复 scalar 优先分发后，700M memory 中位数 +5.41%。两版 `CheckedSegment::execute` 均为 2156 条指令、opcode 序列相同，入口从 `0x1000f7158` 移到 `0x1000f69c8`。将受检段放入另一个 4KiB 对齐的 `__TEXT,__wire_irchk`，权限、逐指令回滚及预算逻辑保持；同时为两个节显式声明 `regular,pure_instructions`，消除 linker 对代码节/unwind 属性的警告。失败日志也保留。

再次撤出 module 改动，独立冻结 `ir-layout-checked-wire/wire`：SHA-256 `6832b3042699771ead13f04b08766061468e102c06c947c4ceaf3271f0478142`，5,885,560 bytes，比上批增加 16,576 bytes。完整 release 403、debug 288、strict Clippy、fmt 和真实 GPU 7 项通过。26 个 case/path 三对全部校验；PE32 integer/raw 短样本 +3.33%，700M 为 +1.12%。AMD64 memory +0.07%，其余最终结果最大回退为 SSE +3.63%，RSS 中位数最大增加 144KiB。

SSE 的 700M 复测保留了 +3.63% 局部成本，没有触发三对均超过 5% 的拒绝条件。诊断 replay 的两版 IR=1000003、fallback=0、translations=13、invalidations=0；`execute_mixed` 均有 2925 条指令、opcode 序列相同，入口从 `0x100104d7c` 移至 `0x100102c1c`。接受这一已调查的布局成本，以固定受检段边界；没有增加向量/ISA 实现、cache 容量或通用 payload，也不将其全部归因于单一地址变化。后续模块组合独立对照这个新基线。

Alice 的 200M work 仍按相同预算出口停止：retired 199588536、API calls 390560，隐藏窗口 client 314×211，draw/present 0。证据在 `ir-layout-checked-*`，包括全部短样本、700M 扩样、独立 stats 和机器码分析；计时与 Cargo/diagnostics/native 测试分开。受检段反汇编须显式选择 `--section=__wire_irchk`。

## PE32 执行边界隔离（2026-10-15）

文件 catalog 的三个候选先后出现持续的 PE32 IR 回退，保留原 handler 边界的版本仍有 integer/IR +206.49%、FP/IR +8.37%、bit/IR +11.44%，因此全部撤出；失败记录见 [文件 catalog](refactoring-file-catalog-2026-10-15.md)。本项以 `5cca1b9` 为代码基线，性能 before 为 `r2-module-current-wire/wire`，不包含文件 catalog 或 PE32 resource owner。

macOS 上将五个实际执行边界分别放入 4KiB 对齐的代码节。只声明节、对齐及 SIMD 的非内联边界，Rust 循环 body、ISA、FLAGS、逐指令故障/预算、IR 格式和 cache 容量保持。SIMD 的外层 execute 包含原来的 EIP 回滚，execute_inner 继续由编译器优化；没有改变错误提交逻辑。

| 实际函数 | 代码节 | before / after 指令数 |
| --- | --- | ---: |
| Cpu::steps_cached | `__wire_ir32` | 2208 / 2203 |
| Cpu::execute_mixed_run | `__wire_mix32` | 503 / 502 |
| Descriptor::execute_inner → execute | `__wire_simd32` | 1659 / 1664 |
| Cpu::step_x86 | `__wire_raw32` | 2238 / 2238 |
| Cpu::step_0f | `__wire_0f32` | 6220 / 6212 |

SIMD 的 before/after 包含不同的编译调用边界，不将其计数解释为等长代码。raw32 的 opcode 序列相同，其余生成指令有变化，完整反汇编和摘要保存为 `pe32-executor-boundary-*-*.asm`、`pe32-executor-boundary-code-summary.json`。

此前 `pe32-executor-layout-*` 候选虽然完成 28 个路径验收，实际只生成四个独立节：给 execute_inner 加属性后它仍被内联，SIMD 节不存在。删除该无效属性又改变内联选择，重建 binary 不同，因此没有沿用其数字；最终把属性放到实际 execute/回滚边界，并以 inline(never) 保持这一调用边界，重新完成以下验收。

- 冻结 `pe32-executor-boundary-wire/wire`，SHA256 `c9953706333efce939baa9a9a84294a13a1276731a380f202735e132ba7706b7`，5,923,928 bytes；相对 before 增加 16,496 bytes，来自新增对齐代码节和链接信息。
- 完整 release 408 passed / 7 ignored；debug Win32/AMD64/FPU/Unicorn 293、fmt、strict all-targets/all-features Clippy、发布构建通过。真实 GPU/Metal 7 项显式通过。
- 28 个 case/path，100M work、各三对串行 AB/BA，全部 work/retired/checksum/API count 校验。PE32 integer/IR 首对离散，另做三对 700M，为 −0.01%；bit/IR 短样本 +3.21%，700M 为 −9.97%，全部初筛与扩样保留，不推断游戏增益。
- 使用扩样后的最终矩阵，最大 CPU 中位数回退 +2.01%（PE32 integer/raw），RSS 中位数最多增加 131,072 bytes。未引入执行缓存或额外 guest 工作量；code-alias/data-alias 校验保持。
- Alice 复用现有隔离目录，200M work 正常预算停止（exit 1），retired 199588643、API 390480、隐藏窗口 314×211、draw/present 0。该前缀尚未进入正式窗口渲染。

原始证据仍位于 `wire-out/refactoring-2026-10-14/`，前缀 `pe32-executor-boundary-*`；最终指标为 `pe32-executor-boundary-final-metrics.json`。代码布局与文件 catalog 分别提交、分别验收；其它平台保持原节布局，外部独立链接库仍须自行配置对应节的对齐。
