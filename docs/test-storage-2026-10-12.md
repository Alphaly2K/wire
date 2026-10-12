# 测试目录复用与清理

不再每轮复制整个游戏，也不使用硬链接。Alice 复用主 checkout 的
`wire-out/native-mono-probe-state/runtime`；这是已有的隔离副本，原始游戏保持只读。
每次测试通过临时软链接使用该目录、graphics、MoltenVK 和 registration reference。
wire 可执行文件只冻结一份小型临时快照，避免 Cargo 重建影响正在运行的测试。

`probe_startup.py --sandbox` 显式选择已有隔离目录；不再创建游戏 clone。
同一目录有排他锁。正常退出、失败、180 秒超时或 Python 中断都会通过 ExitStack
删除本轮软链接及 wire 快照。Player 日志先保存到本轮 `guest-logs`；配置与存档
留在共享隔离目录中，不随测试次数增加游戏副本。每轮保留命令、输入哈希、结果和画面。
SIGKILL 等强制终止无法运行 Python 清理回调；这不属于已验证的正常退出路径。

2026-10-12 已删除旧 worktree target、临时实验 target、重复游戏 runtime、重复
Mono reproducibility build 和旧的大型 API 日志，移动保留存档而非删除。
当前 `.codex/worktrees` 约 34 MB，磁盘可用空间从约 120 GiB 增至 158 GiB。
后续不再清理 `/tmp`，也不在 worktree 中重新构建或复制游戏。

存储回归 4 项通过：连续两轮使用相同 sandbox inode；wire 输入重建不改变运行中
快照；超时仍保存 Player 日志并删除临时链接/快照。另验证指向原游戏的 sandbox
在执行之前被拒绝，并在运行前清除旧 Player 日志，避免启动前失败留下上一轮的
日志。实际 probe-132 退出后也清理了所有临时链接/快照。这些是 harness 验证，
不代表游戏已进入可交互界面。
