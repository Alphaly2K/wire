# 进程身份与 LastError 契约迁移

## 独立基线

以时钟迁移 `91a4901` 为代码基线，复用 `r2-clocks-wire/wire`，SHA256 `573529ac7f879977f11f808f9fb24eb65402eed097ef678ac200cade6b404d83`。本批七项为 GetCurrentProcessId、GetCurrentThreadId、GetCurrentProcess、GetCurrentThread、IsDebuggerPresent、GetLastError、SetLastError，不移动调度器、TEB/PEB 或资源 owner。

先在旧 handler 上通过五项独立测试：PE32 scheduler TID、PID/伪句柄、LastError 从 TEB 导入/同步及 stdcall；返回槽 fault 时 state/EAX 已提交而 TEB/EIP/ESP 未提交。Win64 保持全宽伪句柄、DWORD error 截断、PEB 原始 debug byte、kernel32/kernelbase 范围；hot 与 delay-general 均先检查返回 frame，故障时不写 error、不增加 API count，delay 也不提前解析模块。focused 命令同时匹配到六项既有 context 测试，共 11 项通过；其中本批新增五项。

原创 `gateway-context` 直接写 TEB error 为 42，再循环调用 GetLastError 并累加返回值。PE32 raw/IR 与 Win64 12K smoke 全部校验退休数、API count 和 checksum。默认 case 集合保持。日志在 `wire-out/refactoring-2026-10-14/r2-context-baseline-*`，fixture 与期望值独立于 catalog。

## 候选接入与正确性

七项 provider、scalar 参数、返回约定与 typed ID 集中到 ContextApi，ApiId variant 追加到末尾。PE32 旧 ABI 和名称 handler 删除，仍直接使用原 scheduler/state；Win64 删除重复 arity/DWORD/general handler，冷绑定按 typed ID 选择原六个 HotImport，热执行 body 保持。DebuggerPresent 与 delay import 仍走 general 路由，未合并两架构的状态模型。详见 [调用边界](../architecture/context.md)。

五项 focused 新回归通过，完整 release **442 passed / 7 ignored**、debug Win32/AMD64/FPU **290 passed**、显式 native GPU/Metal **7 passed**、工具 **19 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。TLS/FLS 的下一批草稿未注册，不属于这些结果。

候选冻结于 `r2-context-wire/wire`，SHA256 `b6971fd9a39005601f2910da636187b84aed83b9e72f8d54ed09db99c0853175`，5,943,880 bytes，比时钟版增加 **6,400 bytes**。性能验收使用稳定启动协议，与 Cargo、采样、runtime diagnostics 分开。

## 性能验收

**25 条** case/path 的三对结果全部校验：PE32 raw/IR 的 integer、SSE、FP、bit 与 context/clock/module 网关，AMD64 integer/memory/SSE、context/thread-ID/clock/module 网关及 FP/bit/code-alias/data-alias。API 负载保留格式化与调用计数；PE32 使用同样的 utility QoS 和 discard trace 参数，全部使用显式稳定启动协议。

唯一超过 3% 的初筛是 AMD64 SSE/700M，CPU 中位数 **+3.006%**，三对 +3.026%/+3.006%/+2.652%。扩为 **2.1B** 并加入同 before 哈希 control，采用 after/before/control、before/control/after、control/after/before 三种顺序。长样本中位数 **+2.358%**，三组 +4.017%/+2.335%/+2.114%；control/before −0.180%，after/control +2.543%。保留这项局部成本；没有三组均超过 5%。不同 work 独立成组，初筛不删除、不与扩样混算。

其余初筛最大回退为 AMD64 clock **+2.505%**；PE32 raw integer 的三对分别 +5.384%/+1.299%/+3.236%，中位数比例 +1.306%，未形成持续门槛。PE32 context IR/raw 与 AMD64 context 分别为 −2.526%/−0.757%/−0.520%。RSS 中位数最大增加 **160KiB**。ISA、cache 容量与 HotImport body 未改，不据此推断游戏 FPS 或整数算法提速。完整汇总 `r2-context-stable-final-metrics.json` 包含 25 条路径和一个独立长样本分组，所有轮次和同哈希对照均保留。

提交前核对冻结 manifest 中 **228 个**已注册 Rust 文件，SHA 全部一致。Alice 200M 三对均校验通过：预算出口、隐藏 314×211 窗口、Draw/Present 0；CPU 中位数 −3.556%、wall −1.904%。该前缀未进入实际绘制，变化的退休/API/RIP 保留，不声称逐指令路径相同。游戏复用现有 runtime，协议单独记录为 fixed-argv-game-v1。证据在 `r2-context-stable-alice/`。据正确性、完整矩阵、扩样及启动回归接受本批，SSE 的局部成本明确保留。
