# 配置克隆、Awaitable 与 PlayerLoop 接入

在本地 main 继续，上一批已提交为 `45c153a`。目标仍是 Alice 的可交互游戏界面，
不能把配置加载、过渡背景或 Present 当作完成。

probe-129 已实际执行 Internal_CloneSingle_Injected，返回原 Unity 的真实 v2 句柄。
普通克隆和带 parent 克隆共用 live engine pointer、Boolean 与结果类型检查。
接着完成背景配置构造；数组元素因旧六个 image 集合被拒绝。typed array 现按当前
持有且记录指纹的完整 live image family 检查，仍要求 concrete metadata 和完整值布局。
这不接受开放泛型、未知 image 或一般原生指针。

Awaitable 的十个固定声明先逐项核对签名与原 registration target，再注册已编译入口。
next frame、seconds、fixed update、end frame 和 AsyncOperation 转发原 Unity；完成查询、
cancel、release 仍由原引擎执行。原 engine allocation 只在私有注册表中，native IL 收到
永不复用的 token；释放后地址复用不会恢复旧身份。原 IL 创建的强 GCHandle 经 typed
target 核对后转交已有 guest handle registry，让引擎释放对应的真实 native handle。
不把宿主句柄值交给 Win64，也不假造异步完成。

PlayerLoop 的 default/current getter 与 setter 转发原始引擎。数组里的 Type 与 delegate
使用真实托管 roots/reference barriers。两个 IntPtr 字段只对冻结 CoreModule 的
PlayerLoopSystem/PlayerLoopSystemInternal 精确 declaring class、field、offset、type 准入。
它们是 Windows 引擎地址数据：仅原 getter 的作用域可登记，限定实际 UnityPlayer image
范围、可读地址及两个字段各自的已产生集合。普通值布局继续拒绝 IntPtr；这种 slot
也不能作为 blittable GPU scalar payload 或普通 native invoke pointer 使用。

mono_object_new 与 String/Array 分配一样拆为 prepare/execute/finish，实际 Mono 分配期间
不持 Metadata/Memory/Runtime 的 Rust 借用，临时真实强 root 覆盖返回前的生命周期。
旧直接 dispatch 的对象分配分支已移除；新增回归在实际分配函数里触发 profiler heap
resize，并执行原 guest 指令，覆盖同步 GC 重入。

真实 PlayerLoop/Awaitable 执行结果仍待后续 probe；安装和单元回归不算实际游戏调用。
full_game_backend=false。

probe-130 已越过完整 live image family 的数组布局检查，随后在嵌套
`mono_unity_gc_disable` 停止。GC scope 现由主路径与所有 Metadata 重入共享同一份
计数和真实 Mono 状态，probe-131 已实际越过此处。该轮继续到配置克隆中的
`mono_gc_wbarrier_set_field`：NULL owner/NULL value 写入真实 Unity 临时堆槽位，
旧重入路由只接受 callback stack，因此拒绝。

该临时槽位现按真实 live allocation ledger、8B 对齐、owned non-code access 和
完整引用身份检查准入；padding 与已释放分配仍拒绝。实际 HeapAlloc/HeapFree
回归通过。2026-10-12 当前完整 release feature 回归为 **521 passed / 7 ignored**，
all-target Clippy 与格式检查通过。临时槽位修复的下一轮真实游戏验证尚待完成。

测试不再每轮复制游戏：主目录中的一份隔离目录通过临时软链接复用，退出/失败/
超时均清理临时输入与 wire 快照。详见 `test-storage-2026-10-12.md`。

主目录 `wire-out/native-mono-m1` 现复用唯一一份 locked source，验证旧 9 个补丁前缀
后应用其余 14 个补丁。runtime/AOT 工具使用 fresh build objects；运行库仍禁用 JIT，
启用 MSVC Class ABI。构建审计仍为 304 linked objects / 668 dependencies / 0 unmatched，
十套 native fixtures 的普通与 Unity incremental=3 配置均通过，五个 executable
memory negative routes 在两个配置下均 exit 86。

- 新 runtime SHA256：`9c777efecc3066b3f574968e2566591cafa77a0d35306cb920dc77177f913b4c`。
- 新 support SHA256：`def3ecff994baa8542b1926b6b19875457224fe841dc13076a14670cf13c43c7`。

