# 当前状态

更新日期：2026-10-14。重构前固定代码基线 `2e78377`；本轮进度见 [重构记录](reports/refactoring-2026-10-14.md)。

R0/R1、R4、R5 已完成：正式 fixture、测试 harness、GUID 唯一契约、PE32 Process32、owned host UI 请求与 GDI/像素状态分离。R2/R3 已迁移 identity、guest environment、version 和 42 个 NLS/locale export，另完成双架构模块及 Win64 文件状态归属；PE32 文件状态、file/module API catalog 及其余家族仍待迁移。R7 已完成当前 PE32 内核目录、领域测试和公共性能工具的整理，后续目录随其余领域迁移继续推进。最新完整 release 为 403 passed / 7 ignored（新增两项资源行为基线，运行时代码仍为 `4500aaf`）；strict Clippy、工具 12 项通过。7 项外部图形测试在 Win64 文件状态批次再次显式执行，全部通过；该批 debug Win32/AMD64/FPU/Unicorn 为 286 项。

R5 的 29 个解释器 case/path 已做三对 AB/BA，work/retired/checksum 全部校验；扩样后的最大 CPU 中位数回退为 +2.66%。R4 的 memory 成本在 R5 的 700M 复测中已消退：相对 R4 -6.04%，相对最初基线 -2.94%。这是结构调整后的代码生成/布局结果，不推断完整游戏 FPS；调查与全部轮次见 [重构记录](reports/refactoring-2026-10-14.md)。

version 的 11 个受影响 case/path 三对均校验通过；PE32 SSE raw 初筛 +4.39%，700M 扩样后为 +2.50%，相对最初基线 -6.72%。其它初筛回退低于 2%，RSS 无实质增长。版本查询、ABI-only 校验函数和两架构故障差异见 [version 契约](architecture/version.md)。

NLS 的 14 个 case/path 三对均校验通过；Win64 查询/转换负载 CPU 中位数 -33.08%/-11.31%。memory 扩样相对 version +3.14%、相对最初 +1.80%，已调查 IR/cache 计数及段反汇编，记录该布局成本。没有修改 ISA/cache 实现；回调和转换契约见 [NLS](architecture/nls.md)。

R7 的 38 个 case/path 三对全部校验通过；large-code 初筛超 3% 后扩为 2B work，成本降至 +0.22%，相对最初 +1.23%。扩样后的全矩阵最大 CPU 中位数回退 +2.41%，RSS 最大增加 32KiB。完整证据见 [R7 记录](reports/refactoring-r7-2026-10-14.md)。

模块状态批次的首个候选因 SSE 回退被拒绝；修复编译内联并消除引用更新的重复查表后，23 个 case/path 三对校验通过。相对 R7，700M SSE IR/raw 的 CPU 中位数 -13.56%/-2.78%，其它负载最大回退 +1.71%，RSS 最大增加 208KiB。失败候选、修复及最终 native/启动证据见 [模块记录](reports/refactoring-modules-2026-10-14.md)。

## 主路径和能力

PE32 kernel 资源 owner 抽取因 V2 bit-test 持续回退被拒绝，运行时代码恢复至 `4500aaf`。event 内联版的 700M bit-test 为 +7.83%，后续惰性 tid 和保留原类型身份的候选也未满足门槛；采样与指令形态调查见 [拒绝记录](reports/refactoring-pe32-kernel-rejected-2026-10-14.md)。仅保留独立故障/寿命回归和原创 event fixture，PE32 状态迁移仍待完成。

Win64 文件状态批次的首版因 AMD64 memory 持续回退被拒绝；恢复小型页查询 getter 内联后，24 个 case/path 三对全部校验。扩样的 memory −0.64%、PE32 integer/IR +0.38%、data-alias −2.12%，其余最终矩阵最大 CPU 中位数回退 +2.71%，RSS 最大增加 128KiB。标准 endpoint/alias 故障基线、短样本调查和全部证据见 [文件记录](reports/refactoring-files-2026-10-14.md)。

| 范围 | 当前入口与边界 |
| --- | --- |
| PE32 | `wire run <exe>`；`wire prepare <exe> --out <wir>` 后可用静态 IR。raw/IR 均解释执行，`.wir v3` 持久格式不变 |
| AMD64 | `wire probe64 <exe> --startup`，已有受检稀疏内存、typed IR、DLL/TLS、合作线程、异常与 Win64 服务；仍是实验入口 |
| 图形 | PE32 GDI/D3D9、AMD64 DXGI/D3D11 子集；Vulkan/MoltenVK 加速需显式提供已构建的 native graphics 库，未宣称完整 DirectX 符合性 |
| Mono | 执行游戏自带的原始 guest Mono；原生 ARM64 Mono 桥接已撤回，本轮不重构或重建该实验 |
| Wasm | JSC 可选加速研究与原创循环 fixture；主程序默认走解释器 |
| 文件/设备 | 受限 guest 文件系统；虚拟身份、单 CPU 和无设备/transport 行为按现有 API 契约处理 |

真实游戏兼容与性能的历史结果从 [文档入口](index.md) 查阅。当前基线的 Alice 启动 smoke 到 200M work 正常按预算停止，原始 Mono DLL/TLS 已执行；该前缀未进入 Draw/Present。

## 必需验证

```sh
cargo fmt --all -- --check
cargo test --release --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
python3 -B -m unittest discover -s tools -p 'test_*.py'
```

`2e78377` 的完整 release 结果为 382 passed、0 failed、7 ignored；构建、fmt 和 strict Clippy 通过。Unicorn 参照测试在受限 macOS 沙箱可能触发 SIGILL，必须在允许参照执行的环境完成验证。发布运行时不使用 Unicorn。

7 个 ignored 均有外部条件：2 个 Metal/software 像素对照需要真实 Metal device；3 个 native GPU 资源/Shader/owner 测试需要 native graphics、MoltenVK 与原创 DXBC/GS fixture；2 个 Win64 DXGI draw/swapchain 测试也需要上述 native 输入。涉及相应边界时显式提供环境运行，不将默认 ignored 计作通过。图形工具可从 `tools/build_native_graphics.py` 和 `tools/probe_*graphics*.py` 复现；第三方版本由 `third_party` 记录。

## 测试与性能存储

本轮 baseline executable、manifest、完整测试名及后续结果保存在 `wire-out/refactoring-2026-10-14/`。before 仅冻结一份已构建小型可执行文件，不复制游戏。已有游戏隔离目录通过软链接/原路径复用，原始素材保持只读。

任何影响 gateway、回调或执行布局的结构调整，按至少三对串行 AB/BA 检查 work/checksum、CPU/wall/RSS 和输入哈希。计时与 Cargo、采样、diagnostics 分开；持续回退先调查，不带着失败设计扩大迁移范围。
