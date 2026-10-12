# Unity SharedStatic、job reflection 与实际 worker TLS

实现位于主目录 `/Users/alphaly/Projects/wire` 的 `main`。旧 worktree 仅保留冻结 runtime、
原始注册表、IL 和历史诊断材料。游戏程序集和冻结 Mono runtime 未修改；没有生成 JIT、
动态 trampoline 或宿主 executable mappings。完整游戏 backend 仍为 false。

## SharedStatic 使用同一 backing

固定入口 `Unity.Burst.LowLevel.BurstCompilerService::GetOrCreateSharedMemory` 的实际 ABI 为
`Void*(Hash128&, UInt32 size, UInt32 alignment)`；method token `0x0600046a`，原始注册目标
`0x1800694a0`。按真实 metadata 核对 Hash128 两个 UInt64 字段、16B size 和8B alignment。

原始 engine lookup 的反汇编 `0x180309210` 表明：size=0 返回 NULL；alignment=0 使用4；
同 key 只有 size/alignment 一致时复用地址，否则返回 NULL；新分配实际清零。每次请求都执行
这个原始 lookup，保留实际 NULL，不用桥的缓存冒充 engine 返回。

成功的新分配在 exact guest byte interval 安装 retained HostRegion overlay：native IL 与
guest Memory/read/write/RMW 访问同一 backing。邻居 bytes 不映射，跨 overlay 边界的访问
拒绝；owned noncode 快路径排除 overlay，防止缓存绕过共享 backing。底层页的 protect、
commit、release、decommit 以及一般 shared unmap 在活跃期拒绝。

ExecutionLease 是持有 HostRegion 的同线程、不可 Send lease，覆盖整个 native invocation，
不只覆盖 adapter 的瞬时 copy。调用中新增 region 立即加入 scope；真实 finalizer 的公开
profiler begin/end 也持相同 leases。cleanup 等待 finalizers 前释放 owner leases，cleanup
返回后恢复最终 bytes 到原 guest allocation，再 revoke/remove overlay。

当前诊断只接受8B引擎句柄槽，最多256项、1MiB。每次 guest callback 前及 native invocation
完成时，内容必须为 NULL 或实际 live guest engine data handle；拒绝宿主地址。它不开放
任意 SharedStatic pointer graph、Burst codegen 或 job scheduling/execution，也不改变
BackendSession/production Admission。

## 原始 job reflection 注册

`Unity.Jobs.LowLevel.Unsafe.JobsUtility::CreateJobReflectionData` 为实际
`IntPtr(Type wrapperJobType, Type userJobType, Object function0, Object function1, Object function2)`；
token `0x0600013e`，原始目标 `0x18005fdb0`。

两个 Type 转换为现有 rooted RuntimeType tokens；非 NULL function 核对真实 CLR Delegate
assignability，并借用 canonical rooted object registry。完整5参数调用进入原始 engine，
返回值必须是实际 guest allocation handle。metadata/v2 handles 的受检重入完成真实注册；
不传递宿主函数指针，不创建动态 callback，不把注册成功当作 job 已能执行。

固定 Win64 guest adapter 支持最多8个整数参数：前4个进入 RCX/RDX/R8/R9，arg5..8 位于
RSP+40..64。浮点参数仍只支持前4个 ABI 位置。`fixed_typed_icall` 只核对这些提前编译的
固定 adapters，普通 managed Frame 的 raw-pointer admission 没有扩大。

## 真实 finalizer Free 与日志

冻结 runtime 没有导出 `mono_gc_is_finalizer_thread`。公开的
`mono_profiler_set_gc_finalizing_object_callback` / `gc_finalized_object_callback` 在冻结
`mono/metadata/gc.c` 中紧密包围实际 managed finalizer 调用，桥以此建立真实 TLS scope。

foreign Free 仅能在该 scope 中提交 exact live native base 与 matching allocator，状态为
Live → Queued → Releasing → Retired。foreign 线程不访问 Memory/Runtime/native bytes，
不等待 owner；owner 在入口/出口及 shutdown drain，实际执行 Unity Free。pending/retired
span 拒绝访问，锁不跨 guest callback，host storage 留到 Mono cleanup 返回以防地址复用。

`DebugLogHandler.Internal_Log_Injected` 的 LogType/LogOption/ManagedSpanWrapper metadata 与
Win64参数位置已核对，复制 owned UTF16 后转发原始 engine；只接受 NULL context。
`Internal_LogException_Injected` 仍未接通。

## 有限 worker TLS 生命周期

