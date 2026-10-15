# 当前状态

更新日期：2026-10-15。重构前固定代码基线 `2e78377`；本轮进度见 [重构记录](reports/refactoring-2026-10-14.md)。

最新已提交的运行时代码仍为目录枚举契约 `240e4ac`。其后独立补齐路径/句柄、section、Interlocked/SLIST 的 ABI 与故障基线；最近 `90658e2` 的三项 Interlocked/SLIST 测试在旧 handler 上通过，原创 gateway-slist 的双架构 smoke 也已校验。路径/句柄、网关边界、页槽/helper 布局及 section 首版候选均未合入。release 单编译单元候选完成 429 项正确性、native 7 项与 45 条性能路径后，因顺序轮换的三组 bit 长样本均回退超过 5% 而撤回。当前原构建配置下的 section 复核仍在进行，不计入已接受 API 数量。

R0/R1、R4、R5 已完成：正式 fixture、测试 harness、GUID 唯一契约、PE32 Process32、owned host UI 请求与 GDI/像素状态分离。R2/R3 已迁移 identity、guest environment、version、42 个 NLS/locale、31 个模块和 45 个文件 export，另完成双架构模块及 Win64 文件状态归属；PE32 内核/文件状态及其余家族仍待迁移。R7 已完成当前 PE32 内核目录、领域测试和公共性能工具的整理，后续目录随其余领域迁移继续推进。最新目录枚举契约批次完整 release 为 417 passed / 7 ignored，debug Win32/AMD64/FPU/Unicorn 302 项、fmt 与 strict Clippy 通过；7 项外部图形测试再次显式执行，全部通过。工具12 已在新增 fixture 基线提交验证。

R5 的 29 个解释器 case/path 已做三对 AB/BA，work/retired/checksum 全部校验；扩样后的最大 CPU 中位数回退为 +2.66%。R4 的 memory 成本在 R5 的 700M 复测中已消退：相对 R4 -6.04%，相对最初基线 -2.94%。这是结构调整后的代码生成/布局结果，不推断完整游戏 FPS；调查与全部轮次见 [重构记录](reports/refactoring-2026-10-14.md)。

version 的 11 个受影响 case/path 三对均校验通过；PE32 SSE raw 初筛 +4.39%，700M 扩样后为 +2.50%，相对最初基线 -6.72%。其它初筛回退低于 2%，RSS 无实质增长。版本查询、ABI-only 校验函数和两架构故障差异见 [version 契约](architecture/version.md)。

NLS 的 14 个 case/path 三对均校验通过；Win64 查询/转换负载 CPU 中位数 -33.08%/-11.31%。memory 扩样相对 version +3.14%、相对最初 +1.80%，已调查 IR/cache 计数及段反汇编，记录该布局成本。没有修改 ISA/cache 实现；回调和转换契约见 [NLS](architecture/nls.md)。

R7 的 38 个 case/path 三对全部校验通过；large-code 初筛超 3% 后扩为 2B work，成本降至 +0.22%，相对最初 +1.23%。扩样后的全矩阵最大 CPU 中位数回退 +2.41%，RSS 最大增加 32KiB。完整证据见 [R7 记录](reports/refactoring-r7-2026-10-14.md)。

模块状态批次的首个候选因 SSE 回退被拒绝；修复编译内联并消除引用更新的重复查表后，23 个 case/path 三对校验通过。相对 R7，700M SSE IR/raw 的 CPU 中位数 -13.56%/-2.78%，其它负载最大回退 +1.71%，RSS 最大增加 208KiB。失败候选、修复及最终 native/启动证据见 [模块记录](reports/refactoring-modules-2026-10-14.md)。

模块契约批次以独立布局调整后的 `c1af099` 为基线，26 个 case/path 三对校验通过；integer/memory/data-alias 初筛超过 3%，扩样后分别为 −0.10%/+2.31%/−0.72%，最终最大 CPU 中位数回退 +2.31%，RSS 最多增加 96KiB。bit-test 2.1B 为 +0.13%；静态导入与 fresh-provider 动态 lookup 的历史差异、已有 gateway fallback、全部失败/扩样轮次见 [模块 catalog 验收](reports/refactoring-module-catalog-2026-10-14.md)。

## 主路径和能力

macOS PE32 的五个实际执行边界已分别隔离到对齐代码节，SIMD 以 execute/EIP 回滚边界为准。28 个 case/path 三对及 integer/bit 700M 扩样全部校验，最终最大 CPU 中位数回退 +2.01%、RSS 最多增加 128KiB。实际生成代码与完整证据见 [布局记录](reports/refactoring-ir-layout-2026-10-14.md)。

