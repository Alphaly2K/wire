# Metadata value payload、typed arrays 与对象图同步

更新：2026-10-12。有限 Unity 启动诊断，完整backend准入仍false。冻结23patch runtime
没有修改/重建；没有恢复boxed InputBinding类名/88B分支或逐类ctor名单。

## 值布局

`value_layout.rs` 根据实际metadata描述width1..256B、alignment1/2/4/8、最多64 slots。
支持primitives、与实际size/alignment相符的整数1/2/4/8B enum、reference slots，及同owning
image的scalar-only nested struct。nested depth<8、acyclic，必须完整覆盖bytes，不能含
Boolean、padding、refs；未知struct、generic complex、byref/raw pointer拒绝。

inline scalar slot记录真实alignment，不能用width代替。Vector3宽12B/alignment4可以作为
Bounds的两个nested slots；guest字段getter/setter仍针对正确的top-level field，不能用child
field访问parent。scalar_inline也采用同一完整coverage规则。

padding只发布owned zero，不复制native padding。guest非零padding、noncanonical Boolean、
bad/misaligned refs拒绝整batch。全部slot/element验证后才写native；refs使用真实Mono barrier。
CoreModule/render-core/URP/InputSystem/corlib/URP 2D共六个fingerprinted images按真实element
class image选择layout，不再将value arrays限制为InputSystem。

## Typed SZ arrays

reference与metadata value元素共用typed数组。长度≤4096、payload≤65504B、最多256 entries。
new后立即pin；借用已有native数组也先pin，再核对rank/class/length/element-size/data offset。
复用canonical instance identity；+16是readonly bounds/length，+32只包含declared scalar bytes
与guest tokens。native header/vtable/pointers保持私有。zero-length只发布header。

reference fields和value内ref slots只接受exact slot barrier；scalar/padding不能冒充reference。
String在转换slot之前先canonicalize，避免产生没有UTF16 view的另一份instance identity。
数组declared refs纳入bounded graph；typed layout、roots、GC及retirement检查均独立执行。

原生managed方法可以改已有数组。prepare_refresh先验证全部arrays无pending guest writes、
root/layout不变，再按declared slots读取并转换native值；padding仍zero，Boolean/refs仍受检。
字段与array两个完整plan都成功后才commit，late bad ref不能使前面的snapshot先被改写。
只统计实际变化的array refresh；probe-34计数0，表示此次游戏尚未在managed调用内修改已有
array payload，不能把单元回归当作该真实场景的验收。

## Boxed payload

metadata boxed-zero allocation使用真实mono_value_box并立即pin。六个image按actual class
image选layout，分配时安装serialization fields并自动生成owned连续payload；这使C++反序列化
可直接读写exact data，不需要先请求mono_object_unbox。

完整payload与已有field cells共享canonical backing/offset。coalescing前验证native snapshots、
pending writes、refs/padding；补齐NonSerialized payload字段，mapping失败恢复旧views，成功后
撤销旧backings。field/unbox连续访问保持同一身份，不泄露host地址。

probe-34实际有4 owned payloads，wrapper请求native-value-unbox-calls仍0。wrapper只接受live
boxed identities，核对native payload=object+16，给guest的是owned token+16；wrapper自身仍
没有真实游戏调用验收。

## Storage 与同步

普通CLR对象以Unity public/SerializeField/SerializeReference描述serialization fields。
冻结corlib的closed reference GENERICINST额外使用全部实际非static/literal storage fields，
包含private/NonSerialized/readonly backing。没有List字段名表；其他未知shape不因发现
reference就获得整块内存映射。UnityEngine.Object未完成admitted binding/ctor前不提前发布。

managed调用前guest→native同步，之后所有已发布field/array prepare、共同commit，覆盖child
List及数组。新native ref完成canonical转换后才遍历/发布。pending guest writes绝不被refresh
吞掉。field reference使用Mono setter，对象本身是reference C API参数，不能传&object。

## 证据

`wire-out/native-mono-unity-assets-probe-34/results.json`：exit0，52次后续ctor/5 interface
callbacks，1749 fields/1749真实GC checks；11 reference arrays/12 value arrays全部核对；
326 allocations、619 borrowed roots、436 strings、8 RuntimeTypes全部GC核对与退休通过。
4 box payloads，unbox wrapper请求0；guard233/0，original Mono未attach，full_game_backend=false。

最新managed边界为带Type参数与3个out String的APIUpdaterRuntimeHelpers static Boolean
方法，尚未执行。需要真实typed invoke/return/byref reference桥接，而不是补类名构造分支。
完整release feature467 passed/7 ignored，Clippy clean；覆盖late bad roots/padding/Boolean、
跨child owners与跨arrays的dirty batch拒绝、canonical string identity、泛型闭合/继承接口、
SerializeReference和临时GC恢复。日志同managed construction文档。