冻结 `mono_domain_get` 读取当前 OS 线程 TLS，而非全局根域。每个 guest worker 对应一个
独立 native OS thread，容量32；接通 domain/root 查询、root attach、current 与 detach。
domain、Thread object 与 v2 strong handle 留在该实际线程，guest 只收到不映射对象头部的
opaque token；每次重新 attach 使用新 generation，其他 worker/已退休 token 拒绝。

命令/结果队列只携带 scalars。attached worker 在等待时进入/退出实际 GC Safe region，
marker 地址在整个 enter/exit interval 保持稳定；owner 等待也配对进入/退出。当前仅准入
已经验证的 preemptive suspend 模式，不宣称 iOS coop scheduler 已接通。

worker registration C API 产生的 GC/resize profiler 事件以 owned scalar queue 同步交回
解释器 owner，并使用真实 paused guest worker TEB；仅许可固定 profiler targets，不安装
metadata route，不准入 worker managed IL。worker 不持 SharedStatic lease、不借用
Memory/Runtime。关闭时先真实 detach/join，再释放其他对象 roots 和清理 VM。

另外15个逐项核对冻结source的不可变metadata getter（names、parent/element/nesting/type、
type token/rank、method class、field parent、signature return/count/instance）可以由进程owner
串行执行；每次先查询实际worker domain，确认仍attach到owned root。它们不读取current-domain、
不执行managed IL、不触发lazy field/signature setup；其他metadata操作仍需单独线程契约。
返回names复制到owned CString arena，handle继续canonical registry转换，不跨队列发送指针。
退出前还执行真实max-generation GC，再逐个worker查询current/strong target，核对token身份。

guest worker 的 Thread reference 保存要求 NULL object、8B aligned live C++ data slot，或
真实 retained worker stack interval；不采用可任意改写的 TEB bounds 作为唯一依据。
非 NULL value 重新查询实际 worker current/strong identity 后才保存。guest slot 不进入
native write barrier，宿主 Thread 地址不写给 guest。

## 真实游戏证据

主目录 `wire-out/native-mono-unity-assets-probe-75`：28个真实 SharedStatic 与28次 job
reflection 注册；1次真实 finalizer Free queued/drained，pending=0；GC disable1/enable1、
unwind0、depth0，guard238/0。停在 guest worker 3 的首次 mono_domain_get，有序错误退出。

`probe-76` 已越过该边界：实际 detached worker 的 domain_get 两次返回 NULL，随后真实
root attach 返回独立强根 token；退出时 attach1/detach1、live0、closed=true，guard241/0。
下一请求是 worker `mono_gc_wbarrier_set_field`，随后补受检 Thread reference 保存。

`probe-77` 已实测受检Thread保存：NULL object，actual worker stack slot
`0x7fe00c0ff680`，independent token `0x6ffe00001008`；随后止于class_get_name，未伪造name。
追加15个thread-neutral getters与真实worker GC验证后另行复测。

这些失败诊断的有序 teardown 不等于完整成功：最终对象/数组/fields 的强制 GC 验证尚未
全部执行，最新完整成功检查点仍为 probe-41。不可见314×211 guest window、0draw/0Present
不是正式窗口验收；诊断耗时不构成端到端性能收益。

冻结 runtime SHA256：`c152197b7cf1623a3d40a51a36e1e44d888b77f337b1378bb979598d2fad2eb0`；
support SHA256：`34705bafc092d1e644b2c7e0873340b398d1a81dd3e51c31af37a9ba0a29cea9`。

`probe-78` 进一步实测不可变class name getter，停在worker class_get_fields。它涉及首次
field setup中的真实TLS递归状态，不能直接扩大thread-neutral getter名单。
退出前真实max-generation GC后，worker current/strong identity核对1次成功；attach1/detach1、
live0、closed=true，finalizer queued1/drained1/pending0，guard241/0。其他对象/字段的完整
最终GC snapshots仍未执行，整轮保持exit1/full_game_backend=false。

完整release feature回归：490 passed/7 ignored；默认配置380 passed/7 ignored。日志
`wire-out/native-mono-shared-static-worker-feature-tests.log`。新增回归覆盖exact共享overlay与
RMW/邻居/VM变更拒绝/恢复、finalizer来源/重复释放/owner drain、Win64 arg5/arg8、真实guest
worker TEB/retained stack核对，以及独立OS worker TLS、强根token generation、profiler队列
和配对detach/join。feature和默认all-target Clippy -D warnings均通过。
