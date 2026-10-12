# 元数据驱动的普通 IL 调用、类型缓存与引擎重入

更新：2026-10-12。目标是执行真实托管方法，避免维护类名/构造函数名单。
`managed_calls.rs` 按 metadata 验证 static/reference instance method 和 typed ABI，冻结 ARM64 Mono interpreter
执行原始程序集 IL。没有生成 JIT、动态 trampoline 或修改游戏/冻结 runtime。
当前仍是有限启动诊断，完整 backend 准入为 false，尚无正式窗口和端到端性能验收。

最新后续已接SharedStatic同一backing、真实job reflection注册、finalizer Free owner队列
和独立native worker TLS；详见 `native-mono-burst-shared-static-2026-10-12.md`。
这些固定typed边界没有恢复普通managed方法/构造函数名单，也没有扩大Frame的raw-pointer准入。

## 入口与参数

普通入口必须位于实际 provider-owned live image registry：持有 native image 引用、记录 SHA256、
有效 user_refs 或 borrowed 状态，且未退休。method 非泛型/非 inflated，
且 declaring class 为实际闭合类型。拒绝 abstract、P/Invoke、InternalCall、native/runtime、
unmanaged、synchronized、无 IL 或超限 body。上限为16参数、65536B IL、256 max-stack。
不依据 method 名称/token 准入，APIUpdaterRuntimeHelpers 只是实际游戏触发的首个样本。

支持 exact primitive/enum scalar、owned image reference 输入，以及 scalar/String byref。
primitive/enum 的实际 class、width、alignment 核对，reference 以真实 assignability 检查。
非 String reference outputs、struct ABI、value receiver、method 泛型等尚未准入。

guest 参数向量只在受检 memory 中读取，scalar/byref 使用栈上固定16×8B owned slots，
传给 Mono 的是这些 host slots 或真实 rooted 对象，guest 地址不进入 native invoke ABI。
引用槽在 native stack 上，BDWGC 能扫描 IL 调用中写入的新引用；不能放在普通 Rust heap
的 Vec payload 中假定 GC 会扫描。相同地址/shape 的 byrefs 共用同一槽；partial overlap、
不同 shape overlap、parameter vector overlap、exception/byref overlap 全部拒绝。

byref/exception 输出必须在 live guest stack、heap allocation 或 committed private writable
VirtualAlloc 中，检查权限、alignment、边界与 code history。guest callbacks 后再次检查存储
仍存活。全部 native outputs 转换/校验成功后才写 guest。异常拒绝，不能伪造 false/NULL。

## 返回值、引用与同步

scalar 返回值保留真实 mono_runtime_invoke 生成的 box：核对 exact class、payload=object+16、
canonical Boolean，立即 pin 后安装 metadata-derived owned payload；不复制成另一对象。
String return/out 通过既有 canonical string registry，native identity 与 guest UTF16 view
保持一致。调用前同步 guest→native 字段/数组；调用后仍完整 graph prepare/commit refresh。

ordinary roots、RuntimeType roots、field cells、box payloads 上限提升至32768，以覆盖样本的
27184 TypeDefs 扫描；metadata handles 仍65536，strings 仍1024，typed arrays 仍256。
各 arena 不重叠，普通 root 的 native→token 与 RuntimeType 的反向索引改为 BTreeMap；
命中仍核对实际 native root identity，没有跳过 GC 检查。

## MonoClass 与弱句柄

UnityPlayer 的 0x1807dc660 类型缓存查询在 0x1807dc6b6 读取 MonoClass 首个字段计算 hash。
caller 在 stack 中存放 class pointer，实际字段是冻结 MSVC-profile MonoClass::element_class。
不是结构体返回值/指针层数猜测。通过真实 mono_class_get_element_class 对照 native 首字段，
递归 canonicalize class identities，最多64节点，只发布 offset0 的 readonly 8B guest token。
userdata offset232 继续是独立 owned writable scalar；其余 class header 不映射，host pointer
不泄露。自环/element chains 保持稳定身份，native immutable identity 改变则拒绝，退出撤销。

元数据谓词按真实 Class/Type 输入和 gboolean/MonoBoolean 返回宽度桥接。isinst 调用真实
Mono，只接受原对象或 NULL；不准入新的 remoting/proxy identity。

