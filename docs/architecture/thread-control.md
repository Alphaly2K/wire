# 线程执行与生命周期

PE32 与 Win64 的 CreateThread、ExitThread 使用同一 Wasm JIT 执行协议。provider 与参数宽度来自 `win32/catalog/thread_control.rs`；DWORD 参数截断为 32 位，Win64 的入口、参数、句柄与 SIZE_T 保持完整宽度。其余线程控制 API 的迁移状态以根 [GOAL.md](../../GOAL.md) 为准。

CreateThread 接受 CREATE_SUSPENDED 和 STACK_SIZE_PARAM_IS_A_RESERVATION，未知 flags 返回错误 87。安全属性只接受正确结构长度、空描述符和继承标志，其余描述符返回错误 50。非空 ID 输出在资源发布前检查；失败不分配线程身份或句柄。新线程有独立的栈、TEB、TLS 槽表和架构状态；初始线程 ID 为 1，后续 ID 单调递增，资源槽可在完整退出后复用。最多 64 个活动 worker 资源槽，worker 栈按 64 KiB 对齐并全量提交，上限 16 MiB；不支持按 guard 增长。入口地址在实际执行时取指，非法地址报告精确客体故障。

所有线程共用同一代码缓存、VMM 和 Wasm JIT。切换只发生在驻留执行已保存状态之后，保存 GPR、FLAGS、XMM/MXCSR、x87、段基址、异常与 REP 续点。work 和退休数为进程累计值，不随线程恢复旧值。计算线程每 10000 work 让出一次时间片，阻塞或显式让出允许提前切换。

每个模块为每个活动线程保存独立静态 TLS 块，从原始模板和零填充建立，不复制其他线程修改后的内容。LoadLibrary 在初始化前为全部活动线程准备新增 TLS，包括未启动及挂起的线程；FreeLibrary 在释放映像前统一检查、清空并回收全部线程的对应槽和块。

新线程在进入用户入口前，按模块初始化顺序执行 TLS 和 DllMain 的 DLL_THREAD_ATTACH；退出按逆序执行 DLL_THREAD_DETACH。通知与用户入口使用相同 JIT、预算、故障及返回帧协议。DisableThreadLibraryCalls 抑制相应模块的线程通知；含静态 TLS 的模块拒绝此设置。加载和通知帧由明确的线程持有，允许该线程重入装载；其他线程的模块服务以及未启动线程等待加载锁。已启动线程可以继续执行普通代码和 I/O，从而满足锁持有者的等待。

worker 普通返回和 ExitThread 共用退出通知与回收路径。先执行当前线程的 [FLS 析构](locals.md)，再发送 DLL_THREAD_DETACH；原始返回值或显式 code 按 DWORD 保存，析构和通知的寄存器变化不覆盖退出码。全部 TLS、栈与 TEB 释放范围和槽输出先检查，成功后才发布退出状态；故障修复不会重放已完成回调。关闭句柄不结束线程；退出状态由剩余别名和所有调用层级的活动等待保留，资源回收不等待最后一个句柄关闭。清理成功后，线程持有的互斥体标记为 abandoned，由后续等待取得所有权并报告 WAIT_ABANDONED。

ExitThread 可以结束初始线程并让 worker 继续执行，最后一个线程退出返回 ThreadReturned。EXE 原始入口 RET 仍返回 Returned，交回映像运行结果。持有加载通知帧的线程在回调内调用 ExitThread 返回错误 170；退出析构的非局部展开、完整 USER 清理与相关退出控制的范围记录在 GOAL。

FreeLibraryAndExitThread 在两种 ABI 接受 kernel32/kernelbase，先按 FreeLibrary 的引用和依赖规则完成模块释放，再进入同一线程退出协议。DLL 可以由其自身 worker 卸载；完成后的控制流直接进入退出续点，不返回该 DLL 的已释放代码。仍有引用或 pin 的模块保留，逻辑 provider 保持进程寿命；无效模块的 FreeLibrary 错误写入 LastError 后仍退出线程。该 API 不返回到调用者。

释放先发送被卸载模块的 DLL_PROCESS_DETACH，随后线程退出执行 FLS 析构和其余模块的 DLL_THREAD_DETACH；被移除模块不再收到线程通知。通知允许嵌套异常、普通服务和让出，最终 TLS 槽写入或回收故障不重放已完成通知。初始通知帧预检失败不修改引用或计 API work；接受后 API 仅计一次 work。其他线程持有加载锁时等待原锁；当前线程正在执行装载通知时，该组合退出请求明确报错并保持原状态，尚不支持放弃装载器拥有的回调。

初始进程 attach 被拒绝时，逆序通知和映像/TLS 回收同时释放尚未启动 worker 的栈、TEB 与句柄，随后保持启动拒绝状态。

QueueUserAPC 在两种 ABI 接受 kernel32/kernelbase，要求 THREAD_SET_CONTEXT；无效句柄、权限不足及退出目标分别返回错误 6、5、31。每线程最多排队 256 项，容量错误 8 先于 callback 访存。成功排队推进调度状态；退出和进程清理丢弃剩余队列。callback 接受可执行客体地址或已绑定服务入口，队列规则与旧入口共用。

SleepEx、对象 Ex 等待、MsgWaitForMultipleObjectsEx 的 MWMO_ALERTABLE，以及 WSAWaitForMultipleEvents 支持用户 APC。参数和对象先验证，再检查队列；已排队 APC 优先于消耗信号或超时，无需采样时间。投递前验证整批 callback 和首个临时栈帧，失败保留队列；随后使用同一 Wasm JIT 的每线程回调续点执行，支持嵌套异常、等待和线程退出，共用 32 层回调深度限制。正常完成后恢复外层调用者的完整架构状态，保留累计 work/退休数，忽略各 callback 返回值，等待返回 0xc0。后续栈帧故障保留未完成序列，不重放已完成 callback。

GetThreadContext、SetThreadContext 在两种 ABI 通过 kernel32/kernelbase 提供，分别要求 THREAD_GET_CONTEXT 和 THREAD_SET_CONTEXT。已退出或正在退出的线程返回错误 6；其他线程必须已 SuspendThread，否则返回错误 170。当前线程可以查询其逻辑 API 调用者上下文，直接设置当前执行线程返回错误 170。AMD64 CONTEXT 必须按 16 字节对齐，不合要求返回错误 998。

查询按 ContextFlags 选择 control、integer、segments、floating，以及 i386 的 ExtendedRegisters；仅覆盖请求的字段，保留其余字节。完整结构大小为 i386 的 716 字节或 AMD64 的 1232 字节，读写统一受检；不支持的调试寄存器和 XSTATE 组件返回错误 50，缺少架构标志返回 87。异常上下文采用同一[字段布局](exceptions.md)。

设置先读取、验证完整输入，再发布目标状态。控制寄存器中的特权 FLAGS 与固定用户段选择子归一化；非规范 AMD64 PC/SP、非法 MXCSR 或不一致的 MXCSR 副本返回错误 87，保留目标状态。成功设置取消旧 REP 比较与待投递故障；PC/SP 改变时取消旧等待、释放目标引用并丢弃被新栈越过的回调，窗口回调的未完成资源按原清理协议归还。PC/SP 未变时保留已捕获的等待参数和首次 deadline。装载通知帧仍由装载器拥有，期间更改 PC/SP 返回错误 170；越过活动展开预检/cleanup 也返回 170，等待完整装载边界和 collided-unwind 协议。计数仍为进程累计值，设置上下文不会回滚 work 或退休数。
