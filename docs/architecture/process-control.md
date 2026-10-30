# 进程句柄、查询与退出

新 Wasm 入口在 PE32/AMD64 均接受 kernel32/kernelbase 的 OpenProcess、GetProcessId、GetExitCodeProcess、Get/SetProcessAffinityMask、SetThreadIdealProcessor、四项处理器/group 数量查询和 GetCurrentPackageId。

根进程默认为 PID 1，子进程由同一组分配不复用的公开 PID；初始 TID 为 (PID−1)×65536+1，worker 使用所属进程的后续身份。OpenProcess 可以打开同组已发布的进程，未知或未发布 PID 返回错误 87；inherit 标志保存在新句柄上。接受 0 至 PROCESS_ALL_ACCESS 范围内的具体权限位，generic/security 扩展访问掩码尚不支持，返回错误 50。零权限句柄可以关闭、复制和查询标志，不能执行需要查询或同步权限的操作。

进程伪句柄为客体指针宽度的 -1。DuplicateHandle 可以把它转换成真实句柄，也可以复制已有进程句柄；源/目标进程参数接受伪句柄或具有 PROCESS_DUP_HANDLE 的当前进程句柄；跨进程复制返回错误 50。权限只能保持或缩减。新句柄默认不继承 protect-from-close 标志，inherit 由调用者指定；close-source 在输出成功后关闭原真实句柄。CloseHandle、Get/SetHandleInformation 与其他句柄种类共用调用边界，protect-from-close 拒绝关闭。每 VM 最多保留 1024 个进程句柄，身份递增且关闭后不复用；静态启动拒绝清理全部进程句柄。

GetProcessId、GetExitCodeProcess 和 GetProcessAffinityMask 要求 PROCESS_QUERY_INFORMATION 或 PROCESS_QUERY_LIMITED_INFORMATION。目标进程在运行期间返回 STILL_ACTIVE，退出后返回其实际 DWORD 退出码。亲和性查询按客体指针宽度写两个 mask，统一预检后发布，均为 1；SetProcessAffinityMask 要求 PROCESS_SET_INFORMATION，只接受 mask 1。SetThreadIdealProcessor 要求 THREAD_SET_INFORMATION，只接受处理器 0，或以架构的 MAXIMUM_PROCESSORS 查询当前值；返回原处理器 0，失败返回 DWORD_MAX 并设置 LastError。

活动/最大处理器数和 group 数均为 1。处理器数量查询读取低 WORD group，仅接受 0 或 ALL_PROCESSOR_GROUPS。进程未打包，GetCurrentPackageId 返回 APPMODEL_ERROR_NO_PACKAGE，不访问输出指针、不改变 LastError。其余查询成功保留 LastError，完成计一 work、不增加退休指令、不采样时钟；参数和输出故障沿公共受检访问与保存帧协议恢复。

GetProcessMemoryInfo（psapi）和 K32GetProcessMemoryInfo（kernel32/kernelbase）支持当前进程，权限为 PROCESS_QUERY_INFORMATION 或 PROCESS_QUERY_LIMITED_INFORMATION。有效远程进程句柄返回错误 50。小于基础结构的 DWORD 容量先返回 FALSE/122，不访问句柄或输出。基础结构为 40/72 字节，容量达到 44/80 字节时写扩展结构；cb 写实际结构长度，较大缓冲区的尾部保持。查询不读取输入 cb。EX2 的额外字段尚未实现。

工作集为全部已提交 backing 页的字节数；私有提交量与 PrivateUsage 只统计 MEM_PRIVATE，排除 MEM_IMAGE 和 MEM_MAPPED。各峰值在成功映射/提交时维护，解除提交/释放保留峰值，重复提交不重复计费，加载临时空间的历史峰值不合入进程。当前 VMM 全部即时建立 backing，没有分页、按需缺页或客体内核 pool，对应计数为零。输出使用完整预检，成功保留 LastError。接口依据 [GetProcessMemoryInfo](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo) 和 PROCESS_MEMORY_COUNTERS/EX 布局；计数描述的是客体 VMM。

真实进程句柄可用于[Ex 内存接口](virtual-memory.md)与[混合等待](waits.md)。VirtualAlloc/Free/ProtectEx 要求 PROCESS_VM_OPERATION，VirtualQueryEx 要求 PROCESS_QUERY_INFORMATION；这些 Ex 内存接口与 Read/WriteProcessMemory 当前只操作当前进程，合法远程句柄返回错误 50；FlushInstructionCache 的真实句柄要求 PROCESS_VM_WRITE。进程等待要求 SYNCHRONIZE，目标运行期间保持未就绪，等待身份独立于句柄关闭。等待超时与信号消费沿统一等待协议处理。

