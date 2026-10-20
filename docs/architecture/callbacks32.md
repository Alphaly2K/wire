# PE32 回调执行边界

`x86::process::call_guest` 拥有 guest callback frame、预算、嵌套执行及 Returned/Transferred/Exited 区分。DLL/TLS、SEH 和 CRT initializer 直接调用该入口，继续由原调用者决定保存、恢复和非局部转移策略；它们不再绕行 Cpu::call_guest 兼容 facade。

`x86/process/callbacks.rs::invoke` 统一原 USER 的 caller-context 策略：调用前 save_ctx，正常返回或错误时 load_ctx，Transferred/Exited 时保留新上下文。USER 窗口/计时器/对话框、locale、qsort comparator 和 UnhandledExceptionFilter 直接使用该进程服务。CRT 不再依赖 USER callback helper；原 user32::invoke 实现删除。

SEH 使用异常入口保存的上下文，DLL 初始化和 CRT initializer 也有各自策略，继续直接使用 call_guest。本批没有把这些不同的恢复规则合并。Cpu::run/call_guest 的公开兼容 facade 保留给外部调用，普通指令、执行循环和原 callback frame 构造不变。领域 handler 的 callback 请求/状态机仍按家族继续收敛，不将本批记为 R4 全部完成。

窗口文本与 focus/capture/active/foreground 已使用领域请求：User32 产生值、文本 callback 参数或所有权通知，Process 的 window_state 协调器执行 guest 并逐次重查目标。领域准备不再借 CPU 或总状态；提交先于通知、失败不回滚、Transferred/Exited 不恢复原 caller 的策略保持。见[窗口状态](window-state.md)。

对话框控件、窗口/线程/子窗口枚举也使用请求：USER 准备文本缓冲或原顺序快照，Process 的 dialog 协调器执行回调；整数解析只在正常返回后提交，顶层与子窗口的 FALSE 返回差异保持。见[对话框与枚举](dialog.md)。

绘制边界同样使用请求：USER 与 Graphics 准备区域/DC，Process 的 painting 协调器执行擦除并在正常返回后写 PAINTSTRUCT。sent 消息的选择/完成归 USER，monitor 临时输入归 display adapter，两类 callback 都由 Process 执行。见[绘制](painting.md)、[消息](messages.md)和[显示](display.md)。

posted 消息、计时器和键盘转换由 USER 产生值、等待或 dispatch 请求；Process 的 messages 协调器执行阻塞、TIMERPROC 与 WNDPROC。参数读取、change bits、copy-out、移除和键盘消费顺序保持，等待仍在原 API stub 上重新分发。

窗口几何和更新由 USER 准备；Process 在 WM_WINDOWPOSCHANGING 正常返回后让 USER 读取可变 WINDOWPOS 并提交，然后执行 WM_WINDOWPOSCHANGED。默认 WM_MOVE 与 WM_SIZE 之间重新查询过程/状态，见[窗口控制](window-control.md)。

窗口生命周期、显示、同步发送与默认过程随后迁入同一请求模式；`src/user32/` 内已没有 CPU、GuestCallOutcome、invoke 或 call_guest 依赖。创建/销毁循环与各自 cleanup 策略由 Process 的 window 协调器拥有，sent 表仍封装在 USER。其余 DLL/SEH/CRT 等进程边界按各自恢复策略继续收敛。

CRT 的数学返回、初始化器数组和 qsort 编排也归 Process；qsort 保留 caller-context invoke，初始化器继续直接 call_guest。函数表在每次调用后重读，首个错误与继续执行策略保持，见[CRT 边界](crt32.md)。

基线 `1a93e31`。原 caller-context helper、八个调用文件、Process 执行循环/frame 作格式化后源码对照，路径与策略保持。初次编译遗漏两处 locale 调用，补齐后验收；失败日志保留。release 完整 Win32 255、IR 31、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。证据：`wire-out/refactoring-2026-10-14/r4-callback-owner32-{source-checks,verification}.json`。