probe-132 在实际执行之前被旧 runtime 指纹拒绝；已完成上述重建验证后才更新精确
准入指纹。未放宽 runtime/support/game fingerprint 检查。该轮未进入 Unity 运行，
退出后临时软链接与 wire 快照均实际清理。

probe-133 使用新的精确指纹实际运行 Unity，17.27 秒后停在角色配置容器的
`value layout requires an owned metadata value class`。已越过临时堆写屏障和嵌套
GC scope，实际克隆了 CameraConfiguration 和 CharactersConfiguration，并展开
其通用字段/容器图。该轮目录仅 5.1 MB，只有日志、哈希、结果和画面；退出后没有
游戏 runtime、副本 binary 或 dylib 副本。

值布局原先将每个子 struct 限定在根 struct 的同一 image。新增 `layout_in` 由
调用方传入已持有且记录指纹的 bounded image family；根类型仍必须属于明确的
根 image，子值类型只能来自传入的 family。typed arrays、普通字段、box/unbox
和 managed scalar aggregate 使用同一契约。闭合类型、递归深度、字段声明者、
重叠、对齐、padding、Boolean、reference barrier 与 raw pointer 拒绝规则不变。
single-image `layout` 保持原限制，给只接受 CoreModule 的固定 ABI 使用。

这项修复随后由 probe-134 验证越过角色配置；尚无执行成功的
PlayerLoop/Awaitable 证据。

跨 image 值布局修复之后，完整 release feature 回归 **522 passed / 7 ignored**，
格式检查与 all-target Clippy 通过。probe-134 实际越过所有 Naninovel 配置，包括
UIConfiguration/UnlockablesConfiguration，进入 EngineInitializationUI 与 CanvasScaler
构造；22.04 秒后停在嵌套 `mono_object_get_virtual_method`。这是尚未接入重入路由
的通用虚方法解析，非缺少这些类的构造函数。

嵌套虚方法解析现在复用现有 checked resolver 与 pinned receiver request，分为
prepare/execute/finish。执行 native resolver 时不保留 Metadata/Memory 借用；仍
核对 rooted reference receiver、声明者 assignability 和非静态方法，结果只发布
metadata token，不发布 vtable/code 地址，也不在解析时调用方法。
worker domain/current 查询日志限制为前 16 次和计数的二次幂；attach/detach 仍记录，
累计统计保持完整，避免长期循环输出同一条诊断。

probe-135 实际执行上述嵌套虚方法解析，继续展开初始化 UI 的 UnityEvent、Slider、
PersistentCall 与 ArgumentCache。22.43 秒后在 Resources.Load 的旧独立 2,000,000
work 预算停止。probe-136 增加诊断后重现：target=0x1800b88f0，最后
RIP=0x18123c362，depth=1，38,201 次 Mono 重入 / 13 次 managed invoke。
最后位置在原 Unity 资源分配路径；对象图仍持续增加，未据此宣称加载成功或无限循环。

已验证签名及原 registration target 的 Resources load/builtin 与 Object clone
adapter 现在选择独立 32,000,000 work 上限。普通标量、GC/profiler callback 保持
2,000,000；没有运行时任意预算开关，也不改变深度、借用、root 或地址检查。
预算恰好执行完 RET 时允许正常返回；失败诊断包含 target/RIP/work/limit/depth 和
重入计数。真实 guest 指令回归覆盖准确边界、无限循环拒绝、失败后 TEB/depth 复原，
以及大型资源预算不会影响前后的普通 callback。完整 release feature 回归
**524 passed / 7 ignored**、all-target Clippy 和格式检查通过。

代码提交到本地 main：`f29769a`。probe-137 继续执行了更多 UnityEvent 与 Image
OnAfterDeserialize，23.60 秒后停止于尚未接入的 callback control transition。
该轮已越过旧 2M work 上限，没有跑满新 32M 上限；仍未证明 Resources.Load 正常
返回。下一轮增加真实 gateway/control 诊断，以区分需要的 Win32 callback、调度
或 loader 行为，不将这些请求当作成功忽略。

