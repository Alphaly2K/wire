# R7 CPU profile 工具归属

状态：已完成。CPU profile 的采样、比较、栈解析、汇总和火焰图实现进入 `tools/perf/profile.py`。纯迁移提交 `5348af7` 中，该文件与原工具逐字节相同，SHA256 `2d9c02188ae1f3702c554cc057a2d9c41316168d1b7ed24dc0e3cd208205c67c`；下述显式采样模式是后续变更。原 `tools/cpu_profile.py` 脚本和 Python 模块入口保留；import 模式使用模块别名，旧 `cpu_profile.demangle` patch 继续作用于实际实现。

使用已保留的消息 before/GDI/日志优化三份真实采样作离线对照，固定 `PYTHONHASHSEED=0`，避免同权重热点排序受随机哈希种子影响。新旧 `summary.json`、`runtime-folded.txt`、`flame.svg`、stdout 和原脚本帮助逐字节相同。`python -m tools.cpu_profile --help` 正常；与脚本调用相比只保留 Python 自身的 invocation prefix 差异。证明 `r7-profile-proof.json`，旧/新结果目录 `r7-profile-before`、`r7-profile-after`。

纯迁移阶段全部 **24 项工具测试**通过，包括原架构/legacy 栈权重与嵌套 guest callback 分区检查。该阶段没有运行新的 guest，也没有更改 Rust 或 native 代码，不重复运行运行时性能。采样内容、FLAGS/ISA、导出契约及已验收 675 项范围保持。

后续给 capture/compare 增加显式 `--headless` 和 `--discard-api-log`。前者传给解释器，后者在新轮次目录将 `api.log` 链接到空设备，仍执行完整格式化、计数和最近调用记录，避免大量磁盘写入。已有日志或链接会在启动 guest 前拒绝，不覆盖旧证据。默认行为保持；headless 与正数 `--stop-after-presents` 冲突时以参数错误退出。capture 的 `command.json` 保存实际 argv 和模式；compare 的 `commands` 保存同样的模式信息。不同 UI/日志模式须独立测量。

使用已验收 `r2-api-log-wire/wire` 实际采样消息 IR：sampler 0、guest 1（原创 fixture 的 int3 出口），149,999,999 retired、EAX 74,999,995、21,428,570 API calls 全部匹配，取得 **3978 个运行时栈样本**。另用同一 binary 做 12,009 指令双执行 compare，1715 API calls、寄存器、outcome、退出码相同；该小规模检查只验模式与结果，不作性能结论。实测参数冲突 exit 2、已有普通日志保持原内容。证据 `r7-profile-modes-{capture,summary,compare,protected,proof.json}`；工具 **24 项**检查通过，日志 `r7-profile-modes-tests.txt`。本次没有修改运行时。
