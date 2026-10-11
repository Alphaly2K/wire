# Metadata value payload、SZ arrays 与泛型 backing storage

日期：2026-10-11。实施仍是有限 Unity 启动诊断；完整后端准入为 false。
冻结的23patch runtime没有修改/重建。本轮删除 boxed InputBinding 类名/88B名单，
并继续沿 metadata/shape/interface contract 实际执行原始 IL。

## Value layout

`value_layout.rs` 描述 bounded value class 的实际 scalar/enum/reference slots：
width 1..256B，alignment 1/2/4/8，最多64个有效slot；枚举底层必须与实际 size/alignment一致。
包含 NonSerialized instance fields，static/literal不属于 payload。ref slots 要求 align8；
每个 offset/宽度须对齐且不重叠、不越界。nested struct、generic complex、byref、raw pointer
仍拒绝，绝不把未知 struct 当 plain bytes。

padding 是 owned zero bytes，不复制 native padding；guest 非零 padding 和非canonical Boolean
拒绝。同步先验证所有元素/slot/ref identity，再发生任何 native stores，防止后面的坏引用
让前面的 scalar/reference 更新先发生。reference 使用真实 Mono write barriers。

## Typed SZ arrays

`reference_arrays.rs` 同时处理 reference 和 InputSystem metadata-qualified value elements。
reference element 不再按 Map/Action 类名筛选，而按 frozen CoreModule/render-core/URP/
InputSystem/corlib image 与实际 reference class 筛选。nonempty长度1..4096，payload≤65504B，
最多256个entries。native new 后立即 pin；class/element-size/bounds/length/data offset精确核对。

guest +16 的 bounds/length 为 readonly owned header，+32 为 typed payload。只含 guest tokens
及 declared scalar bytes；native vtable/header/address不发布。`mono_gc_wbarrier_set_field`
owner=NULL 仅在 exact reference slot 接受；scalar/padding不能冒充 reference slot。
`mono_array_addr_with_size` 核对实际 width/index/生命周期。

已有 native SZ array 可从 rooted graph 借用：先 pin，再验证实际 rank/class/length/size，
逐 slot native→token；复用 instance registry 已有 canonical identity。empty backing array
仅发布 readonly header，禁止访问 data。强制 GC 核对、roots释放和views撤销与新数组一致。

`native-reference-array-writes` 统计所有 array reference barriers，包含 value elements里的refs；
reference/value array数量与GC checks分别统计，borrowed typed array另有独立计数。

## Boxed payload

boxed-zero allocation 使用实际 `mono_value_box`，立即 pin。`mono_object_unbox` 仅接受 live
allocated box，核对实际 native payload=object+16，但给guest的是owned token+16。
首次 unbox 将既有碎片字段cells合到一个canonical完整payload；后续 field与unbox view共享同一
backing和offset。验证全部native snapshots/pending writes/refs/padding后才换映射；映射失败恢复
旧views，成功后撤销旧backings。missing NonSerialized字段也纳入精确cell，GC后逐字段核对。

真实probe-17没有游戏unbox调用（两个unbox/payload计数均0），所以这里只验收单元覆盖的
coalescing、连续读写、ref隐私、late bad ref/padding拒绝、canonical一致性和退休；不把它算作
真实游戏unbox ABI已通过。

## Corlib generic reference storage

Unity对List backing字段有直接C++读写。仅对冻结corlib内resolved reference GENERICINST，
metadata描出全部非static/literal字段（包含private/NonSerialized/readonly storage）；复用同一
field转换、批量验证和barriers。任意其他class/不支持shape不会因此获得未知内存映射。
managed constructors/methods的准入规则仍独立，未开放任意泛型invoke。

从admitted receiver的已发布字段出发，受检发布bounded backing graph（最多256个不同对象）；
数组仍需自己的typed layout。没有List字段名/token表，没有把native对象整体memcpy给guest。
本轮真实验证VolumeProfile的List<VolumeComponent>、原生empty backing array和guest生成列表。
后续managed方法若修改已发布child/array，还需受检refresh，不能吞pending guest writes。

## 验证

`wire-out/native-mono-unity-assets-probe-17/results.json`：exit0，41次后续构造、2次interface
回调，990 fields/990 GC checks，5 reference arrays/8 value arrays及全部相关GC checks，
572 array reference barriers/32 scalar writes；91 allocations、588 borrowed roots、430 strings。
原ColorCurves资源仍真实释放，guard233/0，original Mono未attach，full_game_backend=false。

完整release feature454 passed/7 ignored、Clippy clean；定向54 passed。新回归覆盖bad ref在后
拒绝全batch、value alignment/overlap/padding、array header权限及退休、借用identity与ref转换、
泛型值类型拒绝、private storage descriptor、伪同名接口callback拒绝及virtual resolution。
下一真实managed entry为另一个image中的Renderer2DData::.ctor，尚未实际执行。