probe-138 定位到 `api-ms-win-core-synch-l1-2-0.dll!WaitOnAddress`，原资源 callback
已执行 3,695,384 work 后真正阻塞。仅提高 callback 预算不能解决：外层 Process64
的 guest worker CPU contexts 此时暂停，主线程原生 IL 在同步等待原 Unity 的 job。

新增 `mono_scheduler` 以同步作用域借用现有 guest contexts；只在 main native
callback 等待时按有界 quantum 执行可运行的 C++ guest worker。执行使用唯一的
Memory/Runtime、该 worker 原 TEB/CPU/FPU 状态与现有 Win32 等待/唤醒实现，并把
work 计入 callback 上限；返回前恢复 main 身份及保存 worker 进度。没有第二个
宿主线程执行 guest 代码，也不把 WaitOnAddress 替换成成功或轮询内存变化。
目前 notification/loader、worker exit、其他 control 和 worker Mono gateway
仍明确拒绝，后者必须接实际 worker TLS 的 typed request，不能在主线程代执行。
真实指令测试证明 main 只在 worker 写入并 WakeByAddressSingle 后返回成功，记录
实际 worker thread ID；未支持的 worker Mono gateway 保持 CPU/上下文并恢复 main。

probe-139 使用该调度路径实际越过等待；初始化 UI 的 Resources.Load 返回原引擎
真实句柄 `0x6ffea0003510`。后续克隆进入 Canvas 层级通知与 Image 的
OnCanvasHierarchyChanged，24.15 秒后因缺
`UnityEngine.Behaviour::get_isActiveAndEnabled_Injected` 而抛出托管异常。
Mono 打印的“class libraries out of sync”是未注册 icall 的通用消息；这轮具体缺失
声明已经定位，不据该文字认定 runtime/corelib 指纹不匹配。

新增组件状态族同时核对 17 个 Behaviour/GameObject/Component 签名与原注册地址，
全部通过后才安装已编译 adapter：enabled/active 状态、SetActive、layer、component
index、gameObject/transform，以及 static 状态。输入只转发仍 live 的原 guest 引擎
数据地址；Boolean 使用原 Win64 ABI，handle getter 复用真实 v2 root、结果类型和
图同步检查。普通托管 OnCanvasHierarchyChanged 等继续由元数据解析，无类名白名单。
下一轮 probe-140 验证这组接口及后续 UI 克隆；可交互游戏验收仍未完成。

组件族与调度代码完整 release feature 回归 **525 passed / 7 ignored**，Clippy clean。
probe-140 通过整族安装检查，但本轮主循环在首次图形提交之前遇到独立 Win64
`imm32.dll!ImmGetCompositionStringW` 缺口，13.23 秒退出；没有执行成功的组件
状态 marker，不能报告 UI 克隆完成。不同实际调度/窗口事件可让该导入提前出现。

沿用现有“桌面没有 IME/HIMC”的真实能力状态，composition A/W 查询、设置、
conversion/open status、notify/font/window 等 API 成组支持无上下文的失败路径，
返回 0/FALSE 且不读写输入/输出 buffer，没有伪造 HIMC 或启用 composition。
只读核对 Wine `dlls/imm32/imm.c` 的 invalid IMC 分支（LGPL-2.1-or-later；独立 Rust
实现；未复制 C 代码），参考 SHA256 为
`c0511233694b0ec314b9124345a3fc4b230619c133f0d3f9bc6fe55a03be272a`。
新增回归检查 NULL/未签发上下文、不可读输入与 sentinel 输出不变。

同时把 worker immutable metadata leaf 的重复事件限为前 64 次和累计 query 的
二次幂，完整调用/metadata 统计与 API inventory 保持。probe-140 原先积累了
217,305 行启动输出（本轮目录 22 MB）；这项变更减少诊断存储和日志构建成本，
不以日志减少量宣称整款游戏性能倍数。

当前这批整体验证为 **526 passed / 7 ignored**，all-target Clippy、格式检查及
harness 4 项回归通过。协作等待回归还覆盖整个 worker TEB 被撤销时的错误返回，
确保 main 身份恢复且 worker context 没丢失。probe-141 使用这份冻结 executable。

