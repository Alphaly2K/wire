# 原生 Mono 通用桥接：可行性验证

## 为什么暂停逐接口适配

此前固定 adapter/Spec 的程序集 SHA、MethodDef token、UnityPlayer 地址只验证了
Alice 的冻结样本。相同 Unity 版本的不同游戏也可能采用不同模块、构建配置和原始
注册目标，需要从实际元数据和注册表取值。当前已能显示窗口/Logo、执行配置构造，
完整原生 Mono 后端仍需可交互主循环和第二游戏验收，`full_game_backend=false`。

用户要求暂停面向单游戏结果继续堆补丁，先验证是否有可推广的调用机制；若核心
机制仍离不开逐方法 Spec 或运行时机器码，则应放弃当前原生化方案。

实现应收敛为少数运行时入口：托管 IL 调用、InternalCall 注册/分派、对象和 GC
身份转换、原生内存/回调生命周期。方法名和签名属于运行时数据；按方法手写 ABI
与复制构造函数不是目标架构。跨架构边界需要处理 ARM64/Win64 参数分配及两侧
地址空间，相关逻辑应复用统一协议。

## 本轮机制

`scalar_engine.rs` 不再含逐方法 Spec、程序集指纹、MethodDef token 或目标 RVA。
它在受持有的 loaded image family 中读取真实类/方法/完整签名、enum storage、
值类型布局和 ParamList 的 in/out 属性，目标地址来自 sealed 的实际引擎注册表。
不匹配、未注册、同名 icall 重载歧义和未支持的形状保留缺口。

ARM64 signed executable 预编译一个 8,192 槽的入口池（初版 4,096）。每个槽只有固定的 slot index
和到共同入口的分支；共同入口保存 x0–x7/d0–d7，按数据描述符转换 ARM64 的独立
整数/浮点寄存器分配与 Win64 的参数位置/stack ABI。返回通过 RAX/XMM0 位模式再
转换至 x0/d0。入口池不会在运行时生成、修补或新增可执行映射；描述符只是私有数据。

支持范围为最多 16 参数的静态 primitive/enum 调用，以及已准入类族的 plain-value
`ref/out`。后者要求 actual metadata 中每个字节都被非 Boolean 标量覆盖，无 reference、
pointer、padding 或重叠字段。native payload 复制到作用域内的 guest buffer，输出
全部预检后写回；相同地址/布局的 alias 保持同一 guest 地址，不同布局或部分重叠拒绝。
`out` 不读取原 native 存储。native pointer 不交给 Win64。

UnityEngine.Object 的子类型由实际 TypeDef 和继承关系发现。其静态 binding 的第一个
参数若为 corlib 的 exact IntPtr，且真实 Param 名为 `_unity_self`，可准入 engine-self
协议；执行前核对 live original guest engine allocation。于是 Transform、Camera 等
普通状态/值类型接口共用同一机制，无各类方法列表。对象返回句柄按下述包装协议推导，
其他 IntPtr 参数/返回保持待处理。

类族列表只是语义准入策略，没有每个方法的实现或签名值。IntPtr、裸指针、引用、
allocator/GCHandle 类 Int64、callback 和异步协议没有由纯 ABI 推断为安全可用。
普通 managed IL 构造/方法继续走已有元数据路由，而非逐类函数白名单。

## 判定标准与范围

1. 同一预编译入口池要用不同数据描述符处理变化后的函数名、参数组合和 guest 目标，
   包括混合整数/浮点、NaN/negative-zero、6 个浮点参数后的 Win64 stack 输出指针。
2. `ref/out` alias、重叠拒绝和失败时不写回必须通过实际 guest 指令验证。
3. 真实游戏要仅凭运行时 metadata/registration 自动安装对应接口，并执行原引擎的
   Quaternion 入口；noexec guard 不允许新增未预编译机器码。
4. 此前已安装的语义 adapter 不被覆盖，全量盘点区分 lookup resolution 与实际执行。

前两项验收 ABI 机制。不同 profile 回归使用自编 fixture；真实第二游戏的兼容性验收
仍待实施。引用/缓冲区/线程/GC 的剩余协议需要各自的端到端验收。
整个启动 provider 仍锁定 Alice 与 25-patch native runtime；解除这些限制、验证其他
游戏/Unity 版本以及可交互主循环，是独立的未完成门槛。

## 验证结果

三个 ABI 回归通过，完整 release feature 回归 **535 passed / 7 ignored**，
all-target release Clippy clean。fixture 实际调用预编译 ARM64 pool，并执行自编
Win64 指令核对 mixed 参数、变化后的 target、f32/f64 返回位、ref/out 及 alias。
错误 Boolean 结果、输出长度、部分重叠和未签发 engine-self 地址保持原输出。

probe-159 无逐方法 Spec 自动安装 **394** 个入口。真实执行
`Quaternion.Internal_FromEulerRad_Injected` 成功，接着遇到
`Transform.set_rotation_Injected` 缺失。noexec guard checks=333 / denied=0，
耗时 29.12 秒，263 draws / 99 presents。真实第二游戏尚未验证。

probe-160 完成 metadata-derived engine-self 复测，自动注册 **3,391** 个入口；
`Quaternion` 转换、`Transform.set_rotation/get_rotation` 与协程 Boolean 输出均实际
成功。全量 lookup missing **10,314 → 6,923**，resolved **1,135 → 4,526**。
本轮耗时 29.53 秒，265 draws / 100 presents，guard checks=333 / denied=0。
完整 release feature 回归 **535 passed / 7 ignored**，all-target Clippy clean。

新有限终点为 `PhysicsEvents.InvokePreSimulate(PhysicsScene2D, Single)`：PhysicsScene2D
完整 plain-value 布局为 8B size / 4B alignment，旧普通 IL 参数路由误要求 alignment
等于 size。后续按 actual metadata 修复这个通用值类型对齐规则，不添加该方法特例。

本轮判定：预编译统一调用机制通过当前验收，可继续按 ABI/语义协议推进。下一阶段
验收主循环与实际第二游戏。既有早期 adapter 仍采用冻结样本规格，整个 provider 的
跨游戏启用还要移除这些前置限制；新桥接层已经从实际注册表解析目标。

## 统一内存协议与值类型对齐

普通 IL 的 plain-value 参数现在分别使用实际 size 和 alignment；8B/4B 的
PhysicsScene2D 不再按 size 要求 8B 对齐。该改动完整回归为 **536 passed /
7 ignored**，build 和 all-target Clippy 通过。probe-161 因主循环调度先遇到
`UnsafeUtility.MemCmp` 缺失，29.92 秒后停止；216 draws / 83 presents，guard
checks=334 / denied=0。此轮尚未实际执行到 PhysicsEvents 的对齐边界。

probe-162 暴露了原型的过度约束：MemCmp 的参数也可来自原生 IL 局部值或字段，
不属于 UnsafeUtility 的桥接分配池。离线 IL 盘点发现 57 处调用，包含 InputSystem
的 PrimitiveValue.Equals 和 FixedList 的值比较；这些由 ldarga/ldflda/AddressOf
取得实际原生地址。原生 Mono 的 ldind/cpblk 同样直接使用本地址空间的指针。

因此 UnsafeUtility 的填充、复制、比较、复制重复元素与步长复制按纯原生 unsafe
指针契约执行，调用者保证 span 有效；字节始终在原生地址空间内，未映射或发布到
guest。保留空指针/长度/地址运算校验；零长度不访问指针。重复/步长复制在写入前
验证整数溢出、工作量与交叠包络。跨架构导出仍要求 Arena::range 的完整 owned span
与 live allocation；原生局部值不会因此获得跨架构指针权限。没有按调用者类型新增
方法特例。该设计也删除了纯 MemSet 的 guest 执行及重复缓冲区复制。
这组 allocator/memory target 从实际 sealed registration 取值，去掉冻结地址。
SetupCoroutine 的 Boolean 输出协议也按实际 declaring class 和完整签名识别，
移除了固定 MethodDef token。

修正后的 probe-163 实际完成 6 次 8B MemCmp，继续通过
PhysicsEvents.InvokePreSimulate / InvokePostSimulate 和 NotifyBeforeUpdate；size 与
alignment 分离的修复在实际引擎调用中通过。29.66 秒，259 draws / 97 presents，
guard checks=335 / denied=0，没有 icall 缺失或 adapter reject。有限终点为
NativeInputSystem.NotifyUpdate 的 IntPtr 输入事件包，下一步复用输入事件快照协议。
游戏尚未到可交互界面，最后 Present hash 保持先前加载画面。

本批完整 release feature 回归 **539 passed / 7 ignored**，release build、all-target
Clippy（`-D warnings`）和 8 项 inventory/harness 回归通过。新增原生栈/值内存测试
覆盖无符号字节比较、填充子范围、复制 overlap、重复/步长复制的 gap，以及零长度、
NULL/整数溢出的处理；跨架构 owned export 的退休检查继续通过。