external weak v2 handle 使用真实 mono_gchandle_new_weakref_v2，保留 frozen BDWGC bit0 tag。
weak handle 不发布可读对象快照，get_target 查询实际 native weak link，消失则返回 NULL，
变化成另一对象则拒绝。strong cell、free、retirement 和 double-free 规则保留。
有限诊断另有长期 strong object roots，因此实际样本验证 weak handle 创建/查询/释放，
不宣称证明对象失去最后 strong reference 后的真实回收；NULL transition 有独立回归。

## 验证记录

probe-37 越过原来的 unmapped class prefix，实际368次静态调用、372 box payloads、
2117 field GC checks、375 RuntimeTypes、872 strings，全部 roots/views 生命周期通过。
probe-39 又越过类型谓词与 weak handle 创建，component ctor44、14个实际 weak handles，
停在 mono_object_isinst。冻结 runtime/support SHA 未改变；guard237/0，原始 Mono 未 attach。
这两次均为有限 assets startup probe，耗时不作为真实游戏加速对照。

最新probe-40 exit0：368 static invokes/1104 out writes、68次后续ctors、2193 fields全部GC checks，
425 allocations/1429 borrowed roots、872 strings/375 RuntimeTypes全部GC/retirement通过；14 weak
handles，372 box payloads。游戏正常GC disable1/enable1、unwind0/finaldepth0；guard237/0。
472 release tests passed/7 ignored、all-target Clippy clean。下一请求VolumeComponent.OnEnable
instance void零参数；正式窗口未验收。日志为wire-out/native-mono-static-calls-tests.log和clippy.log。

## 普通实例与同一 owner 重入（2026-10-12）

probe-41 exit0：21次真实 ordinary instance IL invokes（包括 VolumeComponent.OnEnable），
与368次 static invokes共用 typed Frame、checked outputs 和完整 graph refresh。
receiver 核对 actual closed class、owned image 与真实 CLR assignability；value receiver 仍拒绝。
返回 reference 以 canonical String 或既有 strong-root identity 传播，不造返回值。
2193 fields/2193 GC checks、425 allocations/1429 borrowed roots、872 Strings/375 RuntimeTypes，
全部相关生命周期通过；GC disable1/enable1、unwind0/finaldepth0，guard237/0。正式窗口未验收。

普通 IL 的 native callback scopes共享主流程 instances、fields、Strings、typed arrays、
attributes、RuntimeTypes、empty arrays、external handles。`PreparedInvoke`分 prepare/execute/finish，
注册表和 Memory 的 RefCell borrow不得跨过可重入 IL/engine调用。nested constructor仍复用
metadata-qualified instance void()契约，没有 InputSettings 或 analytics 类名/ctor token名单。
一般 nested ordinary invoke目前只接受static零参数void/reference；一般instance/带参数/scalar
nested return、完整异常传播、任意native bindings与全Process GC roots仍未准入。

leaf重入复用实际image identity/owned CString arena、typed field API、String/RuntimeType查询、
checked object allocation、arrays、reference barriers和真实strong/weak v2 handles。
thread_current只认可guest主线程、既有native强根和实际current thread identity；不开放Thread头部。
reference field setter传对象本身，scalar/inline传受检guest输入的owned copy。

## 固定 engine adapters

新增analytics span、Resources Type→Object[]和ScriptableObject Type/Boolean→IntPtr工厂：
核对actual metadata与冻结注册目标后，用提前编译的Rust C ABI入口执行原始guest引擎。
Resources保留CLR array covariance和实际array identity。
工厂IntPtr是v2 GC handle：guest/native handle槽转换只发生在private native adapter中，
宿主handle不写guest。实际Unmarshal IL读取GCHandle.Target；不能把guest handle地址传给native IL。

27个Application、NativeInputSystem、SystemInfo、Screen和Jobs状态 scalar bindings整体完成signature/target验证后注册。
primitive/enum actual class、宽度/alignment、static InternalCall、非泛型、无byref核对。
Boolean canonical，float参数按Win64位置进入XMM，RAX/XMM0返回分别保留原始bits。
类型转发按同一实际MonoClass identity去重，真正不同owner仍拒绝。无JIT/动态trampoline。