probe-141 再次实际加载初始化 UI；`Behaviour::get_isActiveAndEnabled_Injected` 和
`Component::get_gameObject_Injected` 均执行并通过原引擎结果检查。随后停在
`GameObject::GetComponentsInternal_Injected` 缺失，尚未完成 UI 克隆或游戏验收。

组件查询这批以游戏自带 CoreModule 的真实 metadata/IL 为依据，成组核对数组查询、
List 查询、按 Type/名称查询、父子组件查询以及 count/index 共 8 个声明和注册地址。
System.Type 复用 RuntimeType root；List 必须来自冻结 corlib 的闭合 List<T> 声明，
经现有元数据字段/数组图发布实际 backing storage。发布前刷新已有 native 修改，
防止旧 guest 镜像反向覆盖 IL 写入；回调后执行字段、数组和 root 同步。
数组结果按真实 requested type 或 Component 的 SZ array 检查，NULL 仅在有 resultList
时准入。普通组件结果复用原 v2 handle 类型检查。安装和真实游戏验证分开记录。

probe-142 实际越过 `GetComponentsInternal_Injected`，将原生 IL 的 List<T> backing
字段和数组交给原引擎，结果回到 native Mono，继续到 Canvas 的像素比例查询。
23.55 秒后缺 `Canvas::get_referencePixelsPerUnit_Injected`，尚未完成 UI 克隆。
当前完整回归仍为 **526 passed / 7 ignored**，release build、Clippy 均通过。

下一批 Canvas / RectTransform / Transform 层级共 73 个固定接口，同样在全部
真实 signature 和 registration target 校验通过后注册。Canvas scalar、enum、
material/camera/rootCanvas getter，RectTransform 的 Rect/Vector2 读写和 parent/
child/sibling 操作均转发原 Unity。enum 核对真实 Int32 底层布局，浮点 setter
使用原 Win64 参数位置对应的 XMM register，返回读取 XMM0；Rect/Vector2 须有
完整 non-Boolean scalar 覆盖，按 4B alignment / 16B 或 8B width 检查，复制到
临时 guest 槽位，回调结束才写回 native byref。所有句柄结果仍经 canonical roots
与期望 managed class 检查；没有自行计算布局或替代原引擎状态。

这批完整 release feature 回归 **526 passed / 7 ignored**，build 和 all-target
Clippy clean；probe-143 实际执行 Canvas 的 referencePixelsPerUnit 与 Transform
GetParent，以及 Component 的 GetComponentsForListInternal。26.99 秒后在下一次
组件 List backing array 写屏障处遭遇类型不匹配，检查在写入前拒绝；继续诊断
实际 array class 与 List 字段的期望 class。未实际执行的成员仅算 ABI 安装。

probe-144 的精确诊断确认：字段期望 `UnityEngine.CanvasGroup[]`，原引擎辅助
函数提供的是 `UnityEngine.Component[]`。这不是 RuntimeType identity 丢失；
保留原字段 assignability 检查，禁止把不兼容的 backing array 写进 native List。

List 查询 adapter 现调用原 GameObject 的带类型数组查询，遍历顺序和层级/includeInactive/
reverse 仍由原引擎产生。Component 路径先通过已校验的原 get_gameObject 获取实际
所属 GameObject，再查询该对象；没有读取猜测的引擎 struct offset。结果按实际
requested SZ array class 核对，全体元素须兼容目标 List<T> 的 Add 参数类型，随后
调用其真实 metadata-qualified Clear/Add IL 并刷新 native container graph。所有
native allocation/invoke 均不持有 Memory/Metadata 的 Rust 借用，List 有临时强 root。
不再把 caller List 直接交给原引擎的不兼容 backing-array 操作。

新增回归覆盖末尾元素类型错误时旧 List 不变、NULL 与顺序保持、空查询清空列表、
Clear 失败时不执行 Add。完整回归 **527 passed / 7 ignored**，release build 与
all-target Clippy clean。probe-145 实测这个 typed list 投影及后续 UI 初始化。

