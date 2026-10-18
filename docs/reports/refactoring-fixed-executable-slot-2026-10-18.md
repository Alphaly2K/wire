# 固定 executable 路径与 cwd 的性能协议（2026-10-18）

USER 名称/属性的 trace 修订通过完整正确性后，原 stable-argv-v1 的 48 条三对矩阵全部结果校验，但部分 Win64 网关出现数倍 CPU 成本：thread-id +193.579%、GDI profile +140.224%、rectangle +123.683%、keyboard +103.720%。相同 executable SHA 的早期副本没有同样成本；保留 `r2-user-objects-trace-final-stable-final-summary.json`，不接受该独立候选，也不执行其游戏验收。

正式矩阵结束后，串行采样旧版、最终候选、同哈希早期副本及最终候选重复轮。四轮的 560,000,010 retired、700,000,010 work、140,000,000 API 和 RAX 全部相同；两候选副本与重复轮约 6.95–6.98s CPU，样本中的主要热点权重接近，没有重现原矩阵的数倍成本。采样是独立诊断，未纳入正式计时，见 `r2-user-objects-trace-profile-v2/`。

进一步让旧版和两个同哈希候选副本串行复用同一 executable 路径、argv0 和 cwd，每轮替换的仅为小型解释器，输入相同。九轮结果均校验；旧版/候选/副本 CPU 中位数为 1.009615/1.032045/1.017599s，候选相对旧版 +2.222%，副本相对候选 −1.400%。这是新条件下的诊断结果，不与旧协议中位数合并，也未把早期较快轮次归因于源码变化。原始 `r2-user-objects-fixed-slot/` 保留复制式诊断，具体 allocator/cache 机制仍未确定。

三个性能入口增加可选 `--fixed-executable-slot`，显式要求 `--stable-launch`。每个 case/path 的所有标签与轮次使用同一 `execution/wire` 和 cwd；启动前硬链接**冻结解释器**并核对 manifest SHA，子进程退出后移除该链接，把生成文件移入所属轮次的 `workspace/`。不链接或复制游戏资产，不遗留每轮解释器副本，也不共享前一轮的可变工作目录。既有 stable 和默认模式保持。

manifest 记录独立 `fixed-executable-slot-v1` 协议、shell SHA、staging 与 artifact 规则；run 记录核对后的 executable SHA，汇总拒绝不匹配或缺失的 slot SHA。不同启动协议继续分组，原失败矩阵不会被覆盖或静默合并。

工具 23 项通过，覆盖实际路径/cwd 一致、失败出口、生成文件归档、源程序内容与链接数恢复、错误 SHA 拒绝和汇总防混用。首轮测试不适当地复制 macOS 受保护的系统 shell metadata，第二轮复制后的平台二进制被系统拒绝执行；改用原创 shell fixture 检查 staging/退出规则，真实冻结程序另外验证。首次真实入口发现局部 regex 变量遮蔽 context helper，已修正；失败日志均保留。

三个真实入口的 12K smoke 通过：PE32 三项 IR/raw、AMD64 六项两标签与 V2 四项两标签，另有旧窗口实现 Win64/PE32 IR/raw 三条原创 fixture 校验。PE32 fixture 首轮将触发 INT3 算入 retired，多算一条，已校正为 `6*n+15`，返回值/API 计数保持。证据根 `wire-out/refactoring-2026-10-14/`，前缀 `r7-fixed-slot-*`、`r2-window-query-fixture-baseline*`。

名称/属性和窗口查询的集成候选将使用该协议重新运行完整矩阵；在集成验收之前，累计仍为 501 项。
