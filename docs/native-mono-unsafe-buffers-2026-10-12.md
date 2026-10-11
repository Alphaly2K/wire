# 有限启动诊断的 private native IL buffers

这不是完整backend准入，也不是`src/mono/buffers.rs`的生产共享内存接入。
`mono_startup/unsafe_buffers.rs`只服务冻结runtime、实际owned image和同一主线程的同步IL诊断。
游戏原始程序集和冻结runtime不改，不生成JIT或动态trampoline。

## ABI 与两个地址

实际Malloc是`Void*(Int64 size, Int32 alignment, Allocator, IntPtr context)`，不是三参数。
Free是`Void(Void*, Allocator)`；MemSet是`Void(Void*, Byte, Int64)`。
整个注册批次先检查原始engine target和实际static/non-generic InternalCall metadata：
Void* element确实是System.Void，scalar exact class/width/alignment，Allocator是真实Int32 enum。

只准入size1..64MiB、power-of-two alignment1..4096、TempJob(3)/Persistent(4)、NULL context。
Temp(2)会被engine scheduler隐式退休，custom/context allocator有不同owner契约，当前明确拒绝。
包含未完成重入分配reservation和已退休native storage的总量≤64MiB，累计allocation≤4096。

先准备owned aligned native bytes，再调用原始Unity Malloc，保留真实NULL失败结果。
非NULL guest结果核对alignment、完整range的live owned writable noncode backing；读取其实际
初始bytes一次，初始化private native storage。只有native IL拿到该data pointer；guest engine
只拿原始guest allocation，Memory中没有host mapping，普通field/value/array ABI仍拒绝raw pointer。
不能把native地址直接当guest Engine API参数，不能声称两个地址是始终一致的shared alias。

MemSet只接受属于live private allocation的完整子区间，包括checked offset/overflow。
把对应guest地址传原始MemSet，核对其全部输出确实是fill byte；再次验证lifetime后才覆盖native
子区间。这里无需把任何native数据或指针写给guest。其他pointer型engine调用/共享访问
仍需独立typed契约；当前绝不按任意地址blind memcpy或假定native heap都可以直接映射。

MemCpy/MemMove在该冻结profile注册到同一guest实现。固定adapter核对实际Void(Void*,
Void*,Int64)；只接受两端均在live private allocation中的span，全部bounds/overflow先验证。
随后执行实际native byte copy，保留overlap；guest记录不作byte同步，也不发布native数据。
managed array pin、native IL栈、其他host区域的pointer尚未成为合法来源，不能盲目读任意host地址。

## 生命周期

native storage和registry是owner-thread-only，callback必须存在live synchronous guest owner。
不持registry/Memory RefCell borrow跨原始engine调用。
Free必须exact live native base、matching Allocator；interior/double-free/unknown pointer拒绝。
先调用实际Unity Free，再logical retirement；retired host storage保留到Mono cleanup返回，避免
host地址立即复用导致stale pointer撞上新allocation。保留storage不等于stale handle仍有效。

Mono cleanup前不得释放仍被IL/static/finalizers引用的bytes。cleanup返回后，诊断才通过原始
Unity Free清理剩余guest分配记录，随后drop全部private storage。report区分ordinary frees和
cleanup frees，记录live/retained/closed。真实成功probe还需检查GC disable/enable、final GC、
noexec guard和engine resource teardown，不能以一个Malloc局部成功代替。

该路径没有BackendSession的生产scheduler/SharedBacking lease，也没有跨线程访问准入。
不得因此将full_game_backend设true，或替换整个Mono allocator。要供真实game loop使用，仍需
在实际owner/session和typed pointer graph上接入共享buffer或者明确的copy边界。

## 当前证据

probe-64实际分配1886B、alignment8、Persistent(4)，下一缺失InternalCall是MemSet；其后abort。
不能将这次结果计为完整GC/cleanup成功。此前probe-62/63/64已局部通过真实JSON反序列化。
Mono定向80 tests与feature Clippy通过对应Malloc/Free源码；MemSet源码的最终回归另补。
单元回归覆盖native alignment/末byte读写、guest/native地址隔离、exact-range overflow、matching
allocator、interior/double-free拒绝、retired storage不复用、重入reservation容量、closed owner拒绝。


probe-65进一步实际完成两次allocation（1886B、1986B）和两次完整零填充，以及一次正常Free；
随后缺失MemCpy导致abort。完整feature release482 passed/7 ignored、Clippy clean覆盖该源码。
新MemCpy/MemMove及其两端越界/overlap/stale回归仍待probe-66和新Rust验证。


probe-67证明零长度copy是真实请求；从原始PE读取CRT jump-table[0]，target为0x181b8a14e
直接RET，无pointer访问。新adapter在size0时保留该语义，非零span的完整检查仍在。
该轮实际4 allocations/4 MemSets/33 copies/3 ordinary frees；随后未实现Screen API导致abort。
probe-70已实际越过Screen和对象查询；错误退出触发foreign finalizer Free时按现有owner契约
拒绝。native finalizer线程不能偷偷借用主线程Memory/Runtime，也不能将它当作已完成的生产
buffer生命周期。后续需要实际owner队列/停止/资源lease契约；当前没有该准入。


最终源码验证（2026-10-12）：release feature483 passed/7 ignored，default380 passed/7 ignored；
两种配置的all-target Clippy -D warnings均clean，最终feature release binary已重建。
日志：wire-out/native-mono-instance-reentry-tests.log、native-mono-instance-reentry-clippy.log、
native-mono-default-regression-tests.log、native-mono-default-regression-clippy.log。

最新真实probe-71已通过JobsUtility状态setter，随后缺失
Unity.Burst.LowLevel.BurstCompilerService.GetOrCreateSharedMemory，实际ABI为
Void*(ref UnityEngine.Hash128, UInt32 size, UInt32 alignment)，冻结target0x1800694a0。
它要求同一key的稳定共享存储、native IL与原始引擎可见性及退出owner/lease，不能直接返回
guest地址给native IL，也不能用当前private Malloc mirror冒充shared memory。
其错误路径还遇到DebugLogHandler.Internal_Log_Injected及foreign finalizer Free拒绝。
probe-62..71的局部进展仍未取得新的整轮GC/guard/cleanup验收，完整成功基线仍probe-41。
full_game_backend=false，没有正式游戏窗口或端到端性能收益声明。
