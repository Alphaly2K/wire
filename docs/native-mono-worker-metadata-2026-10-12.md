# 原生 worker 元数据与诊断停止清理

继续在 main 开发；冻结 Mono/runtime support、原游戏程序集不变。

`mono_class_get_fields` 首次枚举会进入 `mono_class_setup_fields`，使用实际 OS TLS 的
递归初始化列表。因此把枚举以及已有 typed query 中涉及 lazy setup 的查询派给对应
guest worker 的持久原生 OS thread。每条命令核对实际 root domain 与 strong Thread
identity；不把它们扩大成可在主线程代执行的 thread-neutral getters。

主线程解析 canonical class/method/field/type/signature 和 iterator tokens。私有
WorkerRequest 只携带这些由保留 image/domain pools 管理、GC 不移动的 metadata
capabilities；不得携带 guest payload 地址或 managed object。WorkerBorrow 独占借用
registry 到同步执行完成，所有 workers 先 detach/join，随后才释放 images/domain。
结果为 owned CString、scalar 或私有 metadata capability，主线程转换 canonical token
并更新受检 guest iterator slot。迭代器不能跨 parent/function 串用。请求失败不更新
迭代器；即使 profiler reply 失败，也等待命令完成再释放 registry 借用。

自定义属性的 from_class/from_field/has/free 使用同样的借用约束和实际 worker TLS。
只发送 owned native CustomAttrInfo 与 typed class/field capabilities，构造函数和
managed attribute object 不在这条新通道的范围内。沿用冻结 runtime 的 checked query
wrapper；实际 NULL、metadata error 和 info ownership 均保留。

`probe-79` 已越过字段枚举、字段类型/偏移/声明类查询，止于字段属性查询。
`probe-80` 完成整批 worker 扫描，14200 次 worker 命令，继续到主线程 IntPtr[1]
分配。该轮暴露停止清理缺陷：配置边界的 stopped predicate 被误用作 cleanup-complete。
退出时因缺少 owner scope 而 abort，因此该轮不作为成功证据。

修复后 `probe-81` returncode=0：同样越过 worker 扫描；实际 max-generation GC 后
worker strong identity 验证1次，attach1/detach1、live0、closed=true；对象/fields/数组
GC snapshots 和 root/view teardown 完成，legacy shutdown callback 恰好1次。
noexec guard 241 checks/0 denied。仍是配置边界成功，窗口不可见、draw0/Present0，
不等于游戏已运行或完整 backend 已接入。

下一边界为 System.IntPtr[1]，caller 0x180798c23。冻结 CoreModule 包含
Display.RecreateDisplayList(IntPtr[])，其 IL 只把原引擎显示器句柄存入
Display.nativeDisplay，随后交给原始 engine InternalCalls。正在加入仅 exact
System.IntPtr SZ array 的句柄布局：每个 guest 输入值须 NULL 或 live private guest
data handle；拒绝代码、metadata、宿主地址；native snapshot 只接受已受检输入集合中的
句柄。此规则不开放任意 pointer field、普通 IntPtr invoke 参数或宿主指针 dereference。

88项 Mono 定向 release 回归通过（含新增 iterator ownership、IntPtr validation、
配置停止与实际 shutdown 状态区分）；继续真实游戏复测后补充最终结果。

`probe-82` 验证实际 IntPtr[1] 分配、强根和数组同步，止于
Display.RecreateDisplayList 参数类型闭合检查。CLI native int/uint 是具体原始类型，
已将0x18/0x19加入类型闭合证明；0x0f pointer、未决generic/byref仍拒绝。这不是扩展
普通 managed invoke 的 IntPtr 参数准入。冻结 engine 的 Display InternalCalls 将
nativeDisplay 解释为显示器编号（ECX），该样本第一个显示器编号为0，数组实际保存0。

`probe-83` returncode=0，实际执行 RecreateDisplayList 与后续启动代码，guest window
已 visible=true、1024×720；draw0/Present0，尚无正式渲染。下一处为
SceneManager.Internal_ActiveSceneChanged(Scene,Scene)，原始方法token0x06002d7d。
正在按metadata值布局准入1/2/4/8B、完整纯标量覆盖的小结构体，复用原生栈word参数；
不按Scene类名分派，不复制padding/Boolean/reference/pointer字段。

完整feature release回归492 passed/7 ignored；随后类型闭合与小结构体修正的88项
Mono定向回归通过。feature all-target Clippy -D warnings通过（小结构体新增部分另测）。

`probe-84` 已执行上述小结构体 Scene 回调及更多原始 IL，随后因为未注册
ProfilerRecorder.Create_Injected 而 abort。现加入13个固定编译的 recorder 适配器，
逐一核对真实注册目标、metadata签名、8B句柄与24Bsample的纯标量布局；byref值复制到
private guest slots，再调用原引擎，不向Win64发布宿主地址。句柄来自实际引擎返回、
live ownership登记；Control(Release)及退出清理都调用原engine。尚未接CopyTo_List/Pointer。
Unit/DataType两个enum明确采用真实Byte存储；其他现有固定适配器仍保持Int32合同。

