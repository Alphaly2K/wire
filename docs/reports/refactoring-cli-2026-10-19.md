# R7 CLI 职责整理

状态：暂缓，运行时源码已撤回并精确恢复 `b433c63` 冻结。完整正确性与命令兼容检查通过，性能对照中宿主漂移较大，未准入。直接基线为已验收日志运行时 `b433c63`，本次不新增 API 或 guest 能力。

入口保留 argv 收集，按文件操作、PE32 启动与 AMD64 probe/startup 分离命令模块。六个原函数的完整文本经入口名/可见性替换后逐项 SHA 核对一致，证据 `r7-cli-wire/function-body-proof.json`；没有修改 library、参数解析、默认值、错误策略或报告内容。模块边界见 [CLI](../architecture/cli.md)。

完整 release **625 passed / 7 ignored**，debug Win32/AMD64/FPU **469**，显式 native GPU **7**，fmt、strict all-target/all-feature Clippy、SIMD crate 静态检查与 release build 通过，日志前缀 `r7-cli-validation-`。原日志两项 debug 检查及工具 23 项对应源码保持，不重复运行。

独立旧/新命令检查覆盖 19 种输入：帮助与缺少参数、未知命令、未知选项与非法预算、缺失文件、32/64 位 imports、prepare、PE32 raw/IR 执行及 AMD64 probe/stats。退出码和 stderr 全量一致；stdout 只归一化三个已有计时/MIPS 字段，guest 状态与其它文本一致。prepare 的 IR 文件和 raw/IR 的完整 API trace SHA 相同。证据 `r7-cli-contract-v2`；首次检查脚本误将错误参数用例当成产生 trace 的执行用例，未改运行时，失败记录 `r7-cli-contract` 保留。

冻结 `r7-cli-wire/wire`，SHA256 `56f4d57b1ca7e40bd3a65a7b40fbbb8b073391398fbb3c38b4d118946924e7f1`，6,024,184 bytes，较直接基线增加 5,472 bytes，预算 +24KiB。PE32 raw/IR、AMD64 计算/内存/缓存别名、实际消息/GDI gateway 的三对计时随后完成，计时期间未修改 Rust。

14 条协议、84 次执行全部校验，CPU 中位数最大回退为消息/raw +2.606%，完整数据 `r7-cli-broad-metrics.json`。但消息/IR 中同一 before 冻结 CPU 为 6.4605/9.2192/9.4485 秒，漂移较大，不能把该轮观测到的负差值记成 CLI 的优化收益。

另补消息 150M IR/raw 的三标签三对、18 次执行，均通过结果校验；IR after/before +9.796%、同哈希 control/before +8.488%、after/control +1.206%，raw 则 after/before +42.207%、after/control +29.409%，全部轮次和逐对离散保留在 `r7-cli-message-control`。原始 before/after/control 也各自出现明显漂移，未得到可重复的稳定对照。只读进程快照当时没有活跃 Cargo，WindowServer 约 40.6% CPU，视频应用两个 helper 合计约 41.9%；没有终止这些应用，也没有将这一时点的快照认定为全部历史变化的因果证明。

六次 Alice 200M 前缀全部按相同预算停止，隐藏窗口、314×211、Draw/Present 0/0；CPU/wall 中位数 +21.784%/+19.914%，before CPU 8.1069/5.1741/4.4539 秒，after 6.6202/6.3012/3.9002 秒。该轮同样不稳定，未准入，不推断游戏 FPS。源码冻结和 game runtime 继续复用，没有复制游戏资源。

独立反汇编显示 before/after 的 dispatch、日志器和 PE32 启动函数均分别为 7984/1015/2901 条指令，序言相同，地址改变；证据 `r7-cli-machine-code`。这没有确定成本来源，因此没有继续枚举内联、CGU 或 LTO 配置。

待所有计时与游戏结束后，用原文本恢复 main 并移除本批四个 CLI 模块；fmt/build 通过，402 个 Rust 文件和 6,018,712-byte release executable 与已验收冻结逐项相同，SHA 恢复 `c0f7d4c12691d2059c735bc2d14e8b1643741d3a975df21936a79b2cf02f937d`，证明 `r7-cli-rollback-verification.json`。因此不对相同字节重跑性能或完整测试。本批暂停，继续独立的离线工具职责整理。