probe-145 在执行前被旧 ordinary-method contract 拒绝：闭合 List<T> 的实例方法在
Mono metadata 中是 inflated method。新增独立 `describe_constructed_instance` 入口，
只接受闭合 constructed class 的非泛型实例 IL 方法；普通入口保持旧规则。image、
完整参数/返回、method flags、IL body/stack 上限和递归 concrete type 检查都保留。
新增回归验证它不准入静态入口、泛型方法或 non-inflated definition。

probe-146 实际越过 List<CanvasGroup> 投影，执行到其元素的 enabled 和 interactable
查询；后者缺 `CanvasGroup::get_interactable_Injected`。完整回归 **528 passed /
7 ignored**，build 与 Clippy clean。`Component[]` 写入 `CanvasGroup[]` 的类型限制
未放宽；成功来自原带类型数组查询与真实 List<T> IL。仍未到可交互界面。

CanvasGroup 八个状态接口和 CanvasRenderer 常用的颜色、材质、纹理、网格、裁剪
与深度共追加 41 个固定声明；UI 表合计 114 项，完整 target/signature 验证后安装。
Color 与 Rect/Vector2 一样要求完整标量布局；带 index 的 material setter 按原
三参数 ABI 转发。完整回归 **528 passed / 7 ignored**，build/Clippy 通过。

probe-147 实际执行 CanvasGroup 的 interactable/ignoreParentGroups，继续到
`Naninovel.ScriptableUIBehaviour::Awake`。25.07 秒后缺 Component 的
GetComponentFastPath_Injected。下一步核对原生 Mono 解释帧中 CastHelper<T> 的
精确引用输出槽位；不能将 IL 给出的任意 IntPtr 当作可写宿主地址。

GetComponent/TryGetComponent 的三个 FastPath 声明现按原 CoreModule metadata 与
registration target 成组安装。Win64 只收到私有 16B guest 缓冲区内部的 +8 地址，
原引擎产生的第一 word 按 MonoObject token 解码，经现有 root 同步及 Component/
requested class 双重 assignability 检查；第二 word canary 须保持不变。

第 24 个 frozen runtime patch 提供 checked interpreter reference-local API：仅当前
线程真实 LMF interpreter exit/parent frames，按 inflated method header 与实际
local_offsets 确认 exact CastHelper<T> local，核对 template image/identity、闭合 T、
16B size/8B alignment、两个非静态字段的名字、类型和 offset。执行原引擎之前先
验证，嵌套回调结束后重新验证，再调用 Mono 内部真实 GC reference barrier。
宿主输出指针不交给 Win64，也不以整个 native heap/stack 可写作为准入依据。

真实 native fixture 覆盖三种 reference T、assignable derived value、NULL、GC 后
引用保持，以及 NULL/misalignment/wrong field/native stack/wrong template/wrong T/
incompatible value/retired frame 的拒绝；所有拒绝保持 sentinel 和原引用不变。
普通与 Unity incremental=3 下全部 11 套 fixtures 通过，五种 noexec negative routes
均 exit 86。构建审计为 304 linked objects / 668 dependencies / 0 unmatched。
Rust 完整回归 **528 passed / 7 ignored**，all-target release Clippy clean，目录
复用/异常清理 harness 4 项通过。只在上述验证完成之后更新精确 runtime/support SHA：

- runtime：`813ba7548520c706de1de76ae8bd7b5b2f31cf1daadbf51e0d4f230bbe173c69`。
- support：`6bb5f274387eb907ecc97ddc963bc832d249adf52a452b58b1d417fa50424e0e`。

probe-148 实际通过 native CastHelper<T> 局部槽位验证，24.54 秒后在原 FastPath 的
guest `mono_gc_wbarrier_set_field` 停止：其 NULL-owner 输出在私有 guest 缓冲区，
不是 callback stack 或 engine heap allocation。新增同线程、按嵌套深度登记的
exact first-word reference output lease；仅这个 8B 地址及其声明 class 可接收真实
rooted/assignable reference。普通 byte payload、+8 tail、padding 和已退出作用域
没有此权限。正常返回和失败都先撤销 lease 再 unmap。新增回归覆盖两种退出路径。

