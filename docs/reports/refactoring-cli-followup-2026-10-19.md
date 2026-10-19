# CLI 职责拆分重试（2026-10-19）

状态：结构、完整 release、命令兼容与启动功能通过；随后在 [Unity 渲染结束后的累计复核](refactoring-post-unity-performance-2026-10-19.md)中通过性能验收。直接基线为控件初始化 `f776057`；本批仅改变 executable 的五个源文件，其余 289 个 src Rust 文件保持逐字相同。此前 [CLI 候选与撤回](refactoring-cli-2026-10-19.md)仍按历史结果保留。

main 保留六行参数收集与分发。四个 CLI 模块分别负责命令选择、静态 PE/IR 文件操作、PE32 启动和 AMD64 probe/startup；guest 资源仍由 library 中的 Process32/Process64 及领域服务拥有。

原 main 与之前保留的完整源码相同。六个处理函数及原 main 去掉 argv 收集后的 dispatcher，调整入口名称/可见性/命令调用路径后经相同 rustfmt 规范化，七份完整函数文本逐项相同。证明 `r7-cli-retry-function-proof.json`。只恢复这些命令处理体，没有恢复旧候选的 library 快照；本轮 paint、控件初始化及先前 API 日志/精确浮点优化均保留。

完整 release **631 passed / 7 ignored**，fmt、strict all-target/all-feature Clippy 与 release build 通过，日志 `r7-cli-retry-validation-*`。library 源码与刚完成验证的控件冻结完全一致；debug **477**、显式 native **7** 的证据沿用该同源批次，不将它们写成本次重复运行。工具 **26** 项本次复测通过，日志 `r7-cli-retry-tools.txt`。CLI 自身另做下述实际 executable 检查。

**21 组命令、42 次 child 执行**逐项相同：原十九组帮助、错误/预算、缺失文件、双架构 imports、prepare、PE32 raw/IR 及 AMD64 probe/stats，外加真实控件初始化的 raw/IR。固定 argv0、解释器软链接与 cwd，逐次核对 binary SHA；退出码与 stderr 全量比较，stdout 只规范化 prepare、PE32、AMD64 的已有时间/速率输出位置。prepare 的 IR 字节相同。

四组 PE32 实际执行还核对完整 API 调用记录与频率内容。首版新增控件 IR 用例因两个 API 的频率相同，HashMap 随机迭代顺序使 footer 的两行交换，全文件 SHA 不同；调用记录与输出保持相同。初始失败保留 `r7-cli-retry-contract`。修订只规范化频率 footer 行顺序，逐条调用的字节和顺序继续严格比较，原始日志及 raw SHA 全部保留；运行时日志实现不变。修订后全部 21 项通过，证据 `r7-cli-retry-contract-v2`。

检查已收进 `tools/probe_cli.py` / `tools/probes/cli.py`，由跟踪的 fixture 库生成三个小型 PE，固定 cwd/argv0/解释器软链接并在退出时删除链接，拒绝覆盖输出目录。独立复跑 **21 组、42 次对照执行**全部通过，另有一次基准 IR 准备；证据 `r7-cli-retry-portable-contract` 保存 binary/script/input SHA、命令及原始输出。用法见[验证工具边界](../architecture/validation-tools.md)。

冻结 `r7-cli-retry-wire/wire`，SHA256 `13a54292965570700a5c1322829627d5eb906fb49c948ebabd87d91e02591cbe`，6,025,576 bytes，较直接基线增加 **5,472 bytes**，满足预定 +24KiB 上限。294 个 src Rust 文件、target/frozen executable 与 manifest 一致，只有 main 与四个 CLI 文件变化；证明 `r7-cli-retry-source-proof.json`。

新增检查工具后另存 `r7-cli-retry-final-wire`，包含工具源码；294 个 Rust 源文件、两个冻结与 target executable 再核对一致，binary SHA 保持上述值。

一对 Alice 200M 功能前缀均到原预算出口，hidden 314×211、Draw/Present 0/0，无新增 API/访存错误；沿用同一 runtime 和锁定的游戏输入，解释器软链接每次退出删除。证据 `r7-cli-retry-functional-alice`。该轮用于启动/命令回归，不作性能或 RSS 准入。

本轮已观察到 Unity/WindowServer 持续活动及明显同哈希漂移，详见 [paint 宿主记录](refactoring-painting-followup-2026-10-19.md)。不重启长计时或枚举内联/CGU/LTO 配置；保留已验证的职责边界，性能与另外七项契约共同待宿主稳定后复核。R2/R3 剩余领域继续按家族迁移，本批不把全仓重构标为完成。
