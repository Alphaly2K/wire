# PE32 回调执行边界

`x86::process::call_guest` 拥有 guest callback frame、预算、嵌套执行及 Returned/Transferred/Exited 区分。DLL/TLS、SEH 和 CRT initializer 直接调用该入口，继续由原调用者决定保存、恢复和非局部转移策略；它们不再绕行 Cpu::call_guest 兼容 facade。

`x86/process/callbacks.rs::invoke` 统一原 USER 的 caller-context 策略：调用前 save_ctx，正常返回或错误时 load_ctx，Transferred/Exited 时保留新上下文。USER 窗口/计时器/对话框、locale、qsort comparator 和 UnhandledExceptionFilter 直接使用该进程服务。CRT 不再依赖 USER callback helper；原 user32::invoke 实现删除。

SEH 使用异常入口保存的上下文，DLL 初始化和 CRT initializer 也有各自策略，继续直接使用 call_guest。本批没有把这些不同的恢复规则合并。Cpu::run/call_guest 的公开兼容 facade 保留给外部调用，普通指令、执行循环和原 callback frame 构造不变。领域 handler 的 callback 请求/状态机仍按家族继续收敛，不将本批记为 R4 全部完成。

窗口文本与 focus/capture/active/foreground 已使用领域请求：User32 产生值、文本 callback 参数或所有权通知，Process 的 window_state 协调器执行 guest 并逐次重查目标。领域准备不再借 CPU 或总状态；提交先于通知、失败不回滚、Transferred/Exited 不恢复原 caller 的策略保持。见[窗口状态](window-state.md)。

基线 `1a93e31`。原 caller-context helper、八个调用文件、Process 执行循环/frame 作格式化后源码对照，路径与策略保持。初次编译遗漏两处 locale 调用，补齐后验收；失败日志保留。release 完整 Win32 255、IR 31、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。证据：`wire-out/refactoring-2026-10-14/r4-callback-owner32-{source-checks,verification}.json`。
