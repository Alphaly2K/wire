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
整个启动 provider 仍锁定 Alice 与 24-patch native runtime；解除这些限制、验证其他
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
