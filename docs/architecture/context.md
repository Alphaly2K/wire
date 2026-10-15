# 进程身份与 LastError 调用边界

GetCurrentProcessId、GetCurrentThreadId、GetCurrentProcess、GetCurrentThread、IsDebuggerPresent、GetLastError、SetLastError 的 provider、参数和返回约定已集中到 `win32/catalog/context.rs`。状态仍由原进程、调度器与 guest TEB/PEB 拥有。正确性、性能和启动结果见 [迁移验收](../reports/refactoring-context-2026-10-16.md)。

| 行为 | PE32 | Win64 |
| --- | --- | --- |
| provider | kernel32 | kernel32/kernelbase |
| process/thread ID | 原 PID 4242；当前 scheduler TID | 原 PID 1；当前 active TID |
| pseudo handle | 32 位 −1/−2 | 完整 64 位 −1/−2 |
| debugger | 原零返回 | PEB+2 的原始 byte，不归一化为 BOOL |
| LastError | 入口从当前 FS TEB+0x34 导入，正常返回后写回 | 当前 GS TEB+0x68 直接读取/写入 DWORD |
| SetLastError 参数 | 单个 DWORD，stdcall 清栈 4 字节 | 截断高 32 位；保持原热路由 |

PE32 保留部分提交：SetLastError 先更新 state，公共返回阶段先写 EAX，再读取返回槽。若返回槽 fault，state/EAX 已修改，EIP/ESP 和 TEB error 尚未完成提交。不能通过预先读取返回 frame 改成另一种事务顺序。

Win64 保留原 frame preflight，返回槽 fault 时不写 LastError，也不增加 API count。六项现有 HotImport 由 typed ID 在冷绑定时选择；IsDebuggerPresent 与 delay import 使用 general handler。delay 解析前的 frame 检查、module handle/IAT 提交沿用公共调用入口。

独立测试在 `tests/win32/context.rs` 与 `tests/x64/context.rs`，固定 ABI、状态和故障行为，期望值不从 catalog 派生。原创 gateway-context 先直接写 TEB error，再循环调用 GetLastError 并累加结果，检查 guest 输出、退休数和 API count。
