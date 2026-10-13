# 当前状态

更新日期：2026-10-14。重构前固定代码基线 `2e78377`；本轮进度见 [重构记录](reports/refactoring-2026-10-14.md)。

## 主路径和能力

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