## 输入、数组与对象句柄共用运行时入口

NotifyUpdate 的现有输入快照协议现同时接入顶层与嵌套 mono_runtime_invoke，复用
普通 invoke、异常处理、GC 重入与图同步；不再另写一套执行路径。私有参数指针在
最终 Frame 位置生成，20B header 后有 canary；输出先验证 lifetime、span 与异常
槽不交叠，再写回 guest。IOCTL 目标来自本次实际注册。

SZARRAY 参数的实际元素类型若继承 UnityEngine.Object，则复用 canonical container
投影、真实 worker root 和 Mono array reference barrier。相同数组使用同一身份，
不增加 Camera 等具体方法 adapter。probe-165 实际通过 GetAllCamerasImpl_Injected。

IntPtr 返回仅在当前 managed wrapper 的完整指令边界尾部为
`call <当前 icall>; call UnmarshalUnityObject<T>(IntPtr); ret` 时识别为对象句柄。
MethodDef/MethodSpec 与闭合返回类型由本次实际 metadata 解析，converter 要属于
实际 engine-base image 中的 UnityEngine.Bindings.Unmarshal，T 要继承 UnityEngine.Object；多 wrapper
推导冲突则保留缺口。结果复用原有 live guest handle → native v2 handle 协议，
零句柄直接返回零。解析不执行 wrapper，也不含冻结 token 或目标地址。

probe-166 自动注册 **3,501** 个入口，实际完成两次输入更新（2 个事件 / 100B，
第二次全部消费），Camera 数组、targetTexture 与 targetDisplay 查询通过。33.62 秒，
262 draws / 99 presents，guard checks=333 / denied=0；本次与测试并发运行，未作性能
对照。随后 DoSendMouseEvents 在 Display.RelativeMouseAtImpl 缺失处抛出异常，
最后画面仍为加载页，尚未达到可交互游戏。下一步按现有 numeric/value 协议成批
审查常用引擎类族，而非增加单方法 adapter。

本批完整 release feature 回归 **541 passed / 7 ignored**，release build、all-target
Clippy（`-D warnings`）与 8 项 inventory/harness 回归通过。新回归覆盖移动后的输入
header 指针、canary 与失败输出不写回，以及变化后的 MethodDef/MethodSpec、截断 IL
与 operand 内伪 CALL 的拒绝。整个 startup provider 的冻结前置规格与第二游戏验收
仍列入待办。

## 常用数值类族与对象指针参数

Display、HDROutputSettings、Physics/Physics2D、Random、HumanTrait、DynamicGI、Debug、
GUI/GUIUtility 与 CameraRaycastHelper 共用现有描述符，不增加逐方法签名或实现。
probe-167 自动注册 3,734 个入口，实际通过 Display.RelativeMouseAtImpl、pixelRect、
eventMask、ScreenPointToRay、裁剪平面与 cullingMask。30.37 秒，242 draws / 92 presents，
guard checks=335 / denied=0；下一缺口为 Raycast 的 Camera IntPtr 参数。

该参数现由实际包装 IL 的有序参数加载与闭合
Object.MarshalledUnityObject.Marshal<T>/MarshalNotNull<T> 推导。调用源、MethodSpec、
参数类型、引擎继承、native IntPtr 返回、静态 IL 属性与实际嵌套声明者均核对。
此协议接受简单直线包装；有分支、参数重排、多个目标调用、其他 pointer converter
或多个包装推导冲突时保留缺口。Marshal 的零值可转发，MarshalNotNull 的零值拒绝；
非零值仍走 live original engine data pointer 检查，宿主地址不会传给 Win64。
对象返回转换允许包装方法引用 CoreModule 的真实 helper，去掉错误的同包装 image
限制；仍以实际 engine-base 的 owned image 为 helper 所有者。

probe-168 自动注册 **3,852** 个入口，实际执行 RaycastTry / RaycastTry2D，越过
DoSendMouseEvents，继续创建 Naninovel.ProjectResources 与 InputActionAsset 并执行
Transform.set_position。33.09 秒后在资源字段投影的 generic value 分类处中止：
现有代码把所有 GENERICINST value 提前拒绝，下一步复用既有引用叶子布局与写屏障。
本轮未完成正常收尾，guard/图形最终计数没有采集；最后画面仍为加载页。

本批完整 release feature 回归 **542 passed / 7 ignored**，release build、all-target
Clippy（`-D warnings`）通过。新回归覆盖动态 token、有序加载、未验证 converter、
分支/重复调用/截断拒绝，以及 nullable NULL 转发、required NULL 和伪造宿主指针
失败时保持 native 输出。纯 scalar ABI 继续拒绝 capability-like Int64/IntPtr。

## 闭合泛型值与对象图边界

GENERICINST value 现与普通 value 共用实际闭合 `value_layout`，含引用、Boolean、
padding 的布局走 nested leaf 与完整 owner 的 Mono barrier；只有完整非 Boolean
标量覆盖才允许 inline。whole-field store、refresh、失败批次与 retire 使用已有
验证协议，未引入 InputSystem 类型名或游戏 token 特例。

probe-169 实际越过泛型值字段，完成多个 InputActionMap/InputAction 构造与
OnAfterDeserialize，以及 InputSystem 内部 UnsafeUtility 状态内存分配/复制/释放，
继续创建 EventSystem。39.06 秒后在递归图的 `UnmanagedMemory.basePtr` 分类中止，
returncode=-6；最终帧仍为加载页，未正常收尾的 guard/GPU 统计没有采集。

全量字段盘点确认该对象的实际声明者 InputActionState 没有 Serializable 标志。
普通对象图不再因为 helper 有 public 字段就展开其 runtime storage；非序列化
helper 和委托保持 canonical root，不发布内部裸指针。实际可序列化 reference
及显式 generic 容器继续投影。此规则共用在顶层/嵌套图中；explicit field ABI 仍
要求 checked typed view，unsupported 指针仍是拒绝边界。

本批 **543 passed / 7 ignored**，release build、all-target Clippy 与 11 项
inventory/harness 回归通过。完整协议盘点与批量工作方式见
`native-mono-bridge-gap-inventory-2026-10-13.md`。整个 provider 的早期冻结规格、
可交互游戏和第二游戏验收仍未完成。

probe-170 验收通过上述对象图边界，实际完成 InputSystem 状态分配/复制/释放与
EventSystem 构造。36.27 秒后正常停在 `.UpdateFunction::Invoke` 的 Runtime 方法
准入边界：401,301,993 retired instructions，245 draws / 93 presents，guard
checks=336 / denied=0，GC scope depth=0，全部 32 个 UnsafeUtility 分配已释放。
最后帧仍为加载页。日志还记录 Naninovel 的异步初始化在
`Object.Internal_InstantiateAsyncWithParams_Injected` 缺失处抛出并记录异常；后续委托
准入通过也需修复异步实例化，才能完成初始化。

委托 `Invoke` 现按实际 `mono_class_is_delegate`、Runtime implementation flags、
closed declaring/parameter types 准入，复用既有 mono_runtime_invoke、canonical roots、
参数/输出/异常协议。普通 runtime/native/PInvoke/synchronized 方法仍不准入；
构造函数继续走原分配路由。generic delegate 的 inflated Invoke 要求闭合实例和
非 generic method，不按 PlayerLoop.UpdateFunction 类型名写特例。

原生 fixture 使用冻结的 24-patch runtime 与既有 corelib support 验证静态目标、
捕获闭包、多播顺序/状态、引用参数/返回、GC 后调用与异常返回。
`wire-out/native-mono-delegate-invoke/renamed-run.log` 为 PASS，guard checks=32 /
denied=0。fixture 采用独立程序集名，避免与旧 NativeProbe 的 AOT support 身份冲突；
没有重建 runtime 或 corelib support。

显式 mono_field_get_value/set_value 现按实际 field identity 单独建立视图，静态、
literal、shadowed、裸指针字段继续拒绝；同对象其他 runtime storage 不会随之展开。
这使 native-only reference 能保留 opaque 身份，并在确实出现 embedding field
访问时复用统一 typed field 协议。

异步下一批统一审查 AsyncOperation、AsyncInstantiateOperation 和 Awaitable：真实
operation 数据指针、InstantiateParameters 的引用叶子、Vector3/Quaternion spans、
managed-object attachment、completion/cancellation，以及 owner-thread destroy 和
GC/finalizer cleanup 的顺序。原 managed wrapper 把运行时数据传给原引擎，不以
伪造完成状态或改游戏 IL 绕过初始化。

## 成批引用、委托与异步生命周期接入

