# R7：启动参数与性能测量协议

## 触发与独立调查

时钟 catalog 候选通过 437 项 release、285 项 debug 和七项 native 验证后，旧性能协议的 PE32 integer/IR 出现三组约 +46% 的持续成本。先按拒绝门槛暂缓接入，保留原矩阵、宿主硬件计数和补样；没有修改 ISA/cache。

尝试显式保留时钟 adapter 函数边界后，构建哈希仍为 `573529ac7f879977f11f808f9fb24eb65402eed097ef678ac200cade6b404d83`，与首版完全相同。因此随后的较快轮次不归因于源码修复；无效的 inline 属性已经移除。

将原候选和同哈希副本同时放入旧协议的 700M 三对轮换，副本可重复出现约 2.3s/7.1s 的差异，guest 返回值、retired、IR 与 API 计数保持。原始证据为 `r2-clocks-identical-copy-investigation/`。这证明旧协议在这些启动条件下会区分相同机器码，单凭该组回退无法判断 catalog 改动的执行成本。

随后固定 argv0 和日志参数、分别保持默认 QoS 与 utility QoS。utility 的三组中，原候选/同哈希副本 CPU 中位数 2.309567s/2.306170s，差异约 0.15%；before/control 为 2.258847s/2.261630s。诊断在 `r2-clocks-fixed-argv{,-utility}-investigation/`，两种 QoS 分别保存，没有混成正式吞吐量。启动字符串及分配布局是线索，具体 allocator/cache 机制尚未确证。

## 可选稳定启动模式

三个 perf 入口增加 `--stable-launch`：固定 zsh 程序执行 `exec -a wire "$@"`，真实 binary 和参数按独立 argv 传入，没有 shell 字符串插值；child cwd 为该轮输出目录，PE32 日志参数固定为 `logs`。输入与静态 IR 的绝对路径在同一批次中一致。原 taskpolicy wrapper 仍在外层，timeout/status/wall/CPU/RSS 与 guest 校验继续来自原工具。

这是独立、显式的测量协议，默认行为保持。测量包含 shell 启动开销，所有对照同样包含。manifest 保存 launch 模式和 shell SHA；汇总工具保留该字段并拒绝跨模式合并。原始离散与失败证据不删除，也不借此宣称运行时或整数算法提速。

工具 **19 项通过**，新增回归验证实际 argv0、带空格/引号/命令替换文字的原样传参、child cwd 与 parent/report 隔离，以及汇总不混合不同 launch/shell。双架构 12K smoke **40 条路径**通过：PE32 12 个 case 的 raw/IR、Win64 12 个 case、V2 四个 case；其中真实文件类型查询与路径网关覆盖 cwd 调整。没有复制游戏，也没有修改 fixture 字节、预期 retired/work 或默认 case 集合。
