# 客体异常投递与 CONTEXT

新 Wasm 入口在 PE32 与 Win64 均支持 AddVectoredExceptionHandler、RemoveVectoredExceptionHandler、RaiseException、SetUnhandledExceptionFilter、UnhandledExceptionFilter 和 RtlPcToFileHeader，以及各自 ABI 的 RtlUnwind；Win64 另支持 RtlAddFunctionTable、RtlInstallFunctionTableCallback、RtlDeleteFunctionTable、RtlLookupFunctionEntry、RtlVirtualUnwind 和目标帧形式的 RtlUnwindEx。ABI 与 provider 使用 `win32/catalog/exceptions.rs`；目录中的旧适配器策略不扩大新入口的实现范围。

VEH 注册状态复用独立于内存和执行器的便携 owner。最多 64 个活动 handler、256 个累计 token，token 单调递增而不复用；NULL target 返回错误 87，容量耗尽返回错误 8。first 非零插到队首，否则追加。删除返回 1/0，成功注册、删除和 filter 设置保持 LastError；错误槽故障不创建注册。每次异常保存开始搜索时的 VEH 顺序。

RaiseException 清除 code 的保留 debugger 位，只保留 NONCONTINUABLE 标志，最多捕获 15 个指针宽度的信息参数，NULL 参数数组表示零个参数。记录地址采用 API 的客体返回 PC，CONTEXT 表示 API 完成后的调用者。参数、记录、CONTEXT 和首个 callback frame 在状态发布前受检；输出故障不计 API work，也不进入 handler。成功的 RaiseException 计一 work，handler 指令及其服务使用同一累计预算。

OutputDebugStringA 产生 DBG_PRINTEXCEPTION_C，信息为含 NUL 的字节长度和字符串指针；W 首先产生 DBG_PRINTEXCEPTION_WIDE_C，四个信息值依次为含 NUL 的 UTF-16 长度、原指针、ANSI 字节长度和临时 ANSI 指针。VEH 可以继续执行以接收事件；W 的 VEH 全部继续搜索时，再投递 ANSI 事件，并重新取得此时的 VEH 顺序。调试输出的内部处理先于调用者的 SEH/语言搜索和顶层 filter，搜索耗尽后完成原 API；显式 RaiseException 即使使用相同 code，也沿普通异常搜索处理。

调试处理器仍在同一 JIT、线程和累计预算下执行，支持嵌套输出、让出与 CONTEXT 恢复。A 的未处理分支恢复该 ANSI 调用开始时的 LastError；已被 VEH 接收时保留 handler 的改动。W 的 ANSI 分支以进入 fallback 时的 LastError 为基准。原字符串、转换、记录及首 callback 预检失败不进入 handler；宽转窄续点或最终 LastError 写回故障保留已完成 handler，修复后不重放。临时 ANSI 字节随本次异常续点存活。外部调试事件传输和系统输出接收端尚未接入。

DebugBreak 在两种 ABI 接受 kernel32/kernelbase，投递可继续的 STATUS_BREAKPOINT。ExceptionAddress 与 CONTEXT 的 PC 指向逻辑 DebugBreak 入口，SP 保持 API 调用时的栈；PE32 的信息参数为三个零，AMD64 为一个零。处理器可以修改 CONTEXT 后继续：PC 增一表示跳过断点，随后从恢复后的栈读取返回地址；改变 PC 可以转到其他客体代码，保持原 PC 会再次触发断点。返回栈故障保留已完成处理器的结果，修复后继续返回。

断点沿有序 VEH、PE32 注册链、AMD64 语言搜索及顶层 filter 的普通异常路径执行，未处理时保持 Exception 停止。AMD64 搜索将逻辑断点入口视为 leaf，从调用者的函数元数据继续；处理器中的目标展开使用保存的逻辑调用者桥接，支持越过本次断点续点。嵌套、让出、DLL 通知和单 work 预算沿原异常协议处理；DebugBreak 本身不采样时钟或修改 LastError。客体 INT3 指令与外部调试器传输仍待接入。