上述委托/按需字段批次的最终完整回归为 **545 passed / 7 ignored**。probe-171
实际执行了 Runtime delegate Invoke，继续到 Canvas 更新；35.36 秒，266 draws /
100 presents，guard checks=337 / denied=0，GC scope depth=0。当时仍缺 Mesh 创建
与异常日志入口，异步实例化异常也仍在日志中，最后画面保持加载页。

直接 CLASS/closed GENERICINST 引用参数若实际继承 UnityEngine.Object，现共用
canonical 对象根、actual assignability、worker pin 和 checked m_CachedPtr 投影。
原引擎收到 guest Mono 身份并执行绑定，回调后同步真实 native Mono 字段；不把宿主
对象地址传给 Win64。实际 System.Exception 引用保持 opaque，原引擎的格式化过程
通过统一 metadata/invoke/explicit-field 路由读取它，保留原异常堆栈。

异步模块一次接入 AsyncOperation 的绑定、状态/进度/优先级/场景激活与析构，
AsyncInstantiateOperation 的等待、取消和 integration budget，共 **13 个配套接口**，
以及 Object 的异步实例化入口。方法的完整语义签名由实际 metadata 核对，目标从
sealed registration 获取，不含 MethodDef token、程序集 SHA 或目标 RVA。Resources
异步加载、UnloadUnusedAssets 和 Awaitable.from_operation 共用同一 operation registry。

operation identity 永不复用，原 engine pointer 留在私有 registry；重复绑定、伪造
身份、跨线程查询和重复释放均拒绝。构造函数先 InternalSetManagedObject、再写 m_Ptr
的真实顺序得到保留。InstantiateParameters 按 actual layout 转换引用/Boolean/标量叶子，
padding 写零；Vector3/Quaternion span 从实际 native IL 存储复制为 owned guest snapshot，
宿主指针不会进入原引擎。输入快照总量有 64MiB 上限，保留至 VM shutdown 回调结束，
避免异步 engine work 持有已经撤销的临时地址。纯 stack/local span 不要求 UnsafeUtility
分配池身份，但仍要求非 NULL、对齐、长度与整数运算有效。

退出增加明确的 owner quiescence：停止新 producer、取消尚未完成的 instantiate、
将已绑定对象的真实 m_Ptr 置零并调用原 InternalDestroy，再退休 Mono handle/root 服务。
foreign finalizer 只将已签发 identity 排队，在有 metadata 服务的 owner scope 中释放。
已进入的 late finalizer 仅对明确处于退出释放过程的记录去重；未知身份与正常期间的
重复析构仍拒绝。所有 retained input mappings 在 VM cleanup 后撤销。

probe-172 实际完成异步创建、托管绑定、Mesh.Internal_Create、Mesh 清理/可访问查询
和 Internal_LogException。35.89 秒，265 draws / 99 presents，guard checks=336 /
denied=0。异步 **created=1 / released=1 / live=0 / retained-bytes=0 / closed=true**，
GC scope depth=0；原引擎异常日志保留了 Mesh 上传等缺口的完整托管堆栈。
最终停止在反射探针的 BuiltinUpdate 缺口，最后帧仍为加载页。

## 字符串来源、数据数组与更长签名

Mesh 数据上传实际为 9 参数，因此共享 ABI 上限扩至 16。编译期入口保存原 ARM64
调用栈地址；整数/浮点寄存器分别分配，耗尽后的参数遵守 Darwin 的自然对齐与窄
标量栈布局，再转换为 Win64 的按位置 8B stack slots。guest callback 为所有栈参数
预留存储，保持 callee RSP 的 16B ABI 对齐。入口池仍完全由 signed binary 预编译。

System.Array 及已验证的 plain-value SZARRAY 输入复用实际 array identity、布局、
canonical root、字段/数组同步与 GC barrier；运行时核对 actual rank-one SZARRAY、
元素 stride 和完整非 Boolean 标量布局。IntPtr、引用/指针/含 padding 的元素不会
按 GPU 数据数组导出。不会逐 Mesh channel 或数组元素类型写 adapter。

ManagedSpanWrapper 同时用于字符串和数值数据，不能按结构名推断 UTF16。字符串
输入从 actual wrapper IL 追踪：实际 StringMarshaller.TryMarshalEmptyOrNullString
将对应 String argument 写入 local，目标 icall 从同一 local 取地址。解析完整指令
边界，核对实际 helper identity/signature、参数顺序和唯一目标调用；重赋值、复杂
参数来源、多包装推导冲突及数值 span 保留缺口。字符串与普通 byref value 可共用
同一 scoped guest payload，精确保留 NULL/empty/Unicode、位置、浮点位和 value alias。

上述 ABI/输入协议完整回归为 **549 passed / 7 ignored**，release build 和修正后的
all-target Clippy（`-D warnings`）通过。probe-173 实际执行 Object.SetName 和
Mesh.SetArrayForChannelImpl，34.89 秒后因重叠值字段拒绝而 abort；原 out-of-sync
消息来自此前查表，不是当前终点。最后帧仍为加载页，abort 没有最终 guard/GPU 数据。

## 显式 union 与统一数组预算

actual metadata 声明为 ExplicitLayout，且所有字段均为对齐、有界、非 Boolean
普通数值，并覆盖完整 payload 时，重叠字段合并为一个 inline cell。原数值位模式
完整保留；引用、Boolean、指针、未覆盖字节与非显式重叠布局保持拒绝。该规则共用
于值字段、byref 和 typed arrays，不按 Color32 或 InputSystem 类型名增加 adapter。
回归同时使用可变 actual metadata API、payload snapshot 和非法布局验收；完整
release feature 回归 **550 passed / 7 ignored**。

probe-174 的全量 snapshot 已更新为 resolved=5,139 / missing=6,310 / unknown=88。
实际 Mesh 上传越过 explicit union，36.74 秒后到达旧 256-entry array prototype
上限，returncode=-6；最后帧仍为加载页。typed array registry 随后改为 4,096 个
canonical 身份与 64MiB 累计 payload 双预算，分配和发布前检查容量，原 token
对齐、rank/stride、实际 layout、根/屏障与 retirement 顺序继续验证。

数组预算批次完整回归 **551 passed / 7 ignored**，release build、all-target Clippy
（`-D warnings`）与 **16 项** metadata/harness/build-tool 回归通过。FontEngine 的
普通数值/输入字符串/引擎对象参数同时纳入共用描述符策略；有原生指针的 font
buffers、复杂 reference outputs 与后台 lifetime 仍按独立协议处理。

probe-175 自动安装 **4,020** 个入口，实际通过 Mesh 颜色/索引上传、BuiltinUpdate
与更多 Canvas 更新，正常退出配置边界：43.64 秒、408,246,343 retired instructions，
236 draws / 90 presents，guard checks=338 / denied=0，GC depth=0。typed arrays
共 275 个，超过旧 256 上限；async created=1 / released=1 / live=0 / closed=true。
最后帧仍为加载页。新的边界是 RenderPipelineManager.DoRenderLoop_Internal 的
render-context IntPtr 参数；Player.log 另记录 Material.CreateWithMaterial 的包装
来源含显式 null guards，简单直线来源分析尚未覆盖。下一批按渲染上下文/命令及
受检查的对象包装协议推进，保持原实际 render loop。

## 成批对象 guard 与渲染所有权协议

对象 marshal 来源分析增加有界、无回边的 CFG：解析完整 IL 指令边界，跟踪参数
来源、dup 和 null 分支；所有 incoming stacks 必须一致。只有实际 metadata 验证过
的 Marshal/MarshalNotNull 和 ThrowHelper 可处理对应转换/终止路径，其他调用、
参数重排、局部/字段写与来源冲突继续拒绝。没有 Material 方法名、token 或 RVA 特例。

probe-176 自动注册 **4,059** 个入口，实际执行 Material.CreateWithMaterial_Injected，
原 MissingMethodException 消失；40.34 秒、244 draws / 92 presents，guard checks=337 /
denied=0，GC depth=0、async live=0。最后帧仍是加载页，pending entry 为
RenderPipelineManager.DoRenderLoop_Internal。这批完整回归 **552 passed / 7 ignored**，
release build 与 all-target Clippy 通过。

ScriptableRenderContext 从实际 metadata 核对唯一实例 m_Ptr、8B size/alignment 和
完整 managed render-loop 签名。每次原始 DoRenderLoop_Internal 调用签发一个同线程、
不可跨线程移动且从不复用的 opaque token；在对应 scope 内还原原 guest engine 指针，
结束立即退休。实例 receiver 和 injected ref receiver 共用同一 byref 协议；原引擎修改
readonly context payload 时拒绝，native token 存储不会被 guest 指针覆盖。