新增多位置byref callback回归核对RCX/RDX/R8/R9、输出与撤销；89项Mono定向测试通过。
`probe-85` 首次显式启用宿主window和Present capture，安装预检发现enum校验只接受Int32，
已修正后继续复测。这轮未发生Present，也没有游戏画面。
此前probe-83的visible=true仅为guest状态，不能当作实际宿主画面证据。

`probe-86` 实际XR Recorder构造使用SharedRecorder标志；冻结IL为CPU 0x99、GPU 0xd9。
已按实际engine引用计数接共享创建，每次成功创建对应一次Release，不将共享返回误判为
重复所有权。描述接口核对24B显式field layout，复制原engine的category/flags/type/unit；
UTF8名称完整检查非代码数据、长度/终止符，保存为VM生命周期内的private native bytes。
padding归零，guest名称地址不直接进入IL解引用。

Recorder释放复用既有Mono profiler认证的finalizer范围：foreign finalizer只排队，owner入口/
出口及VM cleanup之后调用原engine Control(Release)。强根保留期间不提前释放；所有names
与残留engine引用直到VM finalizers结束后退休。新增共享引用/finalizer线程/重复释放回归。
`probe-87` 实际创建6个记录器，24次接口调用、6次finalizer排队/6次owner drain、live0、
closed=true；退出码1来自后续缺GraphicsSettings icall，不计整轮成功。

`probe-88` 新4个GraphicsSettings对象getter用真实v2 GC handles接Unmarshal；
currentRenderPipeline实际非NULL，返回canonical object而非stub。首次开启的宿主窗口达到
1024×720，5次GPU draw/1次Present，frame.ppm及frame.png可视确认完整Unity Logo。
这仍不是游戏可玩验收：后续缺QualitySettings.activeColorSpace，以及worker26申请托管对象。
正在接对应真实TLS分配、跨队列强根所有权与失败回复释放，不在owner线程代执行worker分配。

`probe-89`完成worker26的实际对象分配、强根移交及C++受检引用槽保存；`probe-90`
完成同一worker的外部v2 handle，继续到零参数构造入口。按metadata准入的reference
instance constructor/serialization-interface callback通过独立pinned receiver root派给
实际attached OS worker，既不发布宿主对象指针，也不使用类名/构造函数名单。
constructor images在assets模式使用真实保留的全部live images，core仍为首项。

`probe-91`超时，宿主sample定位到owner等待worker completion、worker26等待SharedStatic
execution lease。原先owner callback scope在同步等待期间仍持有全部shared leases。
修正为TLS中的逐层执行scope保留backing与lease；同步发送命令之前释放所有嵌套层的lease，
实际worker完成后恢复。worker等待owner profiler callback也遵循同一规则；等待的native
stack没有继续访存，guest Memory/Runtime仍只在owner使用。新增真实OS线程回归覆盖两层
owner scope、worker写入、反向callback写入、返回恢复及revoked region拒绝。

`probe-92`越过这处死锁，实际执行UniversalAdditionalCameraData、Light2D、WindowResizer
等构造函数。1024×720宿主窗口9draw/3Present；停止原因是worker26尚未接
mono_object_get_virtual_method，退出码1，仍未到可玩界面。退出时两个worker实际detach/join，
max-generation GC strong identity验证2次；recorder/unsafe buffers/SharedStatic均live0及
closed=true，guard300/denied0。完整默认release回归380 passed/7 ignored。

虚方法查询新增私有pinned receiver WorkerRoot与typed Request/Result：实际worker验证对象
class、声明assignability、instance/reference合同后调用原Mono resolver，仅返回nonmoving
method metadata capability；owner转canonical method token。该通道不传vtable/code pointer。

`probe-93`已完成真实worker virtual resolution与serialization-interface IL，下一处为
mono_class_get_image。冻结class.c/image.c核对这三项只读getter：class_get_image直接返回
class的image；image_get_name读取assembly_name；image_get_table_rows做table index边界检查
后读取rows。不调用lazy setup/current domain或managed IL。worker实际attachment检查后，
owner串行读取并复用既有live image registry/owned text arena，禁止发布native image/char*。

截至虚方法查询接入，完整feature release回归496 passed/7 ignored；all-target Clippy
-D warnings通过。image getter接入后构建及同规格Clippy通过，继续probe-94。

`probe-94`完成image getter，继续到worker mono_get_corlib。冻结domain.c核对corlib与全部
21个已有core class getters只读取mono_defaults；同样采用已attached worker的owner串行
只读通道，corlib返回须与CORELIB fingerprint registry中保留的实际image一致。
image阶段完整feature release回归仍496 passed/7 ignored。

`f77d6c1`已直接提交main：worker metadata/attributes/objects/invoke/virtual、显示器IntPtr[]、
小结构体、ProfilerRecorder、GraphicsSettings和锁交接修复，包含实际代码3123新增行。
本地提交，无push。

