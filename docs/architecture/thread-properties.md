# 线程句柄与属性

线程属性的 provider、参数与返回约定集中在 `win32/catalog/thread_properties.rs`。新 Wasm 入口在 PE32 与 Win64 均接入 OpenThread、GetThreadId、GetExitCodeThread、Resume/SuspendThread、Get/SetThreadPriority、Get/SetThreadErrorMode、SetThreadAffinityMask 和 SetThreadStackGuarantee；线程执行协议见[线程生命周期](thread-control.md)。

初始线程 ID 为 1，后续身份不回绕复用。GetCurrentThread 返回当前架构完整宽度的 −2；OpenThread 只接受仍存在的线程 ID，未知 ID 返回错误 87。运行中的线程可在关闭原句柄后重开；退出线程需要仍有句柄或等待保留身份。每个 VM 最多 1024 个活动线程别名，整个寿命最多 49152 个；使用普通文件与标准端点别名以外的独立保留区间，耗尽返回错误 8。关闭或复制不会结束线程。

线程句柄有独立权限和继承/防关闭 flags，与普通句柄共用 CloseHandle、DuplicateHandle、Get/SetHandleInformation。支持 SYNCHRONIZE、QUERY_INFORMATION/LIMITED_INFORMATION、SET_INFORMATION/LIMITED_INFORMATION 和 SUSPEND_RESUME；THREAD_ALL_ACCESS 映射到该集合，其他访问位返回错误 50。复制只能保留或缩减源权限，越权返回错误 5；查询、挂起、设置和等待分别检查所需权限。复制当前线程 pseudo handle 产生普通独立别名。

PE32 和 Win64 均使用最多 127 次的挂起计数；SuspendThread 增一，ResumeThread 饱和减一，返回修改前计数。对退出线程操作返回错误 6；溢出返回 DWORD MAX 和错误 87。当前线程的挂起在完成服务返回帧之后阻止继续执行。

优先级接受 −15、−2…2、15，读取负值返回零扩展 DWORD；它保存客体属性，当前 round robin 调度不使用优先级排序。查询失败返回 THREAD_PRIORITY_ERROR_RETURN。affinity 只接受单核 mask 1，其余返回错误 87。GetExitCodeThread 成功写 DWORD，活动线程为 STILL_ACTIVE（259），退出线程为保存的 code；输出失败保留原状态和 CPU。

error mode 属于线程，初始为零；有效 mask 为 0x8003，进程专用的 SEM_NOALIGNMENTFAULTEXCEPT 在线程接口返回错误 87。设置时按 DWORD 截断输入，先验证 mask，再向非空输出写旧值，最后提交新值。输出故障不改变 mode，成功保留 LastError。进程默认策略见[进程选项](process-options.md)。SetThreadStackGuarantee 的零请求查询并写零；非零请求保持输入并返回错误 50，尚无异常栈预留。

调用帧在任何输出前保存参数、返回地址及栈完成位置，因此输出覆盖原返回槽也不改变返回目标。接口失败按其约定返回零、DWORD MAX 或优先级错误值并写当前线程的 LastError；成功不清除 LastError。

Get/SetThreadLocale 属于 [NLS 契约](nls.md)，在当前线程 TEB 保存 LCID；新线程使用用户默认 locale，后续修改只影响自身及其 CP_THREAD_ACP。进程默认 ACP、用户/系统 locale 保持独立，回调、让出和等待不回滚当前线程的 locale。

SetThreadDescription/GetThreadDescription 在两种 ABI 接受 kernel32/kernelbase，名称属于线程对象，句柄别名共享；已退出但仍有句柄保留的线程可继续查询。设置需要 SET_INFORMATION 或 SET_LIMITED_INFORMATION，查询需要 QUERY_INFORMATION 或 QUERY_LIMITED_INFORMATION。名称保存原始 UTF-16 单元，最多 32767 个，不将未配对代理项改写为替换字符；NULL 设置为空名称。

GetThreadDescription 每次分配独立的、含 NUL 的进程本地块，通过指针宽度的输出写出，调用者使用 LocalFree 释放。旧结果不随名称变化而改变，空名称也返回可释放的两字节块。输出在分配前预检，字符串读取失败不改变旧名称；输出故障不分配或提交 CPU/work。无效句柄或权限错误的查询写 NULL 输出，错误经 NTSTATUS 转为 HRESULT，LastError 保持；调用者使用 SUCCEEDED/FAILED 判断结果。调用不采样时钟，静态启动失败清理名称和分配。