这批最初完整回归 **553 passed / 7 ignored**，release build / Clippy 通过。probe-177
自动注册 **4,077** 个入口，但旧 pre-invoke admission 仍先按普通参数规则拒绝 IntPtr，
所以尚未实际执行 render loop；39.12 秒、263 draws / 99 presents，guard denied=0。
随后修正 admission，使已验证 context entry 与实际 invoke 使用相同描述符。

后续实现以整组协议为单位：CommandBuffer 的 actual InitBuffer / ReleaseBuffer targets、
m_Ptr 来源、所有权与 owner-thread release 单独验证，普通数值/字符串/对象/数组方法
共用入口池。跨类的 CommandBuffer 参数由 actual BindingsMarshaller 的完整
`ldarg.0; ldfld <actual m_Ptr>; ret` IL 验证，nullable wrapper 同时核对转换分支、literal
native NULL 分支及完整 branch targets。创建和使用不把原 guest pointer 暴露给 native
IL；foreign finalizer 入队，owner quiescence 在 Mono handle/root 退休前释放真实引擎资源。

静态清单中 CommandBuffer 共 **179** 个 icall；其中异步读回、GPU fence、
RenderTargetIdentifier 内嵌指针等仍需要各自的生命周期/布局协议，不因普通方法准入
而放开。CullingResults 的 ptr/m_AllocationInfo 和可供 native IL 解引用的数组数据，
也需要独立作用域与 shadow buffer 协议。全量盘点用于提前发现这些整组边界，游戏运行
用于验证实际回调、线程、资源释放和最终画面，当前 `full_game_backend=false`。

CommandBuffer / context 批次最终完整回归 **556 passed / 7 ignored**，release build、
all-target Clippy 与 **16 项** metadata/harness/build-tool 回归通过。probe-178 自动
注册 **4,127** 个入口，比 probe-177 增加 50 个普通接口；39.13 秒后 context 原型的
heap-only 检查拒绝原 C++ 调用借用的数据，returncode=1。退出 command buffers
created=0 / live=0 / closed=true、async live=0、GC depth=0，guard checks=341 / denied=0。
最后帧仍为加载页，这次没有完成 render-loop 执行验收。

借用上下文现同时核对原 suspended call 的 rsp、已有 temporary-slot 规则、8B 可读
非代码存储和 main stack 范围。heap 与 borrowed-stack 分别记录存储来源；每次绑定
使用都重新核对，scope 退休后 token 失效。回归覆盖有效 caller stack 和低于 caller
活跃边界的失效 stack，而非为某个样本指针放宽全部 engine-pointer 规则。

probe-179 越过上述 admission/borrow 检查，开始真实 render-loop 初始化，进入
Shader.set_globalRenderPipeline_Injected。该资源操作在普通 callback 的 2,000,000
work budget 处停止，returncode=-6、39.81 秒；abort 没有完整最终 guard/清理统计。
随后按 actual owner/继承关系，将既有 32,000,000 asset budget 统一用于 Shader、
Material、Mesh、Texture、ScriptableRenderContext 与 CommandBuffer 类族。标量、
UTF16 与混合 byref 共用同一预算策略，不逐方法添加例外；普通 callback 仍为
2,000,000。预算回归同时验证 asset scalar/typed-payload 路由完成后，普通路由仍按
原预算拒绝长调用。

## 成批实施与渲染循环诊断

后续实施单位为协议，不再以游戏第一次抛出的单个方法作为工作清单。实际 metadata、
包装 IL、原引擎 registrations 和静态 unsafe 使用点已经全量盘点；普通 ABI 自动
生成数据描述符，指针/句柄/回调则按共用协议批量接入后统一验收。

渲染清单中的待办如下。数量来自 probe-174 全量 snapshot，保留历史采样身份，
不代表当前 lookup 状态，也不把游戏未用平台接口列为启动必需项。

| 共用协议 | 静态范围 | 实施/验收状态 |
| --- | --- | --- |
| RenderContext 作用域与普通调用 | ScriptableRenderContext 35 个 icall | 作用域与普通 ABI 已实现，实际 render-loop 已进入 |
| CommandBuffer 所有权、marshal 和普通操作 | CommandBuffer 179 个 icall | 两个所有权边界和 50 个普通方法接入；真实创建验收待完成 |
| 带 finally 的对象返回转换 | 同形 engine-object 包装 | 读取真实 EH ranges，核对 native-result、managed-result、pin 三种独立 local；probe-181 实际通过 GraphicsSettings 返回 |
| 大数值 payload | ScriptableCullingParameters、CameraProperties 固定数组/Boolean/padding | 待实现 metadata-derived 字段转换，不能直接复制裸结构 |
| 借用裁剪结果 | CullingResults 14 个 icall，AllocationInfo/NativeArray | 待实现 scope、shadow buffers 和全部消费者 |
| 渲染目标标识 | RenderTargetIdentifier 及其数组 | 待核对内嵌 buffer pointer 的来源、有效性和生命周期 |
| 异步 GPU 与任务 | fence、readback、job callback | 待实现 owner-thread completion、GC roots 与退休顺序 |

probe-180 在统一资源预算的 **32,000,000 work** 处仍停在 shader-property 哈希
查询，49.88 秒后 abort；继续增大预算无助于定位。probe-181 的 finally 返回批次
完整 release 回归 **557 passed / 7 ignored**，build、all-target Clippy（`-D warnings`）
和 16 项工具回归通过。真实游戏自动注册数增加，实际通过
GraphicsSettings.Internal_GetSettingsForRenderPipeline_Injected；但 57.49 秒后仍在
Shader.set_globalRenderPipeline_Injected 的同一查询 abort。表内有正常 empty
sentinel，最后帧仍为加载页。abort 没有完整 guard/资源清理计数。

probe-182 暂时禁用 callback 的批量执行，仍复现同一状态。代码复查发现原
`step_fallback` 在 decode 后会命中单指令 IR，所以该轮是单步 IR 对照。新增独立
`step_reference` 直接使用 decoded transactional executor，保持 checked decoding、
异常与提交边界；默认路径仍使用 IR。自编字符串比较循环回归核对负地址差、I32
减法、不同大小写、空字符串和终止 FLAGS，同时证明已填充 IR cache 后参考执行
仍有 ir_steps=0。超预算诊断同时记录可读寄存器 header 与其中的有界 pointee。

probe-183 使用真正独立的参考 executor，90.75 秒后仍在同样的 32M 边界停止，
查询字串与实际 bucket key 完全一致。probe-184 又采样最后 4,096 work 的 PC 分布：
原哈希查询进入 6 次、完成返回 5 次，上层着色器属性遍历也正常推进。此前根据单个
终止 RIP 怀疑哈希死循环，证据不足；当前证据指向正常批量资源工作耗尽原型预算。
统一资源协议预算因此改为 128M，普通 callback 仍为 2M；不增加单方法例外，也不
改变指针、作用域或执行内存准入。若新预算仍耗尽，应继续检查上层进度与资产规模，
不能把多个停止点具有相同 RIP 当成死循环的证据。

probe-185 实际通过 Shader.set_globalRenderPipeline_Injected，并正常收尾：66.73 秒、
266 draws / 105 presents，guard checks=322 / denied=0，GC scope depth=0，async
created=1 / released=1 / live=0，command buffers created=0 / live=0 / closed=true。
最后帧仍为加载页。Player.log 的 URP 初始化异常来自 MaterialPropertyBlock.CreateImpl，
属于上述 owned-resource 协议，需要与 CommandBuffer 共用 typed identity、marshal、
owner-thread release 和 finalizer 队列，不能把 factory 安装当成完整资源协议。
独立参考 executor 批次完整 release 回归 **558 passed / 7 ignored**；随后预算修正的
普通/资源隔离回归、release build 和 all-target Clippy 均通过。

## Typed owned-resource 机制

所有权注册表由 CommandBuffer 泛化到 `owned_resources.rs`。同一记录包含实际资源
class 身份、原 guest pointer 和 Live / Queued / Releasing / Released 状态；不同 class
的 token 不可互用，同一个 engine pointer 不能获得两个 live owner。最终释放只在原
engine owner thread 执行，foreign actual-finalizer 入队；quiesce 在 VM/Mono roots 退休
前排空资源，随后关闭 registry。退休 token 不复用，即使 engine allocation 的地址
被另一个资源 class 重用，旧 token 仍失效。

本批同时审查零参数 factory / `void(IntPtr)` destructor 的五组生命周期角色：
CommandBuffer、MaterialPropertyBlock、RectOffset、ObjectGUIState、TextGenerator。
这是一张语义准入表；factory/destructor 的完整签名、m_Ptr 实例字段、marshaller
完整 IL 和原始 targets 均读取实际 metadata/registration，没有冻结数值规格。缺少
角色的 profile 保留未支持状态，已有 lifecycle adapter 的整组接口不被覆盖。
预编译的五槽入口池按 descriptor 选择目标，创建/释放复用同一实现。缺少已验证
BindingsMarshaller 的 class 不获得跨类 pointer provenance。

