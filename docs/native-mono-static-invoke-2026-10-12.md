# 元数据驱动的静态 IL 调用与类型缓存桥接

更新：2026-10-12。目标是执行真实托管方法，避免维护类名/构造函数名单。
`managed_calls.rs` 按 metadata 验证 static method 和 typed ABI，冻结 ARM64 Mono interpreter
执行原始程序集 IL。没有生成 JIT、动态 trampoline 或修改游戏/冻结 runtime。
当前仍是有限启动诊断，完整 backend 准入为 false，尚无正式窗口和端到端性能验收。

## 入口与参数

入口必须位于 fingerprinted constructor image family，static、非泛型/非 inflated，
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