映像执行中的 #DE、已识别的 #UD/#GP、#MF/#XM 及普通指令的访存/取指故障进入同一边界。访存记录包含读（0）、写（1）或执行（8）和精确地址；guard 使用 STATUS_GUARD_PAGE_VIOLATION，其他访存使用 STATUS_ACCESS_VIOLATION。#DE 使用 STATUS_INTEGER_DIVIDE_BY_ZERO，#UD 使用 STATUS_ILLEGAL_INSTRUCTION；#MF/#XM 按未屏蔽的浮点状态选择 Windows 代码。#GP 当前映射为访问异常，信息地址为零；其具体故障操作数地址尚未从生成代码传到 Windows 记录。没有可搜索 handler 时保留原始 Fault/Exception 出口，裸 CPU 模式继续报告原始出口。

VEH 返回 EXCEPTION_CONTINUE_EXECUTION（-1）时从受检 CONTEXT 恢复；返回 EXCEPTION_CONTINUE_SEARCH（0）时继续搜索。PE32 随后读取当前线程 FS 注册链，handler 使用四个参数和 EXCEPTION_DISPOSITION：0 继续执行，1 继续搜索。链最多 128 项，检查栈范围、对齐及环路；搜索结束后可执行顶层 filter，-1 继续执行，0 保持未处理。自动 filter 的 EXECUTE_HANDLER 仍待接入。Win64 在 VEH 之后按运行时函数表跨帧搜索语言 handler，搜索结束后使用顶层 filter 的继续/搜索返回约定。

显式 UnhandledExceptionFilter 在两种 ABI 下接受 kernel32/kernelbase，调用当前注册 filter，透传完整宽度的 EXCEPTION_POINTERS，不在运行时读取或解释该指针。无 filter 时直接返回零；有 filter 时在同一线程/JIT 续点执行，返回原 LONG 并恢复 API 调用者的完整状态，保留累计预算。它不会按 filter 返回值自动恢复 CONTEXT 或终止进程。AMD64 filter 的客体帧参与正常语言搜索和目标展开，跨回调返回边界时使用保存的 API 调用者。

PE32 RtlUnwind 与旧入口共用注册链预检：最多 128 帧、四字节对齐、TEB 栈范围、严格递增 next 和目标可达性在清理前验证。目标 registration 保持安装且不调用；NULL 目标清理到链尾。外部异常记录保留原内容，只加入 UNWINDING 和必要的 EXIT_UNWIND；NULL 记录生成 STATUS_UNWIND。各 handler 接收 record、registration、CONTEXT、dispatcher，必须返回 ContinueSearch（1）；成功后确认 FS 头和 next 未改，再摘除本帧。已完成结果与摘链进度在后续故障前保存，修复后不重放清理。最终从可编辑 CONTEXT 恢复调用者；i386 TargetIp 按原 ABI 忽略，ReturnValue 初始化 EAX。handler 可让出、调用服务或装载 DLL；nested/collided 展开和跨装载边界的非局部转移仍明确拒绝。客体 CRT 恢复 catch 栈后，下个服务边界回收被越过的回调及等待引用，避免后续异常累积旧续点；活动 cleanup 和装载通知不能通过此规则丢弃。

