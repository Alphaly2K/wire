# 元数据驱动的 Mono 构造与序列化桥接

更新：2026-10-12。后续构造不按类名或 method token 分派。冻结 ARM64 Mono interpreter
实际执行原始 IL；Rust 管理签名、rooted identity、typed engine boundaries 与数据同步。
当前仍是有限启动诊断，`full_game_backend=false`，没有正式窗口或可玩验收。

## 托管入口规则

`constructors.rs` 核对真实 Method、declaring class、signature 和 bounded IL body。
可用 image family 为按 SHA256 锁定的 CoreModule、render-core、URP、InputSystem、URP 2D。
构造须 instance `void()`、reference receiver，非 static/abstract/PInvoke/InternalCall/
native/runtime/synchronized；method 自身不能有泛型参数。receiver 实际 class 必须等于
构造的 declaring class。

class-only inflation 可以使用：冻结 `MonoGenericContext` 的 class_inst 非 NULL、method_inst
为 NULL；`closed_types.rs` 递归核对真实 type arguments，拒绝开放定义、VAR/MVAR、pointer、
byref、循环和超限。该 native context 不向 guest 发布，也不准入 method 泛型。

序列化方法按 `UnityEngine.ISerializationCallbackReceiver` 的实际 interface contract 准入。
receiver 必须实现接口，native virtual resolution 必须精确等于请求的 implementation。
允许继承实现：实际 SerializedDictionary`2 receiver 可调用 SerializedDictionary`4 base
实现，无须 class 名名单。伪同名方法拒绝。零参数 callback 的 guest 空参数向量可非 NULL，
但不读取该向量，也不传给 native invoke。

普通静态方法现按 managed_calls.rs 的 metadata/typed ABI 准入；普通实例方法和 value receiver 尚未准入。原有 owner-thread、strong roots、fixed
callbacks、P/Invoke rejection、异常拒绝和 no-exec guard 均保留。引擎对象被图遍历发现时，
不能因此提前发布其字段；仍要走对应构造/binding 及 engine lifetime 检查。

## 字段与图规则

`field_layouts.rs` 枚举 owned image 与 parent chain。Unity 字段依据 public、`[SerializeField]`
或 `[SerializeReference]` 选择，NonSerialized/static/literal/readonly 排除。属性检查只读
metadata，不运行属性构造。`[SerializeReference]` 修复了实际
RenderPipelineGraphicsSettingsCollection 私有 backing field 缺失。

支持 exact primitives、实际整数底层的 1/2/4/8B enum、String/class/Object/SZARRAY 引用、
reference GENERICINST，以及无 padding/Boolean/ref 的完整 scalar inline struct。
actual size/alignment、declaration、offset、overlap 均二次核对。含引用 nested struct、
unknown generic value、byref/raw pointer 继续拒绝；详见 value payload 文档。

分配时为 owned ordinary CLR 子对象及 boxes 发布 metadata 字段，C++ 反序列化不必等待
零参数 ctor。这处理了没有这种 ctor 的 ProbeVolumeSceneData。冻结 corlib 的 closed
reference generic backing 使用全部实际非static/literal storage fields，不维护 List 字段表。
bounded graph 最多256个对象；typed array 的 declared reference slots 也参与遍历。

字段和数组在借用 reference 前统一 canonicalize native String，随后发布 readonly UTF16
view。相同 native String 在字段、数组、instance registry 中保持同一 guest identity。
没有先发布 opaque identity、再偷偷替换字符串 token。

每次 admitted managed call 前同步 guest 写入；之后对全部已发布字段和 typed arrays
分别 prepare，完整验证 pending guest writes、native roots/layout/Boolean/ref 后，才提交任一
snapshot。原生修改的 child List `_size`/`_version` 及数组也覆盖。只提交变化，只有新引用
需要再遍历，不按 receiver 刷新局部图，不吞未同步写入。

## 扫描与临时 GC 区间

Unity 实际请求的 gc_disable/gc_enable/is_disabled 使用真实 BDWGC 实现，owner/thread
核对、最大32层、enable配对及状态检查。诊断边界可能位于暂停区间，因此强制 GC、验证、
root释放及cleanup前恢复全部尚存scope；GC验证必须真的收到完整profiler cycle。
这是保持 Unity 请求的临时语义，不是关闭 GC 以绕过生命周期或性能检查。

owner=NULL 写屏障可存 NULL/已强根token到C++临时槽：精确live HeapAlloc/owned buffer范围，
或Unity自身VirtualAlloc分配器的live private、committed、writable且无code history页。
排除freed slab slots、heap padding、共享backing、image/code/readonly/uncommitted页、
错位和overflow；guest destination/token绝不传入native barrier。

mono_unity_class_get 调用真实checked wrapper；核对 owned image、u32 TypeDef token和实际
row范围，再返回typed Class identity。mono_type_get_object 调用真实runtime，复用RuntimeType
strong root与readonly typed metadata view，核对返回type identity。

## 真实验证与当前终点

实施 worktree `wire-out/native-mono-unity-assets-probe-40`：exit0。

- 68次后续metadata ctor（component46/asset1/map2/action19）、5次interface callbacks；
- 368次实际 static IL invokes、1104个 canonical String byref outputs；
- 2193 fields/2193真实GC checks、425 allocations、1429 borrowed roots；
- 11 reference arrays/12 value arrays全部GC核对，872 strings/375 RuntimeTypes也全部核对；
- 372 owned box payloads，含真实invoke返回boxes；native unbox wrapper请求仍0；
- 14个真实 tagged weak handles，相关handle、roots/views生命周期通过；
- 游戏正常GC disable1/enable1，无诊断强制unwind，final depth0；guard237/0，original Mono未attach；
- ColorCurves实际engine graph/cleanup及其他GC/retirement验证通过。

原来pending的APIUpdaterRuntimeHelpers static Boolean已实际执行368次，不再是当前限制。
Unity类型缓存的element_class prefix、真实类型谓词、weak handles及isinst也已越过。
实现契约和证据见 `native-mono-static-invoke-2026-10-12.md`。

最新请求为 render-core 的 instance void `UnityEngine.Rendering.VolumeComponent.OnEnable`
（method token0x06000c82、receiver为已构造ColorCurves，零参数但参数向量非NULL）。
实际IL/签名在probe-40/startup.log，下一步使用相同typed ABI推广普通实例调用；
不添加OnEnable类名/token名单。冻结23patchruntime/support未修改。

完整release feature：472 passed/7 ignored；all-target Clippy `-D warnings` clean。
日志为 `wire-out/native-mono-static-calls-tests.log` 与 `native-mono-static-calls-clippy.log`。
probe有并发build/tests，耗时不作端到端性能对照；正式游戏窗口仍未验收。