JSON FromJson adapter复制owned UTF16 span，转换existing object与RuntimeType，执行原始Unity
JSON序列化器；不重写JSON语义。结构体分配和unbox复用boxed payload；嵌套value布局按真实字段
展开Boolean/reference/padding叶槽，见value payload文档。普通aggregate field C ABI尚未准入。

probe-57/59已实际越过analytics、Resources和InputSettings工厂/构造，随后完成focus、device
callback switch、polling frequency get/set和Double startup-offset调用，进入JSON。
probe-60/61分别阻塞在nested value allocation及含引用inline struct，均abort；不能把局部VERIFIED
行当作完整GC、noexec guard或cleanup验收。最新成功完整检查点仍为probe-41。

## Profiler sample 与 private native IL buffers

probe-62/63的原始Unity JSON反序列化实际返回rooted对象（113 UTF16 units）。随后Mono解释器
准备IL时发现未注册Malloc/Free，error cleanup断言abort；不将局部输出计为整轮生命周期成功。
probe-64新增真实1886B、alignment8、Persistent(4)分配，随后发现缺失MemSet并abort。

BeginSample/EndSample固定adapter验证actual static Void(IntPtr)和冻结注册目标，IntPtr必须是
真实CreateMarker的已登记结果，包括实际disabled NULL；转发原始guest engine函数，并核对
同一owner的嵌套配对。没有把NULL sample伪造为成功；当前这些新sample尚待实际执行证据。

UnsafeUtility四参数Malloc/Free和MemSet验证actual metadata，包括Void*的Void element、
Int64 size、Int32 alignment、实际Int32底层Allocator enum、64bit ABI中的IntPtr和Byte宽度。
原始Unity allocator管理guest分配记录，native IL使用私有owned aligned storage；两个地址
只在typed adapters内转换，不映射宿主地址，也不开放raw pointer字段/其他engine icall。
MemSet限定live native span，执行原始guest engine写入后核对全部填充值，再写对应native子区间。
这不是SharedBuffers生产接入，详见native-mono-unsafe-buffers-2026-10-12.md。

已完成probe-63源码完整feature release：480 passed/7 ignored，Clippy clean；probe-64对应
Mono定向80 tests passed，Clippy clean。后续MemSet改动仍待新probe和最终完整回归记录。

probe-65实际完成1886B与1986B两次Persistent分配和两次MemSet零填充，以及第一次正常Free；
随后缺失MemCpy导致abort。该源码完整feature release482 passed/7 ignored，Clippy clean。
新MemCpy/MemMove adapter限制两个span都来自private live allocations，保留overlap行为；
只在native data内实际copy，不把可能含native pointer的bytes发布给guest。待probe-66复测。


probe-67实证4次Persistent分配、4次MemSet、33次copy（含真实零长度分支）、3次普通Free，
随后缺失Screen.width/height并abort。原始CRT零长度jump table核对为0x181b8a14e直接RET。
probe-68实际转发width=314/height=211，随后缺失FindObjectsByType；这是实际engine查询值，
不代表正式游戏窗口尺寸验收。probe-69被nested empty array的旧六image layout限制拒绝。

probe-70实际通过Object.FindObjectsByType(Type,FindObjectsInactive,FindObjectsSortMode)。
两enum均核对实际Int32 storage；真实Type/array identity、CLR covariance和同步规则共用Resources。
zero-length SZ数组不需要元素payload layout，但仍要求actual concrete class和live image owner、
真实Mono allocation/strong root、immutable bounds/length views和同一canonical object身份。
非空typed arrays保持既有layout admission，没有按游戏类名添加白名单。

后续缺失JobsUtility.set_JobCompilerEnabled。其原始target0x18005ff00只是对0x182070735写入
canonical byte，getter0x18005fef0读同一byte；不调用编译器。新增Jobs四个state queries/setters
仍走原始engine，不能因此宣称支持Burst codegen、job scheduler或跨线程native buffers。
probe-68/70的错误退出还观察到foreign finalizer Free拒绝：生产finalizer owner调度尚未实现，
不能吞该错误或将局部buffer成功记作完整GC/cleanup。最新完整成功仍probe-41。


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
