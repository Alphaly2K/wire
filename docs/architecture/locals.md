# TLS/FLS 调用边界

新 Wasm 入口在两种 ABI 支持 TlsAlloc/GetValue/SetValue/Free 和 FlsAlloc/GetValue/SetValue/Free，使用共享 catalog 的八个契约。PE32 provider 为 kernel32，Win64 为 kernel32/kernelbase；索引按 DWORD 截断，值与析构地址保留指针宽度。

进程复用便携槽标记组件，提供 64 个动态 TLS 和 128 个 FLS 索引，均从零分配并复用空槽。每个线程独立保存值；尚无独立 fiber 对象，FLS 当前对应每线程的默认 fiber。动态 TLS 与 PE 模块的静态 TLS 索引和指针表相互独立。

初始线程和 worker 均有两页 TEB。PE32 动态 TLS 位于 `TEB+0xe10`，Win64 位于 `TEB+0x1480`；FLS 私有值区位于 `TEB+0x1800`。worker 静态 TLS 指针表仍位于第二页起始处，三者不重叠；PE32 初始 TEB 的两页范围避开 PEB 与共享用户数据地址。调用者通过装载信息取得 TEB，不能假定固定地址。

Alloc 先检查全部存活线程的目标槽可写，统一写零后才占用索引。无空槽返回 OUT_OF_INDEXES（0xffffffff）和错误 8。无效或已释放索引的 Get/Set/Free 返回零及错误 87。TlsGetValue 成功清零 LastError；FlsGetValue 成功保留 LastError，遵循各自的[公开](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-tlsgetvalue)[契约](https://learn.microsoft.com/en-us/windows/win32/api/fibersapi/nf-fibersapi-flsgetvalue)。其余成功操作保留 LastError。

TlsFree 检查并清空全部存活线程的对应值后释放索引，不释放值指向的内存。FlsFree 还为每个非零值捕获原析构地址与参数。全部槽和首个调用帧可写后，清空值并释放旧索引，再由调用线程依次执行普通 JIT 回调。回调可以嵌套调用服务、释放新索引或重新分配同一索引；在途队列持有旧地址/参数，完成时不修改替代索引。

worker 返回或显式 ExitThread 在 DLL_THREAD_DETACH 前收集当前线程的 FLS 析构。先预检并清零带析构的槽，再按索引顺序调用非零值，之后才通知 DLL 和释放线程资源。每个退出阶段收集一次；回调中重新写入的值不再次遍历。普通 EXE 原始入口 RET 仍交回宿主为 Returned，进程有序退出与 fiber 生命周期继续按 GOAL 中的范围推进。

析构使用与异常、窗口过程相同的保存状态和 CallFrame，PE32 接收一个 stdcall 栈参数，Win64 使用 RCX。FlsFree 完成恢复原调用者并返回 TRUE；退出析构保留退出码。后续调用帧故障保留剩余队列，不重放已经完成的回调。首帧故障不释放索引或清空值；参数和返回帧先保存。完成 API 计一 work，实际回调指令独立退休，推进队列不增加虚拟退休或时钟采样。

旧 PE32 的单调 map、旧 Win64 的受限析构适配器继续作为迁移来源；新入口直接复用槽标记，不调用旧执行器。跨退出析构的非局部展开、独立 fiber 创建/切换/销毁和进程退出仍待接入。