各资源的所有普通方法由共用 scalar/value/string/array 描述符发现，外部消费者从
完整包装 IL 推导带实际 class 的 PointerSource，不增加逐方法 adapter。静态清单中
除 CommandBuffer 的 179 个声明外，MaterialPropertyBlock 43、RectOffset 14、
ObjectGUIState 2、TextGenerator 14；typed span outputs、GPU buffers、复杂 reference
containers 和超过当前 16 参数 ABI pool 的调用仍需对应的共用协议。

probe-186 对整个 owned-resource 批次进行实际验收：自动注册 **4,176** 个入口，
60.77 秒、182 draws / 69 presents，正常退出配置边界。MaterialPropertyBlock 通过
原 engine factory 创建，最终 owned resources created=1 / live=0 / closed=true，
material property blocks created=1 / live=0 / closed=true；guard checks=322 / denied=0。
完整 release 回归 **558 passed / 7 ignored**，release build、all-target Clippy
（`-D warnings`）与 **16 项**工具回归通过。最后帧仍为加载页，URP 初始化异常进入
Shader.get_keywordSpace_Injected。下一批按 shader keyword/view 的完整协议处理，
包括借用 view 的来源、nested reference、字符串与数组，继续使用全量静态清单。

## Shader keyword 借用值批次

全量包装 IL 中，LocalKeywordSpace 的 IntPtr 仅用于相等/哈希和传回原 engine；
LocalKeyword 还包含 LocalKeywordSpace、String 与 UInt32，且由 URP 静态字段跨帧持有。
该协议使用独立、不映射到 native/guest memory 的 identity arena。记录保存实际 Shader /
ComputeShader / RayTracingShader class、canonical managed owner root、创建时 m_CachedPtr、
实际注册的 getter target 和原 view pointer。重复查询保持相等，不同 managed owner
即使重用同一 engine 地址也不会共享身份。每次消费核对原 rooted owner 的 m_CachedPtr，
并调用原 getter 验证 view 未被替换；销毁、重绑定、foreign token 和退休后使用均拒绝。

实际 metadata 决定字段偏移、布局和注册目标。LocalKeyword 输入按字段重建：指针
私下恢复、String 通过既有 roots 转为 canonical reference、padding 归零；输出先验证
全部 payload，再通过 Mono barrier 写回引用。普通 shader/material/compute/raytracing/
command-buffer 消费者从共用 signature 扫描发现。GlobalKeyword 的实际布局只有 UInt32，
继续使用普通数值协议。数组返回、BlittableArrayWrapper 和 ShaderKeywordSet 的多指针
状态保留各自的完整协议待办，不把同名的 keyword 接口视为相同 ABI。

同批增加通用 UTF16 输出识别：完整解析 wrapper 指令边界、真实 finally ranges、
span/result 的不同 local 和实际 OutStringMarshaller.GetStringAndDispose 签名。
只有该流程被证明后，ManagedSpanWrapper 才获得输出字符串协议；numeric spans 不受影响。
原 engine allocation 先验证并复制，所有输出完成转换后才写 native storage，释放仍走
原 BindingsAllocator.Free。带 injected value 输出的字符串输入包装也支持从 metadata
匹配实际返回 class，而非假定 wrapper 与 icall 的参数数量相同。

probe-187 自动注册 **4,275** 个入口（比 probe-186 增加 99），61.96 秒、357 draws /
133 presents，正常退出配置边界；owned resources live=0 / closed=true，guard checks=322 /
denied=0，GC scope depth=0。原 Shader.get_keywordSpace_Injected 已真实执行，最终帧仍
为加载页，URP 的异常进入 LocalKeyword.GetShaderKeywordIndex_Injected。原因是共用
engine-pointer 来源分析未覆盖 StringMarshaller 的多次调用、locals 和分支。修正通过
实际 resolved signatures 计算调用的 stack effect；只有已验证的 Marshal/owned-resource
转换能产生 PointerSource，其他调用返回和 local loads 仅为 unknown value。各条路径
到达 target 时必须保持相同来源，back edges、未知调用 effect 和被消费的来源均拒绝。
这项修复覆盖同形 mixed wrapper，而非给该 keyword 方法添加 adapter。

probe-187 前完整 release 回归 **563 passed / 7 ignored**，release build、all-target
Clippy（`-D warnings`）和 **16 项**工具回归通过。该轮同时进行 Clippy，时间不用于
性能对比；mixed-wrapper 分析变更还需单独完成回归和集中验收。

probe-188 自动注册 **4,462** 个入口、deferred=1,029；57.47 秒后旧曲线原型 abort。
实际完成 LocalKeyword index/count 查询、Material.get_shader/FindPass、GlobalKeyword
创建/index/count、URP batching 与 QualitySettings 查询。完整 lookup audit 是本轮
131 个 loaded image 范围内的 11,615 个声明、5,829 个 missing；与 probe-174 的
153 程序集全量 Cecil 清单分开记录。数值/keyword 普通消费者已 resolved，LocalKeyword[] /
String[]、BlittableArrayWrapper 和 ShaderKeywordSet 仍保留独立协议待办。

keyword identity arena 已从会与 async-operation retained buffers 重叠的 `0x6feb...`
移到独立的 `0x6fe9...`。probe-188 使用修正前的冻结二进制，后续验收使用修正版；
不把那次未触发 retained-buffer 的运行算作地址区间隔离验证。mixed-wrapper 完整
release 回归 **564 passed / 7 ignored**，release build 与 all-target Clippy 通过。

## 曲线生命周期和 pinned 数值 span

AnimationCurve 加入共用 typed owned-resource 注册表。实际 factory/destructor
签名、m_Ptr、BindingsMarshaller 的完整 field-load IL、原引擎目标及 Keyframe
size/alignment 都来自本次 metadata。新曲线使用 opaque identity、foreign-finalizer
队列、owner-thread release 与 quiesce；旧的八条 bootstrap 曲线仍保持其真实 managed
Finalize/SuppressFinalize 验证和收尾，不放大旧构造函数的资源数量限制。

probe-189 自动注册 **4,479** 个入口，62.09 秒后仍进入旧 bootstrap factory 并 abort。
Mono 早期编译的 call sites 保留了旧 adapter：`mono_add_internal_call` 的重新注册
不会使已有调用点失效。现让经过身份核对的旧 factory/destructor 成对转发到共用
实现；未修改 executable code，也没有新增构造函数/MethodDef/RVA 特例。
这轮 abort 同样没有完整 guard/资源收尾统计。

本批进一步覆盖生成的 pinned `T[] -> Span<T> -> ManagedSpanWrapper` 包装。实际
locals、闭合 generic argument、constructor/GetPinnableReference/get_Length 的完整
签名、唯一 target 调用及 pin 清理尾部全部核对。numeric 元素须完整非 Boolean
scalar 覆盖，不含 reference、IntPtr、padding。提前越过初始化的 branch、local
重写、未知调用消费 header、部分 overlap 和不同 element 类型的 alias 均拒绝。
来自现成 Span/NativeArray 的包装、含引用数组和 allocator-owned 数组返回仍需各自
的协议，不因为同为 16B header 而自动准入。

原 native pinned 数据复制到与 UTF16/byref 共用的 guest scope；相同实际 element
range 和 type 保持数据 alias，各 header 独立检查。引擎可修改数组内容，全部返回
长度、alias、普通输出及返回值先核对再提交 native bytes；失败、fault 和 header
修改均保持原数组/输出不变。单 span 最大 65,536 个元素 / 128KiB，整个 callback
仍受原 scope/work budget 限制，退出即撤销所有临时映射。

runtime patch 0025 增加只读 `wire_mono_type_is_pinned` 导出，从运行时自己的 MonoType
布局读取 actual local-signature 标记；Rust 没有冻结 MonoType 的内存布局。25-patch
runtime 保持 no-JIT、MSVC class ABI 和原 corelib support。普通/incremental 两种配置
各通过既有 11 组原生 fixture 与 5 项可执行内存拒绝测试；链接对象审查为 304 个
objects、668 个 dependencies、0 unmatched。完整 release 回归 **569 passed /
7 ignored**、16 项工具回归通过。具体游戏执行与最终资源清理还需集中验收。

probe-190 自动注册 **4,591** 个入口、deferred=923；与 probe-188 的同一 loaded-image
lookup audit 比较，有 **129** 个 missing 入口变为 resolved，分布在 **36** 个类。
该数量包括连续两批变更，数值 span 单批相对 probe-189 增加 112 个自动注册入口。
游戏真实创建一个 MaterialPropertyBlock 和 **16 条新 AnimationCurve**，经过更多
URP VolumeComponent 构造/OnEnable。57.85 秒后 SIGSEGV；最终帧仍为加载页。
异常退出缺少 guard 与资源收尾证据，results 中的默认零值不能计为正常释放。
全量平台接口中仍有非游戏所需声明，不用 missing 总数估算游戏完成比例。

