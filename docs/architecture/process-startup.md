# 命令行、启动信息与处理器能力

五项 export 统一声明于 `win32::catalog::process_startup`：GetCommandLineA/W、GetStartupInfoA/W、IsProcessorFeaturePresent。PE32 仅 kernel32，Win64 保留 kernel32/kernelbase 的静态和动态可见性；装载/查询时绑定 StartupApi，执行时进入所属架构 adapter。

`win32::{x86,x64}::process_startup` 只借 guest 内存及既有 A/W 命令行地址。命令行存储由原进程/运行时拥有，查询直接返回地址，不分配或解码字符串。STARTUPINFO 的 PE32 输出严格为 68 字节，Win64 为 104 字节，cb 使用各自大小，其余字节清零；保留 void API 的零返回 carrier、LastError 和 trace 行为。

受检写入先检查整个输出范围。PE32 原 gateway 在输出后读取返回地址；输出与返回槽重叠时使用被写入的内容。Win64 原 gateway 先保存返回帧，再调用 adapter；同样的重叠不会改变保存的返回 PC。输出故障保留各自原 CPU/API 计数提交顺序，没有把两种架构套入共同事务框架。

处理器能力维持原实现：PE32 仅 feature 6/10 返回真，Win64 仅 12 返回真。Win64 使用 DWORD 参数，保留高位截断与 trace 规则。本批没有扩展 CPU/指令能力。

独立测试固定存储地址、输出范围/故障、返回槽重叠、错误/trace、能力参数和 provider；原创 `gateway-process-startup` 循环消费两种命令指针、两个结构大小和三次能力回复。当前验证见[迁移记录](../reports/refactoring-process-startup-2026-10-19.md)。