ExitProcess 与 TerminateProcess 接受两种 ABI 的 kernel32/kernelbase 入口。成功调用不读取普通返回地址，接受的退出操作计一 work、不计客体退休、不采样时钟；退出码保存完整 DWORD，包括 STILL_ACTIVE 的数值 259。TerminateProcess 接受当前进程伪句柄或具有 PROCESS_TERMINATE 的同组进程真实句柄；远程成功表示已提交终止请求，目标在下一次调度时完成终止，已退出目标返回错误 5；无效句柄和权限不足分别返回 FALSE/错误 6 和 5，沿普通调用帧与错误槽故障协议处理。

ExitProcess 先等待加载锁可用，再停止其余线程，放弃这些线程持有的互斥体；临界区仍由原线程持有。其余线程不执行 FLS 或 DLL_THREAD_DETACH。调用线程先执行 FLS 析构，再按依赖分离顺序执行 DLL_PROCESS_DETACH；DllMain 的 reserved 非零，TLS 回调沿现有通知契约接收零。通知使用同一 JIT 和预算，后续帧故障不会重放已完成通知。终止其他线程后，分离通知尝试获取这些线程持有的临界区会保持等待。当前正常退出拒绝活动加载通知和递归进程关闭调用；这些调用在明确报错前保持原进程和 CPU 状态。

TerminateProcess 直接停止全部线程，跳过 FLS 和 DLL 通知，也可在活动加载或退出回调内强制终止。两种终止方式完成后均关闭活动句柄、标准输入、管道和 USER 对象，取消挂起服务及回调，撤销全部生成模块身份和 JS 安装实例。客体内存、堆 backing、环境、模块元数据和已收集输出保留到 VM 销毁，供宿主读取最终快照与排空输出；它们不再支持客体执行。

Web `processExitCode` 在进程运行期间为 null，终止后为稳定的 DWORD Number。正预算 `run()` 和 `service()` 返回 `Exit.ProcessExited`（12）及 `exitCode`，重复调用不增加 work 或退休数，宿主编辑 PC/寄存器也不会恢复执行。`run(0)` 保持零预算契约并返回 Budget，退出码仍可查询。核心 `wire_process_exit_code` 在尚未退出时返回 u64::MAX；服务与调度导出的状态 5 表示进程终止。单进程普通主入口 Returned 与最后线程 ThreadReturned 保持原有协议；多进程组会将入口返回转为正常进程退出并完成通知。

生命周期顺序依据 [ExitProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-exitprocess)、[TerminateProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess) 和 Wine 的进程 FLS/装载器关闭顺序。CreateProcessW 支持范围见[进程启动](process-startup.md)。组内进程的查询和主线程句柄独立于创建者的地址空间；关闭句柄不丢失在途等待的退出状态。已接入的进程错误模式和启动属性准备接口见[进程选项](process-options.md)；标准输入输出与管道见[文件接口](files.md)。

GetPriorityClass 要求查询权限，默认 NORMAL。SetPriorityClass 要求 PROCESS_SET_INFORMATION，持久保存 NORMAL/IDLE/HIGH/REALTIME/BELOW_NORMAL/ABOVE_NORMAL 六类，影响虚拟记录而无宿主调度设置；其他普通数值为 87，BACKGROUND_BEGIN/END 因缺少调度后端为 50。子进程继承 IDLE/BELOW_NORMAL，否则为 NORMAL。

EnumProcesses（psapi）和 K32EnumProcesses（kernel32/kernelbase）按升序枚举同组已发布且未退出的 PID；容量为字节，最多写 capacity/4 项，needed 为实际复制字节。数组与 needed 统一预检。

GetProcessTimes、GetProcessWorkingSetSize 在验证查询权限后返回 FALSE/50并保持输出；没有每进程 CPU 时间或工作集最小/最大策略。SetProcessWorkingSetSize 要求 PROCESS_SET_QUOTA 后返回 FALSE/50。WaitForInputIdle（user32）要求 SYNCHRONIZE 后返回 WAIT_FAILED/50，没有 GUI 启动空闲握手；不等待或采样时钟。实际内存使用量继续使用 GetProcessMemoryInfo。