核心文件 catalog 在上述新基线上独立验收：28 个路径三对校验，整数/GUID 的默认短样本经过同二进制噪声、采样及 utility QoS 扩样调查；受控 700M 结果 +2.32%/−1.38%，最终最大 CPU 中位数回退 +2.32%、RSS 最多增加 224KiB。两种调度模式分别记录，全部失败与离散样本保留。该批 PE32 全部 11 个 typed，Win64 9 个 typed、SetFilePointer/SetEndOfFile 保持 absent。见 [文件 catalog 验收](reports/refactoring-file-catalog-2026-10-15.md)。

路径/属性 catalog 新增 10 个 export，该批文件契约合计 PE32 21 typed、Win64 17 typed/4 absent。31 个 case/path 校验，整数 IR 的 700M 九对 +4.77% 经同二进制噪声与 2.1B 长样本调查；长样本中位数 −4.86%，仍有单轮离散，不宣称整数提速。最终矩阵最大 CPU 中位数回退 +1.91%、RSS 最多增加 80KiB。Alice 200M 保持隐藏窗口和预算出口，完整证据见 [路径/属性验收](reports/refactoring-file-path-catalog-2026-10-15.md)。

macOS AMD64 寄存器段执行器现独立放入 4KiB 对齐的 Mach-O 代码节，保持循环 body 与 guest 语义。该独立批次 release 403、debug 288、外部 GPU 7 项通过；26 个 case/path 三对校验，最大 CPU 中位数回退 +2.12%。它用于减轻 Win32 handler 重构引发的布局敏感性，模块 catalog 仍单独验收。见 [布局记录](reports/refactoring-ir-layout-2026-10-14.md)。

受检段也已独立隔离，并补齐两个节的显式代码属性。该补充批次 release 403、debug 288、native GPU 7 项通过；26 个 case/path 校验，memory +0.07%。SSE 扩样仍有 +3.63% 局部成本，已核对相同 IR/cache 计数及 2925 条指令的 opcode 序列并记录接受；其它最终结果最大回退 +2.63%，资源/cache 上限保持。模块组合以这批为新基线。

PE32 kernel 资源 owner 的多版抽取因持续回退被拒绝。最新重试基于五执行边界和核心文件契约 `6e16790`，correctness 408/293/native7 通过，但 utility 条件的三对 integer/FP/bit IR 中位数仍为 +206.81%/+8.30%/+10.23%，全部撤回。已提交的文件契约与布局保持，资源 owner 迁移暂缓；全部历史采样、回退与撤回证据见 [拒绝记录](reports/refactoring-pe32-kernel-rejected-2026-10-14.md)。独立故障/寿命回归和原创 event fixture 保留。

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

元数据契约追加五个 export，文件合计 PE32 21 typed/5 ABI-only、Win64 22 typed/4 absent。34 个 case/path 三对全部校验，PE32 metadata IR/raw 的 CPU 中位数 −17.85%/−9.02%；AMD64 thread-id 700M 扩样仍有 +4.10% 局部成本，独立 diagnostics 计数一致，已调查并记录接受，其余最终路径最大回退 +2.72%。RSS 中位数最多增加 144KiB，Alice 200M 保持隐藏窗口和预算出口。完整结果和失败候选见 [元数据验收](reports/refactoring-file-metadata-catalog-2026-10-15.md)。目录/变更/枚举、section、标准句柄等契约仍待迁移。

目录与文件变更追加 12 个 export，该批文件合计 PE32 25 typed/13 ABI-only、Win64 32 typed/6 absent。34 个 case/path 三对全部校验，事件 raw 初筛 +4.76% 经 210M 扩样后 +0.15%，最终最大 CPU 中位数回退 +2.25%、RSS 中位数最多增加 96KiB。单轮离散和宿主负载条件完整保留，不据此宣称解释器算法提速，见 [目录/文件变更验收](reports/refactoring-file-mutation-catalog-2026-10-15.md)。

目录枚举追加 7 个 export，文件契约累计 45 个：PE32 32 typed/13 ABI-only、Win64 39 typed/6 absent。首版因 AMD64 memory 成本未接受，最终按契约 ID 冷绑定原 group bit，保留句柄类型、cursor/output 和返回 frame 顺序。37 条性能路径全部校验；整数同二进制调查、GUID/path 六对合并与 metadata 700M 后，最大 CPU 中位数回退 +2.79%、RSS 最多增加 32KiB。FindClose 固定负载 PE32 IR/raw、AMD64 的 CPU 中位数分别 −16.23%/−12.73%/−12.45%；Alice 200M 保持隐藏窗口和预算出口。section、SHLWAPI/imagehlp、标准句柄及 CloseHandle 等仍待迁移，详见 [目录枚举验收](reports/refactoring-file-search-catalog-2026-10-15.md)。

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