probe-149 实际执行并完成 Component::GetComponentFastPath_Injected，继续越过
ScriptableUIBehaviour::Awake，到 UnityEngine.UI.Image::OnEnable；当前缺
Texture2D::get_whiteTexture_Injected。没有放宽 native local 或 guest reference
输出准入。仍未到可交互界面，full_game_backend=false。

纹理/Sprite 常用接口追加 47 项，UI 表现合计 161 项：六种 builtin texture、
尺寸/mipmap/读取状态、TextureDimension/采样 enum、wrap/filter/aniso/bias 状态，
Sprite packing、Rect/Bounds/UV/border/pivot、实际纹理引用等。仍整族核对 metadata
与 exact registration target；Bounds/Vector4 复用完整 scalar layout 检查，不发布
native texture pointers。完整 Rust 回归 **528 passed / 7 ignored**，build/Clippy 通过。

probe-150 实际执行 whiteTexture 和 Sprite.get_texture，继续到 Naninovel 的
CameraManager 构造和 PlayerLoop 查询。现有终点包括 RenderTexture.Internal_Create、
GraphicsFormatUtility/SystemInfo 的格式能力接口，以及 PlayerLoop 重入调用的
mono_type_get_object。后者须使用 prepare/execute/finish 的真实 RuntimeType 分配，
避免在分配及 GC 重入时持有 Rust registry/memory 借用。尚未完成初始化服务。

RenderTexture 创建及常用状态/生命周期追加 54 个固定入口，UI 表合计 215 项。
Internal_Create 只接收经声明 class 核对的真实 Mono receiver，经已有 canonical root
和 cached-pointer view 交给原引擎，回调后同步；没有自行分配 renderer handle。
新增 59 个纯 scalar GraphicsFormatUtility/SystemInfo 转换、分类及能力查询。
SystemInfo 同名同 arity 的旧托管重载导致 probe-151 的安装校验拒绝，现枚举声明
并按完整 scalar ABI 和 InternalCall flags 选择唯一匹配；不放宽签名要求。

mono_type_get_object 的嵌套分配拆为 prepare/execute/finish；execute 不持 Rust
registry/memory 借用，返回值先用临时强 root 固定；finish 检查 exact RuntimeType
class/实际 MonoType identity，再发布 readonly metadata token cell。现有同步 GC
profiler 回归增加这个路径，真实分配中能够执行原 guest callback。
完整回归 **528 passed / 7 ignored**，build、all-target Clippy、harness 均通过。

probe-152 实际完成 RenderTexture.Internal_Create、width/height、depthStencilFormat、
color format、mipmap count、sRGB 和 shadow sampling，以及原格式转换/能力查询。
27.58 秒后到初始化进度 Slider 的 Object.MarkDirty 与 PlayerLoop 数组引用写回。
后者的 RuntimeType token 已在 reflection registry，尚未接入通用 typed-array
reference root 路由。下一步统一其 canonical root 及真正的类型屏障检查。
当前 full_game_backend=false，未宣称标题或可交互界面成功。

probe-153 发现简单地把所有 reflection token 加入 ordinary roots 会让配置 prefab
加载回退：有些 RuntimeType 已先被普通对象图发布，不能再给同一 native identity
分配第二个 token。现统一所有 RuntimeType 返回入口；ordinary graph 先发现时复用
它的原 token，reflection 先发现时在 ordinary root service 启用前接入既有 roots。
已发布身份发生冲突仍拒绝，reference assignability、exact RuntimeType/MonoType
验证和 readonly metadata cell 均保留。新增两种发现顺序、冲突和退休后的回归。
同时按真实声明接入 Object.MarkDirty_Injected，修复进度 Slider 的缺失调用。
完整 release feature 回归 **531 passed / 7 ignored**，harness 4 项通过。
Release build 与 all-target Clippy 通过。probe-154 实际越过旧冲突与 MarkDirty，
完成 GetCurrentPlayerLoopInternal；28.64 秒后原 SetPlayerLoopInternal 在
`0x1800d5442` 读取 graph-first RuntimeType 的 +16 metadata cell 时未映射。
这次数组引用屏障已成功，剩下的是 setter 输入对象图的反射 payload publication。
此外进度 Slider 下一处缺 UISystemProfilerApi.AddMarker_Injected。
