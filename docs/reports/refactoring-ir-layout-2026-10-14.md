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