## 保留原 IL 的引擎对象头读取

probe-192 的 LLDB 现场确定 SIGSEGV 是 `UnityEngine.Object.GetInstanceID()` 中
`ldind.i4` 对 guest 地址的直接宿主解引用；调用链为 RTHandle.SetTexture ->
RenderTargetIdentifier 构造 -> GetInstanceID。m_CachedPtr 按既有真实绑定协议保存
guest C++ 对象地址。原 IL 将它与真实静态 OffsetOfInstanceIDInCPlusPlusObject 相加
后读 I4，在原生 Mono 中需要一个明确的地址空间边界。

runtime patch 0026 增加 immutable、exact-method 的 scalar-read gateway。在实际
core/corelib image family 完整建立后的 ordinary IL invoke 边界、该方法首次 transform
之前注册；已编译的 unchecked
transform 拒绝接入。该方法保留原始 IL、null 分支及返回位模式，禁止被 inline，
其解释器 transform 使用原受检方法的优化限制。在唯一受检 I4 load 中调用预编译
Rust 回调，不改 executable memory、不换构造函数、不生成 managed 替代体。

准入核对实际实例签名、全部 locals、完整无异常处理 IL、每个指令边界、两条
return 分支、null/non-null 分支、实际字段和 IntPtr helpers 的完整签名。FieldDef /
MemberRef 来自当前 image 解析，无固定 token/spec。读取偏移来自原方法已经加载的
真实 static field；不固定为 8，不制造 instance ID。回调核对 canonical strong root、
Unity base/derived class、当前 m_CachedPtr、精确 pointer+offset、I4 宽度和 live guest
data 范围，再从 Memory 读原始值。没有 callback owner、伪造/退休指针或错误类型
返回拒绝状态，由 Mono 原异常边界抛 AccessViolationException，禁止 native fallback。
其他方法继续使用原来的宿主地址语义。

独立原生 fixture 在禁 JIT 构建与 noexec guard 下验证原 null IL、派生 receiver、
GC 后身份、checked guest I4 位模式、拒绝时托管异常和其他 native pointer load 隔离。
fixture 使用独立程序集/support，不覆盖游戏 corelib support。26-patch runtime
SHA256 为 `915541508f53a6f237b1b18634340134952767607b807e9adae48857357cb6fa`；
corelib support 仍为 `6bb5f274387eb907ecc97ddc963bc832d249adf52a452b58b1d417fa50424e0e`。
链接对象审查仍为 304 objects / 668 dependencies / 0 unmatched。游戏验收待记录。

probe-193 暴露了注册早于 corelib family 准入的问题，注册已移到上述 ordinary IL
入口。probe-194/195 确认新增读取代码将 `mono_field_from_token` 的 retklass 误传为
NULL；该 API 命中字段缓存仍会写输出。已改用真实声明类输出 cell，独立 native
fixture 增加首次/缓存字段解析回归，普通与 incremental 配置均通过。

probe-196 正常完成共用 gateway 登记和 4,591 个 metadata-discovered icall 的注册，
但 35.07 秒后在 Mesh.Internal_Create 的 guest wait 中被既有 cooperative scheduler
拒绝；尚未执行到 GetInstanceID 的实际读。真实 worker #3 调用
mono_gc_wbarrier_set_field(NULL, <worker stack cell>, <attached worker Thread token>)。
这是等待时调度路径与正常 worker 路径的协议覆盖差异，不新增 Mesh 方法适配。

等待调度器现可服务 NULL-owner barrier 对 private C++ 临时引用槽的 token store。
复用暂停 owner 的 canonical root route，并由当前真实 worker resources/TEB 核对
栈范围、RSP、对齐、返回槽与 shadow space；栈内目标必须位于 RSP+40 之后，不能
以 allocator 记账绕过 shadow-space 限制。非栈目标只允许 live original/private
guest data；native managed fields/shared reference views、code、foreign token、
非 NULL owner 和退休 worker 均拒绝。整个过程只写 guest token，不把目标地址
交给宿主 Mono barrier，不切换实际 Mono TLS、不执行 managed IL，也不另起 guest
执行线程。回归使用真实 CreateThread resources 验证 TEB、slot、root、返回和失败
不写回。涉及分配/托管调用或其他 worker 状态的 gateway 仍须其原独立协议。

## 批量身份、等待调度和数值结构体

probe-197 的 `0x6ffe00001008` 经 attach/current 日志核对是 worker #3 的 Thread
token，前一轮交接将它误认为 String。原正常 worker dispatcher 已支持这种临时
引用，协作等待路径缺少 attachment 身份检查。现用实际 OS worker 命令完成时的
receipt 核对 guest ID、owner、attached、精确 token 和重新 attach 的 generation。
Metadata scope 共享借用 workers State，期间不能 detach/re-attach；临时引用
只保存 token，不查询主线程 Mono TLS，不把 Thread native pointer 发给 guest。

同批加入只读 domain/root/current 查询：domain/current 来自被 scope 冻结的实际
attachment receipt，root 来自 retained domain；未 attach 的 domain 为 NULL，
current 查询拒绝。分配、attach/detach 和 managed invocation 继续走原实际 OS
worker 通道。栈槽和私有数据槽检查沿用真实 TEB/resources，失败保持目标不变。

启动期 String、RuntimeType、empty SZARRAY 在 ordinary graph API 首次建立时，
先验证原 strong root、实际 class/immutable payload，再按原 token 纳入 canonical
graph。String/array/field/handle/reentry 共用 reverse identity；重复采用幂等，
改变内容/身份或退休拒绝。两个 allocation 入口共用初始化方法。attribute roots
继续使用其独立 checked service。

数值结构体支持最多 4096B，保留 actual alignment、leaf offsets、Boolean 槽与
全部 numeric bytes；padding 是桥接自有零字节。输入/输出都校验 Boolean，返回值
和所有输出通过后才提交 native storage。同范围 alias 必须具有相同 actual value
class/完整布局，numeric 与 semantic payload 不互为 alias；部分重叠仍拒绝。
canonicalization 原地处理已拥有的 snapshot，避免增加一次 payload allocation/copy。

runtime patch 0027 只读解析真实 corelib FixedBufferAttribute 的完整 blob，核对
actual constructor signature、唯一声明、element Type、positive bounded count 和
无 named arguments；不实例化属性、不执行构造 IL。Rust 核对 actual storage 的
UnsafeValueType marker、size/alignment、唯一 element field 与 primitive type。
全部吻合才展开完整元素；首元素外的数据不能误当 padding，standalone unsafe
storage 拒绝。非 Boolean buffer 使用完整 inline range，Boolean buffer 逐元素
校验并保留原 64-slot 上限。离线 Cecil inventory 同样记录 `fixed[count]:element`，
缺少 declaring-field proof 的 unsafe storage 记为 unresolved。

本批先完成独立回归，再统一进行游戏验收。27-patch no-JIT runtime SHA256：
`3058346e80d719110015eca91887b0c2021118e7150637a70666e809a054f8c2`。
普通与 incremental 各通过 12 组 native fixtures、5 项 noexec guard 拒绝测试
（exit86）；实际 compiler-generated float[24]/byte[1024] 元数据和 engine-header
原 IL fixture 通过，guard denied=0。审查 304 objects / 668 dependencies / 0 unmatched。
最终完整 release Rust 回归为 **577 passed / 7 ignored**，all-target release
Clippy（`-D warnings`）通过，17 项工具回归通过；最终 release binary 已重建。

fixture 编译/运行的默认输出现为 `root/fixtures`，游戏专用输入仍为 `root/probe`；
显式 fixture-output 禁止指向游戏 probe，避免验证时覆盖冻结 support。一次旧工具
默认编译曾覆盖 probe 中的 SDK classlib，harness 在启动前拒绝；已从唯一游戏
sandbox 恢复原 DLL，并重新建立/校验 support，游戏原始 mscorlib SHA256 未变。
当前游戏 support SHA256 为
`09c81365daa1bcbcbd792c7d00b47aa765dd23a7ae6cd53d2f7770bdce5bc6fd`。
隔离的 SDK 与游戏 profile 均通过 12 组原生夹具；游戏 profile 普通/incremental
验证使用上述实际冻结 support，而非另行生成的同源文件。

probe-198 自动安装 **4,639** 个入口、deferred=875，数值布局批次相对 probe-190
增加 48 个入口。35.30 秒后协作等待在 worker #3 的 mono_runtime_invoke 停止；
临时 Thread 引用保存已越过，下一缺口属于等待调度与实际 OS worker 调用通道的
接合。尚未记录 GetInstanceID 的实际 checked read；本轮 returncode=-6，没有
正常 guard/资源收尾数据，results 默认的零值不计为清理成功。后续先成批统一
worker 调用、分配和虚方法协议，再集中进行游戏验收。

