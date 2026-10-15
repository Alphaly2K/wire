# 事件与信号量契约迁移

## 独立行为基线

本批计划迁移 16 个 export：九个 Event 和七个 Semaphore。等待、mutex、计时器及调度控制接口保持 legacy，命名对象与句柄 owner 保持现状。基线 executable 复用 `r3-locals32-wire/wire`，SHA256 `677aef9afa0d232c365407de32cb1cfe8b89e24832b8512bfc5c31f6e645534f`。

四项新增测试先在原字符串 handler 上通过。PE32 固定 ReleaseSemaphore 的 count clamp、负数 release、输出 fault 前的资源提交、SetEvent 返回槽 fault 前提交、PulseEvent 清信号，以及六项 ABI-only export 的 error 120 与独立硬编码 stdcall 字节数。Win64 固定两个 kernel provider 的 ANSI/Ex 调用、DWORD 高位截断、共享对象/别名状态、previous-count 输出和 PulseEvent absent。原有 named/cross-kind/close、auto-reset/wait-all、访问权限及 Win64 输出 fault 不增计数等回归继续保留。

扩展原创 `gateway-event` 为双架构 fixture：创建一个 manual-reset event，循环 SetEvent/ResetEvent 并累加返回值。Win64 额外核对 API count，PE32 保留既有 raw/IR 协议。12K smoke 在原 executable 上三条路径均通过；默认 case 集合保持，未复制游戏或引入外部反汇编 fixture。

日志为 `wire-out/refactoring-2026-10-14/r2-sync-objects-baseline-{tests,amd64,pe32}.txt`。本记录目前只验收旧行为基线；契约与 Win64 adapter 草稿尚未注册，运行时迁移和性能验收未完成。
