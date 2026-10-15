# 性能证据汇总工具

R7 增加 `tools/perf/summarize.py`，统一读取 PE32、AMD64 与 V2 的正式 timing manifest/compare.json。运行时基线为 Interlocked 契约 `9f7a0ac`；本批只增加离线工具和文档，没有改变解释器、fixture、计时或原报告 schema。

汇总以架构、case/path、输入及预期工作、QoS、日志/cache 协议和测量源码 hash 分组。同一组保留全部来源与轮次，计算 CPU/wall/RSS 的 min/median/max、成对变化及同二进制 control 漂移；不同 work 或协议独立出行，不生成跨负载总分或自动接受决定。输出为带 schema_version 的 JSON，原始 manifest 和 runs 均保留。

重复路径会先 canonicalize，避免 macOS `/var` 与 `/private/var` 别名重复计样。工具拒绝同标签不同 binary hash、重复或缺失 pair、未验证/timeout 和非 timing 输入；现有输出不能覆盖。fixture 仍负责 guest 行为验证，本工具消费已记录的证据。

工具测试 **16 passed**，其中四项新回归覆盖最慢轮次保留、source/pair 身份、work/QoS 分离、两种标签 schema、raw/IR/架构分离，以及重复/不完整/失败/diagnostics/换 binary 的拒绝。首次测试暴露临时目录路径的 macOS alias 期望差异；测试改为核对 canonical source，失败和最终日志分别保存在 `r7-perf-summary-tests{,-final}.txt`。

使用已完成 Interlocked 批次的 24 个证据目录做离线回放，25 条路径和两项不同规模的长样本分别呈现为 27 组。SSE/IR 六组 +1.949%、thread-ID 700M +4.557%、integer/IR 700M +2.732% 均与原验收一致，未重新执行 guest。输出为 `wire-out/refactoring-2026-10-14/r7-summary-interlocked-replay.{json,txt}`，使用方法见 [工具说明](../architecture/validation-tools.md)。