## 协作等待复用实际 OS worker 通道

等待调度采用两阶段 gateway：保留真实 guest context/PC，释放 Memory/Runtime
借用后，再由原 attached OS worker 执行分配、零参数 managed invocation、虚方法
解析和 lazy metadata。字段、views、handles、typed barriers 与 userdata 复用原
rooted route；只读 attachment 查询保留 leaf 通道。临时切换真实 guest TEB 后
以 RAII 恢复 owner，递归调度与同 worker 重入在发送命令前拒绝。没有把 main
native Mono TLS 用作 worker TLS，也没有增加按类构造函数替代实现。

命令接收器独占借用直到真实 completion；profiler callback 失败仍消费 completion，
退出 GC Safe 后释放返回 roots。metadata preparation/completion 核对同一 retained
operand、kind、parent、iterator 原状态与 guest output slot，退休或已推进的状态
不提交。新增回归覆盖实际 OS worker TLS、callback failure 后继续请求、递归拒绝、
context/PC 恢复和 stale metadata。完整 release 回归 **580 passed / 7 ignored**，
all-target release Clippy（`-D warnings`）通过，release binary 已重建。

probe-199 正常结束（exit0），越过 InputSystemUIInputModule 构造的等待阻塞，
实际执行 checked GetInstanceID I4 read，并创建 RenderTexture 与 CommandBuffer。
本轮 42.85 秒、403,108,513 retired、269 draws / 101 presents；尚未做性能对照。
正常收尾记录 GC disable/enable=9/9，worker attach/detach=2/2、live=0，owned
resources、AnimationCurve、CommandBuffer、async 与 unsafe buffers 均正常关闭，
noexec checks=337、denied=0。画面仍是蓝底沙漏首帧，frame hash 为
`6a9315288a646561e394147f3b32a58ccf2bc3bfb2615da52bbf4a00783124f0`。

下一批缺口来自共用 pointer payload 协议：SupportedRenderingFeatures 的多个
IntPtr scalar output 查询，以及 RenderTargetIdentifier/RenderBuffer 的借用来源。
URP 初始化已先因 SetRenderTargetSingle_Internal_Injected 未解析抛异常，正常
边界随后停在 IsUIOverlayRenderedBySRP。需一起处理，不能以越过查询作为加载
动画恢复的验收。继续按实际 metadata/完整 IL 推导，不固定 MethodDef/Spec 值，
guest pointer 不直接进入 native managed ABI。

## 完整 IL 推导的标量 pointer output 与无 buffer 渲染目标

新增 scalar output 协议从实际 static void IL、IntPtr 参数和 pointer locals 推导
有限 primitive/enum 输出。完整解码并检查有界无环 CFG、指令边界、stack/local
provenance 和所有 normal return 的 definite writes。唯一地址转换须是实际 corlib
IntPtr::op_Explicit(IntPtr)->void*；普通 call/field 的实际签名禁止 pointer、IntPtr
或 byref 传播。ldind、pointer arithmetic、未知调用逃逸、异常处理、back edges、
未写输出的分支及不一致 pointer joins 均拒绝。没有按类名、MethodDef 或 Spec
安装查询名单。实际 SupportedRenderingFeatures 的 11 个查询通过同一套检查。

Frame 为每个输出提供前后 canary 的对齐私有 cell，guest IntPtr 不进宿主调用。
相同范围且相同 actual type 的输出共用同一 cell；部分重叠、不同类型 alias、
参数 vector/input cell 和 exception slot overlap 拒绝。canonical Boolean、完整
canary、返回值和全部 live output storage 通过后才事务写回。顶层与 nested ordinary
invoke 使用同一协议；已有 input-event/render-context 专用描述保持优先级。

RenderTargetIdentifier 使用独立 semantic payload：实际字段、enum/IntPtr class、
offset、size、alignment 和无重叠布局逐项核对。保留原 numeric IDs，padding 为
自有零字节，m_BufferPointer 必须为零；非零 RenderBuffer 仍需要 retained typed
owner/lifetime，未放宽为普通 numeric 字段。输入 readonly，engine 改写拒绝；
semantic/numeric/keyword/context payload 不互为 alias。新增 15 个 metadata-discovered
icall（总计 4,654，deferred=860），不更改游戏/Unity IL，不涉及 IL2CPP。

独立原生夹具通过 Boolean、enum 的两条分支、I8、同址 output alias；拒绝原输入
读取、pointer offset/escape 和未完整写回分支。新工具
`tools/mono/probe_scalar_outputs.py` 只在隔离 fixture 子目录生成原始夹具/support，
不写游戏 root/probe 或原 classlib/support。SDK 与实际游戏 classlib 的普通/
incremental GC 各通过该夹具，并只读核对实际 CoreModule 的 11 个查询和渲染目标
布局；每轮 noexec checks=41、denied=0。完整 release 回归 **586 passed / 8 ignored**
（新增原生 fixture 单独执行通过），all-target release Clippy（`-D warnings`）通过。

probe-200 首次窗口验收在输入事件上发现新 descriptor 抢先处理已有专用协议的
准入顺序回归；已修正为专用 descriptor 优先，再跑 Mono 回归并重建 release。
该次游戏未越过 probe-199 的边界，不记录为场景/动画推进。后续游戏验收另记。

probe-201 实际执行 SetRenderTargetSingle_Internal_Injected 的原 engine target，
不再在 URP 初始化抛出该 unresolved icall 异常；完成 IsUIOverlayRenderedBySRP
和 GetRenderFrameInterval 的受检 scalar output，继续到 OnEndOfFrame/Awake。
44.07 秒后停止在顶层引用 barrier 的旧 startup stack 限制（returncode=1）；
263 draws / 100 presents，画面 hash 与 probe-199 相同，沙漏动画尚未恢复。
继续统一 main/nested 的 metadata-discovered reference cell 与 canonical-root
通道，不放宽为任意 heap reference 写入。

main 与 nested barrier 现共用 `reference_cell_at`：只有 retained ordinary reference
owner 的精确、对齐实际字段偏移能触发 discovery，且只安装该 reference field。
未知位置、numeric/pointer 字段、boxed value、engine 对象、歧义字段或错 owner
保持拒绝；private/NonSerialized reference 的显式访问不改变序列化选择。完整
类布局中无关的 private pointer 不再阻止这一项合法 reference cell 的准入。
原生 fixture 的 OpaqueReferenceCell 包含 IntPtr 与 String 两种字段，验证只选择
String、pointer 不出 view、非对齐偏移拒绝。SDK 普通与游戏 classlib incremental
原生验证均通过，完整 release **587 passed / 8 ignored**，all-target release
Clippy（`-D warnings`）通过。startup stack 拒绝诊断增加 args/owned/rsp，便于
一次定位剩余存储/身份问题；没有放松旧 stack 范围规则。

probe-202 的准确拒绝参数为 owner=NULL、destination=`0x6fff21fb0020`、
value=`0x6fff21eb0000`；destination 属于 CreateAsyncInstantiateOperationResultArray
的返回数组首元素（canonical token+32），并非 ordinary object field。该结果
已 strong-rooted，但旧 ordinary return route 尚未发布数组 header/payload，因此
不会进入既有 typed array barrier。图像 hash 仍未改变，43.0 秒后 returncode=1。

ordinary IL 的 reference return 与 reference outputs 现在按实际 array rank 纳入
既有 container graph publication，保留原 canonical identity。main 与 nested
入口均完成 publication 后才返回 guest；opaque non-array helpers 保留各自协议。
没有添加 async instantiate 方法名单、固定 array offset/length 或伪造完成状态。
新增原生 fixture 覆盖先 root 后 publish 的数组、NULL-owner 首元素 barrier、实际
native 元素同步、GC 后 identity、forged reference 拒绝及 roots/views 正常退休。
SDK 普通与游戏 classlib incremental 均通过，noexec checks=41/denied=0；完整
release **587 passed / 8 ignored**，all-target release Clippy（`-D warnings`）通过。

probe-203 已越过 CreateAsyncInstantiateOperationResultArray 返回数组的首元素
写屏障，随后调用 InvokeCompletionEvent 并进入 Start。当前错误是
UIElementsUtility 静态初始化异常；完整原 IL 显示其中构造 UnityEngine.Event，
实际日志同时记录 Event::Internal_Create / Internal_Destroy 未接入。RenderAs2DUtil
初始化也报告 unresolved icall。本次 43.05 秒后 returncode=1，264 draws / 100
presents，noexec checks=338、denied=0；画面仍为蓝底沙漏首帧，hash 与先前相同。
接下来成批接合跨 engine module 的 owned resource 生命周期、actual Int32 factory
ABI 和相关 scalar family，并增加不执行 managed ToString 的内部异常链诊断。
范围限定于 Mono 桥接，不修改 IL2CPP。

