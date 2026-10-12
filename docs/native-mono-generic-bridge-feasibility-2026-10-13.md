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

ARM64 signed executable 预编译一个 4,096 槽的入口池。每个槽只有固定的 slot index
和到共同入口的分支；共同入口保存 x0–x7/d0–d7，按数据描述符转换 ARM64 的独立
整数/浮点寄存器分配与 Win64 的参数位置/stack ABI。返回通过 RAX/XMM0 位模式再
转换至 x0/d0。入口池不会在运行时生成、修补或新增可执行映射；描述符只是私有数据。

支持范围为最多 8 参数的静态 primitive/enum 调用，以及已准入类族的 plain-value
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
