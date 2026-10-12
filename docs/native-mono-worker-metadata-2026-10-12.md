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
