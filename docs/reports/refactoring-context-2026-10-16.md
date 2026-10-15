# 进程身份与 LastError 契约迁移

## 独立基线

以时钟迁移 `91a4901` 为代码基线，复用 `r2-clocks-wire/wire`，SHA256 `573529ac7f879977f11f808f9fb24eb65402eed097ef678ac200cade6b404d83`。本批七项为 GetCurrentProcessId、GetCurrentThreadId、GetCurrentProcess、GetCurrentThread、IsDebuggerPresent、GetLastError、SetLastError，不移动调度器、TEB/PEB 或资源 owner。

先在旧 handler 上通过五项独立测试：PE32 scheduler TID、PID/伪句柄、LastError 从 TEB 导入/同步及 stdcall；返回槽 fault 时 state/EAX 已提交而 TEB/EIP/ESP 未提交。Win64 保持全宽伪句柄、DWORD error 截断、PEB 原始 debug byte、kernel32/kernelbase 范围；hot 与 delay-general 均先检查返回 frame，故障时不写 error、不增加 API count，delay 也不提前解析模块。focused 命令同时匹配到六项既有 context 测试，共 11 项通过；其中本批新增五项。

原创 `gateway-context` 直接写 TEB error 为 42，再循环调用 GetLastError 并累加返回值。PE32 raw/IR 与 Win64 12K smoke 全部校验退休数、API count 和 checksum。默认 case 集合保持。日志在 `wire-out/refactoring-2026-10-14/r2-context-baseline-*`，fixture 与期望值独立于 catalog。