`probe-95`40draw/13Present，worker在List._items的exact declared reference field写入前停止。
把既有fields/reference-arrays barrier路由接到worker：先验证actual root attachment，仅exact
登记reference cell可转换canonical object token并调用原native field/barrier API。NULL owner
沿用既有实现，从受检cell恢复真实owner，非托管C++临时槽仍单独保持其范围与身份检查。

`probe-96`随后在runtime_invoke被params非NULL检查拒绝。`probe-97`真实签名证实为
UniversalAdditionalCameraData的零参数OnAfterDeserialize；Unity传的是unused scratch
vector。以metadata参数数目证明零参数，不解引用或转交这块scratch，72draw/24Present，
继续到worker数组分配。上述改动92项Mono定向回归通过。

worker数组Request只带retained domain/element capabilities与受限length，在actual worker分配
并立即建立pinned v2 root；Reply持有独立root直到owner采用。布局沿用metadata-derived scalar/
reference slots，owner验证真实class/length/offset32与zero initialization再publish；0长度只
映射immutable header。新OS线程回归覆盖0/1长度、实际worker执行、root移交、私有header、
容量与退休拒绝。大Request用Box使高频scalar worker命令保持小尺寸。

`probe-98`实际分配Int32[1]与Vector3[4]，执行Light2D.OnAfterDeserialize；后续main线程
ScriptableObject factory的nested invoke因旧constructor-only限制拒绝。现复用ordinary IL
签名/closed declaring type/receiver assignability校验，准入zero-argument void/reference
instance方法；构造及serialization接口仍保留其已有metadata验证。没有按OnEnable等名称
选择执行逻辑。继续probe-99获取实际方法和后续边界。

`probe-99`确认nested instance为AdaptivePerformanceManagerSpawner.OnEnable，真实IL开始执行，
止于未注册GameObject.Find_Injected。`probe-100`固定Find/FindWithTag适配器完成实际engine
查找（该样本Find返回NULL），后续Internal_CreateGameObject_Injected缺失。此时完整feature
release回归497 passed/7 ignored，Mono定向93项通过。

GameObject创建/添加组件/DontDestroyOnLoad也采用固定编译的typed adapters，每项核对冻结
InternalCall target与真实metadata：UTF16名字复制到private guest span；对象/Type用canonical
roots；engine IntPtr只能来自live owned noncode guest data。对象返回使用真实native v2
GC handle交给IL Unmarshal，禁止将C++指针当MonoObject。

原生IL直接new GameObject时原有borrowed root尚没有cached-pointer view。新
publish_engine_object先核对UnityEngine.Object assignability，只映射实际m_CachedPtr的8B槽，
保留既有pending guest写入；native初值若非NULL必须是aligned owned noncode guest data。
回归覆盖header不可见、精确cell写入同步、重复publish、code pointer拒绝与root/view退休。

`probe-101`实际引擎成功创建GameObject，rooted-self=0x6fff07c20000；AddComponent进入nested
mono_custom_attrs_get_attr，旧nested route缺此项。复用既有checked native attribute构造及强根
API，不改变其构造准入；继续probe-102。

`probe-102`实际完成AddComponent，执行AdaptivePerformanceManager构造及Awake，
62draw/21Present；停止于尚未注册Object.DestroyImmediate_Injected。不是Mono版本失配：
其后提示out-of-sync是未注册InternalCall触发的通用诊断。完整feature release回归
498 passed/7 ignored；94项Mono定向回归和all-target Clippy通过。

对象销毁新增固定DestroyImmediate/Destroy适配器，分别验证真实Boolean与Single签名，
冻结目标0x1800c2fd0/0x1800c2eb0；延时参数保留原float32位型至Win64 XMM1。NULL沿用
原引擎语义，非NULL仍须live owned noncode engine data；原引擎完成后同步cached-pointer
views，不跳过Awake或伪造删除。正在继续真实游戏复测。

`probe-103`原引擎完成销毁并回调实际OnDestroy，越过AdaptivePerformance初始化，
下一处为NativeInputSystem.Update。新scalar adapter核对InputModule真实Int32 enum与
0x180132a10，按值调用原引擎。`probe-104`进入ShouldRunUpdate(enum,out bool)后，因nested
invoke仅允许零参数被拒绝。

nested ordinary invoke复用已有managed_calls.Frame：从guest读取bounded参数vector，
scalar复制至native栈，reference通过canonical roots与assignability转译，byref仅限当前
callback的live owned stack。managed IL执行期间释放Memory/route借用；完成后重新检查所有
输出与exception槽，canonicalize引用、刷新图，再事务性写回，不发布native stack指针。
仍只允许void/reference返回，IntPtr与raw pointer不进入普通参数。新增回归证明后段输出/
exception失效时先前输出不变，以及1B bool精确写回；95项Mono定向release测试通过。
