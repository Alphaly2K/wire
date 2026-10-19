# R7 CPU profile 工具归属

状态：已完成。CPU profile 的采样、比较、栈解析、汇总和火焰图实现进入 `tools/perf/profile.py`，该文件与原工具逐字节相同，SHA256 `2d9c02188ae1f3702c554cc057a2d9c41316168d1b7ed24dc0e3cd208205c67c`。原 `tools/cpu_profile.py` 脚本和 Python 模块入口保留；import 模式使用模块别名，旧 `cpu_profile.demangle` patch 继续作用于实际实现。

使用已保留的消息 before/GDI/日志优化三份真实采样作离线对照，固定 `PYTHONHASHSEED=0`，避免同权重热点排序受随机哈希种子影响。新旧 `summary.json`、`runtime-folded.txt`、`flame.svg`、stdout 和原脚本帮助逐字节相同。`python -m tools.cpu_profile --help` 正常；与脚本调用相比只保留 Python 自身的 invocation prefix 差异。证明 `r7-profile-proof.json`，旧/新结果目录 `r7-profile-before`、`r7-profile-after`。

全部 **24 项工具测试**通过，包括原架构/legacy 栈权重与嵌套 guest callback 分区检查。没有运行新的 guest，也没有更改 Rust 或 native 代码，不重复运行运行时性能。采样内容、FLAGS/ISA、导出契约及已验收 675 项范围保持。