owned resource discovery 现在遍历 retained UnityEngine.* images，并拒绝同名
类歧义及 factory overload 歧义。actual metadata 决定采用 void-argument、Int32
或已证明 numeric-span 的 compiled adapter；Int32 按 Win64 ABI 保留低 32 位。
Event 的创建/销毁加入同一 typed identity、owner-thread、finalizer queue 协议，
常规属性由既有 scalar engine pool 自动扫描。RenderAs2DUtil 与
UIElementsRuntimeUtilityNative 复用 actual-signature / registered-target 的
scalar family；没有新增按构造函数或 Spec 值替代原 IL 的路径。

异常诊断只读 actual System.Exception 的 _message/_innerException 字段，逐项
核对类型/实例字段归属，用 pinned roots 保留有界 chain，并限制循环及深度；
不运行 ToString。原生夹具覆盖多层 TypeInitializationException、depth limit 和
cycle。SDK 普通与游戏 classlib incremental 均通过，noexec checks=44/denied=0；
actual IMGUIModule Event factory/destructor 元数据验证通过。完整 release 回归
**588 passed / 8 ignored**，all-target release Clippy（`-D warnings`）通过。

probe-204 安装 4,705 个 scalar pool 入口（deferred=874），actual
RenderAs2DUtil::InitializeCanRenderAs2D 已执行。这使原 URP render loop 继续
初始化 camera/rendering/shadow/light data；ScalableBufferManager::ResizeBuffers
及 ScriptableRenderContext::Internal_Cull_Injected 成为实际 unresolved icall。
最终 DoRenderLoop_Internal 抛 MissingMethodException，47.73 秒后 returncode=1；
尚未再次到达 Start/Event 创建，因此 Event 生命周期仅完成元数据/独立 ABI 验证。
不将这次更早的渲染分支错误记录为沙漏动画恢复，后续接合裁剪结果 borrowed
context lifetime 与 native buffer views。IL2CPP 未修改。

## Render-loop 借用的裁剪结果

Internal_Cull_Injected 采用 actual-signature checked compiled entry，仍执行
原注册的 guest engine target。ScriptableCullingParameters 使用完整 numeric
layout（本游戏 1592B），context 恢复原 guest pointer，结果先写入私有 guest
payload。实际 CullingResults（16B）和 CullingAllocationInfo（40B）的全部实例
字段按名字、类型、pointee class、动态 offset/width/alignment 核对，缺失/额外/
重叠/错类型字段拒绝。三组 VisibleLight/VisibleReflectionProbe buffers 按 actual
numeric layout 校验 count、完整 extent、Boolean 和零 padding，再建立 native
numeric snapshots；guest pointers 不供 native IL 解引用。

结果句柄使用独立、不可复用的 identity，绑定原 render-loop scope/thread；
CullingResults scalar methods 通过同一 pool 恢复原引擎句柄。scope 退休后禁止
重新调用 engine。私有 numeric snapshots 留到 VM cleanup 防止 escaped unsafe
views 成为悬垂地址；当前诊断路径上限为 4096 组、32MiB retained data，单次
最多 2MiB。长时间运行的回收仍需明确 NativeArray view owner 协议。

跨页 numeric guest buffer 要求完整范围位于一个 live private allocation，每页
均 committed/writable/owned，且无 code history；旧 scalar-slot 快路径保持独立。
独立测试执行真实 guest 写入指令，验证跨页数组私有 snapshot、guest 修改后的
snapshot 身份、负数/过大 count、输出事务提交、context/foreign-thread 拒绝和
NX 后代码页仍不准入。实际游戏 metadata 与 SDK 普通/游戏 incremental classlib
原生夹具均通过，noexec checks=46、denied=0；release build 与 all-target Clippy
（`-D warnings`）通过。窗口验收另记。

probe-205 实际完成 Internal_Cull_Injected，生成 scoped result identity，随后
执行 ScriptableRenderContext::Submit_Internal，并成功创建两个 engine Event。
UIElementsUtility 初始化阻塞已越过，多个 Start entry 进入原 IL；仍有 SortingLayer
BlittableArrayWrapper 输出与 FreeNativeOwnedMemory unresolved，Render Graph 报错
后继续运行，最终在 mono_runtime_invoke 的 NULL address 访问停止。增加错误路径
的实际 callback target/PC/arguments/registers 诊断，不改变解释器正常执行。
49.56 秒后 returncode=1，271 draws / 102 presents，画面 hash 仍未变化；noexec
checks=337、denied=0。完整 release 回归 **591 passed / 8 ignored**。

## 数值数组输出与空输入边界

BlittableArrayWrapper 输出现在共用 scalar entry pool，元素类型由实际 generated
wrapper 的完整 IL、finally 范围、InitLocals、三组局部类型及闭合
Unmarshal<T>(ref T[]) MethodSpec 推导。实际 out Param 属性必须匹配；多个 wrapper
推导类型冲突时拒绝。当前支持完整 non-Boolean scalar 覆盖的 primitive、enum 和
numeric value；含引用、pointer、padding、数组输入/inout 或多输出形状仍不准入。
没有写死 MethodSpec token、Int32 element 或目标 RVA。

实际 BlittableArrayWrapper、UpdateFlags enum 与 NativeOwnedMemory header 的字段、
归属、pointee、完整 size/alignment/动态 offsets 均核对。enum 值读 metadata constant
blob，不执行静态构造。默认状态要求实际 NoUpdateNeeded=0；其他符号值从元数据读取。
原 guest header/数据完整范围必须为 live private noncode storage；数组经私有 native
numeric snapshot 后才写回 native output。原 Unmarshal IL 继续建立真正托管数组。
DataIsNativeOwnedMemory 的 native header 有独立 issued identity，FreeNativeOwnedMemory
只接受 owner-thread 上 exact live header，恢复其原 guest allocation 并调用实际
注册的引擎 free。重复/伪造/跨线程释放拒绝，失败时 native output 不写回。

快照和退休 header 在 VM cleanup 后释放，地址在此前不复用；诊断路径有限容量为
4096 组、32MiB retained、单数组最多 65536 元素/2MiB。长期回收仍需扩展已证明的
managed consumer 生命周期，不能把这个有界 arena 当作完整长时间运行方案。

probe-207/208 将先前裸 NULL 错误定位到 NativeInputSystem.NotifyUpdate 的
prepare-frame/input-events 阶段。原 InputEventBuffer 构造 IL 在 eventPtr=NULL 且
eventCount=0 时保留默认空 buffer，跳过容量存储；旧桥接却检查 NULL + capacity
hint 的范围。现在空事件状态保留 capacity 数值但不访问零地址，也不发布不存在的
storage；非空 NULL 事件和未持有的非零输出 pointer 继续拒绝。

新增测试覆盖完整/截断/错边界 finally、跨页 numeric snapshot、变化的 flag 常量、
负数/过大 count、NX 代码历史拒绝、原 guest free 指针恢复、重复/跨线程释放，以及
NULL 空事件带非零容量的输出。SDK 普通与游戏 incremental classlib 原生夹具均通过，
实际 SortingLayer 闭合 Int32 output metadata 验证通过，noexec checks=46/denied=0。
完整 release **594 passed / 8 ignored**，release build 和 all-target Clippy
（`-D warnings`）通过。IL2CPP 未修改。

probe-209 自动接入 4791 个 scalar pool declarations（deferred=811）。实际执行
SortingLayer 的 4B/Int32 数组输出，flags=DataIsNativeOwnedMemory，原 Unmarshal
完成复制并通过新 adapter 释放 guest header。下一空事件更新正常完成，输入 NULL
错误已经越过；随后创建多组 GameObject、配置新的相机及 UI。原渲染循环还缺
SpriteMaskUtility 数值接口，添加组件时首次遇到 nested mono_error_init 未路由。
50.43 秒后由明确 adapter reject 停止（returncode=-6）；帧 hash 仍为原加载首帧，
未正常收尾的 guard/GPU 总计不作为验收数据。

MonoError 的 104B 外部状态快照现在共用主入口/嵌套入口的 Registry，范围来自
当前 owner callback stack。只写/read status word，native 私有资源始终由 actual
runtime wrapper 处理；逐页核对 owned/noncode 权限、对齐、当前 RSP 和完整范围，
回调完成撤销其 stack slice 中的状态，防止后续复用同地址而继承旧身份。
SpriteMaskUtility 复用现有 numeric scalar/value descriptor。嵌套状态批次的完整
release 回归 **595 passed / 8 ignored**，模块回归 **189 passed / 1 ignored**；
all-target release Clippy 通过。后续窗口结果另记。
