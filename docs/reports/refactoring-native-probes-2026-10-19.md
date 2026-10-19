# R7 native 图形探针边界

状态：已完成工具迁移。七个旧 `tools/probe_*.py` 入口保留，实际职责进入 `tools/probes/`：DXBC/SPIR-V 检查、native shader/pipeline、资源、Draw、Present、geometry、MoltenVK 必要条件各自独立。共享的 **13 个 ctypes C ABI 布局**归入 `abi.py`，探针不再通过互相导入命令脚本获取布局。旧导入是实际模块的别名，原有公开类、函数与 patch 作用对象保持；脚本及 `python -m tools.probe_*` 继续可用，新包也支持模块调用。

先在旧工具固定七份 help、13 个布局的 size/alignment/逐字段 offset 和 size，并实际执行 shader、资源及主线程 Present 三项探针。新增两个独立测试在旧工具上通过，固定从外部工作目录构建时的真实 C/Objective-C 源文件路径。首版测试未处理 macOS `/var` 到 `/private/var` 的规范路径，修正测试预期后通过；原失败日志保留。

迁移后的 **10 个函数及 13 个结构体 AST 与旧实现一致**，仅有两处已审核路径变化：Present 的仓库根多上一层，MoltenVK 的 C 文件仍定位在 `tools/moltenvk_probe.c`。native source、FFI、构建脚本及第三方输入没有移动或重建。七个旧入口 help 逐字节保持，七个新包与七个旧模块入口均正常；13 个布局逐字段相同。源码证明 `r7-probes-source-proof.json`，原始帮助与布局 `r7-probes-before-proof.json`。

从项目外目录再次执行旧入口，三个真实 native 报告相同；资源报告仅排除两次实时 timestamp 数值，并分别验证单调性及相同正频率。**11 份 shader/像素输出逐字节相同**，包括八份 SPIR-V、RGBA bin/PNG 及资源 clear 输出。shader 探针回读 4096 像素，通道差异为零；资源探针保留 typeless/depth/copy/失败输出等检查；主线程窗口完成 12 次 Vulkan Present、131072 bytes source 回读与 16384 个 UNORM alias clear 像素校验。全部 **26 项工具测试**通过。

本轮机器缺少原先显式提供的 `spirv-val`、`dxbc_compiler` 和 Vulkan SDK headers，因此没有重跑 Draw/geometry 的独立 SPIR-V validator、DXBC compiler CLI 及 MoltenVK C compute 编译。没有使用替代程序冒充 validator；相应处理函数与 ABI 的静态对照已通过。三项真实 native 执行使用现存 graphics/MoltenVK dylib，仅复用小型已保留 shader，没有启动游戏或复制游戏素材。

证据根 `wire-out/refactoring-2026-10-14/`：`r7-probes-{inputs,before,after}`、`r7-probes-native-proof.json`、`r7-probes-tools-tests.txt`、`r7-probes-independent-baseline-v2.txt`。这些是独立 host 工具验证，不扩大 guest 图形能力或运行时正式 675 项契约范围。
