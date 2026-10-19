# 命令入口边界

状态：候选设计暂缓。当前运行时已恢复原 main 中的命令处理；以下模块划分保留作后续参考。候选的正确性和 19 组命令兼容检查通过，性能对照大幅漂移，源码与全部数据保存在独立冻结中。

`main` 只读取进程参数并交给 `cli::dispatch`。CLI 模块属于 executable，不成为解释器 library 的依赖；Process32/Process64 继续拥有 guest 装载、执行与运行时状态。

| 模块 | 职责 |
| --- | --- |
| `cli::dispatch` | 最小参数检查、帮助与命令选择；保留不足三个 argv 时先显示帮助的原优先级 |
| `cli::image` | `imports` 的 PE32/AMD64 静态检查，以及 `prepare` 的 IR 生成、文件写入和报表 |
| `cli::run32` | PE32 启动选项、Process32 调用、渲染/音频统计和故障诊断输出 |
| `cli::probe64` | 实验 AMD64 probe/startup 选项、执行调用与诊断报表 |

参数解析、帮助文字、默认值、错误优先级、退出码和输出由各原处理体保持；本次没有引入新参数框架。六个函数处理体按原文本迁移，只调整入口名称/可见性与模块调用。`probe64` 的 startup 和执行报表辅助函数留在同一命令模块。

后续新增命令只需在 dispatcher 选择对应命令模块；guest 资源和执行语义继续放在 library 的领域/架构层。验收与冻结见 [CLI 职责整理](../reports/refactoring-cli-2026-10-19.md)。