AMD64 静态函数表来自各映像的异常目录，随 DLL 映射发布和卸载消失。装载时检查 12 字节记录的长度、排序、重叠、代码范围和展开地址；失败不发布映像。动态数组由调用者拥有，与回调表共享 128 项注册容量；添加检查完整数组和地址范围，删除只移除动态注册。查询优先搜索静态映像，再搜索动态注册，按当前内存读取记录，写出完整 image base；未找到时返回 NULL 并写零。输出和保存的返回帧可以别名，输出故障不完成调用或改变 LastError；动态数组在调用者删除前必须保持有效。HistoryTable 为可选缓存提示，接受非 NULL 值而不读取或更新缓存；始终查询当前有效映像和动态注册。这与 [Wine 的公开查询实现](https://github.com/wine-mirror/wine/blob/master/dlls/ntdll/unwind.c)相同，缓存内容不授权已卸载或已删除的表。

RtlInstallFunctionTableCallback 要求 TableIdentifier 的低两位均为 1、非零长度/回调和规范的有界地址范围。可选 OutOfProcessCallbackDll 必须是有效且在 32768 个 UTF-16 单元内终止的字符串，仅作为注册元数据保存，不触发 DLL 装载或宿主 I/O。命中范围后在客体 JIT 调用 `(ControlPc, Context)`，参数及返回指针保持完整 64 位；NULL 表示未找到，非 NULL 必须指向完整、覆盖查询 PC 的 12 字节记录。显式查询、异常搜索及目标展开预检共用这条回调路径。请求保存发起时的范围基址与回调参数，回调内删除或替换注册只影响后续查询。返回记录、输出或后续展开元数据故障保留已完成回调的结果，修复后不重放回调。

AMD64 虚拟展开支持 v1/v2 元数据、已完成 prologue 操作、已识别 epilogue、大小栈分配、frame pointer、GPR/XMM 保存、machine frame、chained 和间接运行时函数记录。每次只计算调用者上下文，语言 handler 仍作为客体代码在同一 JIT 中执行。普通返回帧使用返回地址减一的 control PC，硬件异常使用实际 PC；epilogue 检查使用该 control PC，避免跳过 CALL 后仍应参与搜索的 handler。搜索最多 128 帧，检查当前线程栈范围、规范地址、进展和环路；遇到逻辑回调/主入口/线程返回边界停止搜索。

[RtlVirtualUnwind](https://learn.microsoft.com/en-us/windows/win32/api/winnt/nf-winnt-rtlvirtualunwind) 在 AMD64 接受 kernel32/kernelbase/ntdll，展开调用者提供的 CONTEXT、ControlPc 和 FunctionEntry，保持当前执行线程的 CPU 状态。该 CONTEXT 的 ContextFlags、段、FLAGS、MXCSR 与其余未恢复数据保持原字节，不执行异常继续时的选择子或浮点控制校验。栈可以位于调用者提供的可读内存，不受当前 TEB 栈范围约束；内部异常搜索继续检查当前线程栈。保存寄存器的偏移相对固定帧计算，支持 frame pointer 下的动态栈空间。

返回值是匹配 HandlerType 的语言 handler 地址，调用本身不执行 handler；prologue、epilogue 或没有匹配 handler 时返回 NULL。有匹配项才写 HandlerData；NULL FunctionEntry 按 leaf 弹出返回地址，HandlerType 非零时清零 HandlerData。EstablisherFrame 为必需输出。可选 KNONVOLATILE_CONTEXT_POINTERS 只更新实际恢复的 GPR/XMM 保存地址，其余槽保持原值，RSP 槽不写；machine frame 只恢复 CONTEXT 的 RIP/RSP。所有需要写入的输出统一预检后提交，访存或元数据错误保留原 CONTEXT、输出、CPU 和 work；不支持或畸形元数据明确报错。成功调用计一 work，不读取平台时钟。

语言 handler 接收异常记录、establisher frame、CONTEXT 和 DISPATCHER_CONTEXT，返回 0 继续执行、1 继续搜索。RtlUnwindEx 先验证到目标帧的路径，期间可以暂停并执行函数表查询回调；整个路径验证成功后才从内到外执行具有 unwind 标志的 handler，目标帧额外带 TARGET_UNWIND。完成后写回目标 CONTEXT，恢复目标帧的非易失 GPR/XMM 和栈，转到 TargetIp，并将完整 ReturnValue 放入 RAX；丢弃被越过的回调续点。各 cleanup 使用同一预算并可以调用普通服务或让出；准备下一帧或最终 CONTEXT 写回失败不会重放完成的 cleanup。

普通客体代码可以直接调用 AMD64 RtlUnwind 或 RtlUnwindEx，从保存的 API 调用者 PC/SP 开始展开，不需要先发生异常。四参数 RtlUnwind 的 ReturnValue 保留完整指针宽度，CONTEXT 使用本次展开的内部客体栈存储，生命周期覆盖函数表查询和全部 cleanup；它共用目标展开器及故障重试规则，不读取额外栈参数。RtlUnwindEx 调用者提供的 16 字节对齐 ContextRecord 用作上下文存储，初始内容不参与恢复；这符合 [RtlUnwindEx 的存储契约](https://learn.microsoft.com/en-us/windows/win32/api/winnt/nf-winnt-rtlunwindex)。外部 ExceptionRecord 保持地址、链接及参数，只更新展开标志；NULL 记录产生 STATUS_UNWIND、零参数和调用者地址。记录读取/标志写入、上下文及内部帧均在接受 API 前受检，失败不计 work 或修改输出。接受后使用每线程续点推进查询和 cleanup，API 只计一次 work。初始、各 cleanup 及最终上下文写回都使用该入口选定的同一存储。AMD64 两个展开入口均由 kernel32/kernelbase/ntdll 提供；NULL 目标的 exit unwind、collided unwind 和特殊恢复协议仍明确拒绝。

目标展开可跨当前线程、同一装载深度的异常 handler/filter、窗口过程、显式函数表查询及异常搜索中的查询回调。先按元数据遍历回调自身的客体帧，再以 Callback 网关和完整返回 SP 匹配已捕获的续点，接回运行时保存的调用者状态；回调帧上的 cleanup 因此先于原调用者的 cleanup。嵌套边界按实际栈顺序遍历，预算暂停和元数据故障保留进度。到达目标后仅丢弃被越过的续点；目标仍在外层回调中时保留该外层回调，普通返回继续其原协议。被放弃的查询不写出结果，也不重复收取原 API work。

当前要求可达的非零目标帧。HistoryTable 同样不用于缓存查找，其原始指针传递给 cleanup 的 DISPATCHER_CONTEXT。活动展开预检/cleanup 的边界需要 collided-unwind 协议，当前拒绝跨越；DLL 初始化及装载通知帧仍由装载器拥有，不能被普通目标展开丢弃。exit unwind、nested/collided disposition、STATUS_LONGJUMP 及 STATUS_UNWIND_CONSOLIDATE 的特殊恢复协议仍未实现，明确报错。

尝试继续 NONCONTINUABLE 异常、未知 disposition、非法 CONTEXT 或注册链均报告明确错误，不按成功返回。搜索耗尽后返回 Exception 和 Windows code，重复 run 保持该停止状态而不重放 handler；外部 CPU 编辑可以解除此停止。处理期间发生的客体异常支持嵌套投递，最多 32 层，每线程独立保留续点。

CONTEXT 使用 Windows i386/AMD64 布局，支持控制、整数、固定用户段选择子及浮点状态；i386 还支持旧式 FLOATING_SAVE_AREA 与 ExtendedRegisters。恢复遵循 ContextFlags，拒绝调试寄存器等未实现组件、非规范 AMD64 PC/SP、特权 FLAGS 或段选择子更改及非法 MXCSR。i386 同时请求浮点和扩展状态时以扩展状态为准；AMD64 的两份 MXCSR 必须一致。x87 CW 按已实现配置规范化，状态汇总位及物理标签重建，XMM 和 MXCSR 随异常上下文恢复。

异常记录及上下文放在客体栈中，客体可编辑 CONTEXT；读取或写入失败保留当前控制阶段。硬件异常在投递帧故障时保存原始记录，修复后继续投递；handler 已返回后的 CONTEXT 读取故障不重放 handler。通用返回和装载回调的交错规则见[回调边界](callbacks32.md)。

底层 wire_deliver_exception(owner, reason) 在驻留执行租约结束后使用，reason 为 Fault（3）或 Exception（5）；返回未接管（0）、已进入投递（1）、投递访存故障（2）或接口错误（3）。wire_startup 同时推进装载、待投递异常和回调返回，新增状态 3 表示未处理 Windows 异常。wire_exception_code 返回 code+1 的 u64，零表示没有 Windows code；Web 的 Exception 结果据此返回 exception.code，原始 CPU 出口仍返回 exception.vector。
