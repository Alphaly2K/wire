# Unity 普通对象受检分配（2026-10-09）

本阶段新增实际代码：`mono_startup/instances.rs`、patch 0023、provider 分配/句柄/GC
接入与真实 native fixture。有限 `objects` 边界已实测，完整游戏 backend/Admission 仍 false。

## 原生分配与所有权

`wire_mono_object_new_rooted_checked` 在 Mono GC unsafe 区间内调用真实 checked allocation，
在返回桥接层之前创建 strong pinned v2 root；仅返回已拥有的 root 与 uint16 status。
MonoError private storage 在 C runtime 内 cleanup。NULL domain/class/status、abstract、
value 与 array class 拒绝。分配不运行 instance constructor。guest 获得独立 opaque
object token，无 object header、vtable、native pointer 或托管引用 alias。

Registry 受 owner/domain/class/capacity 约束，class 身份和每次 root target 受检；外部
strong handle 和 main-stack reference 存储均认可该新身份。退出先释放外部/内部 roots，
再撤销视图；forged、retired、错误 class 与 root 改变均失败。

## Unity m_CachedPtr 的精确标量视图

原始 UnityPlayer SHA 不变。RVA `0x78392c` 的 `48 89 78 10` 将绑定的 guest C++
对象地址写入 Mono wrapper +16；首次实测在此 fault。锁定 CoreModule 的
UnityEngine.Object::m_CachedPtr 为非 static/byref 的 native int，native offset 16。
对该 base 的实例只映射 +16 的 owned 8B READ/WRITE cell，相邻 header/field 均 fault。

强制 GC 前把 shadow 同步至真实字段。先验证所有 root 和 scalar 地址（NULL 或
8B 对齐且已映射的 guest data），再写 host 字段；guest 指针仅作为 native int 数据，
不会作为宿主可解引用地址使用。GC 后检查 native 字段与 owned shadow 相同。
影像由 Mono checked type/assignability 确认，不靠给整个对象映射假布局。

## 独立 runtime 与测试

23-patch runtime：`wire-out/native-mono-objects-runtime`，完整 fresh build（不复用旧
object 文件）；304 linked objects / 668 compiler dependencies / 0 unmatched。

- runtime SHA256：`c152197b7cf1623a3d40a51a36e1e44d888b77f337b1378bb979598d2fad2eb0`。
- corelib support SHA256：`34705bafc092d1e644b2c7e0873340b398d1a81dd3e51c31af37a9ba0a29cea9`。

十套 fixtures 的普通/Unity incremental=3 两配置全通过；五个 executable-memory
negative routes 均 exit 86。objects fixture 验证未运行 ctor 的 zero fields、真实
instance ctor、scalar/reference 跨 GC 保活和无效类型；object root 由 C 分配时建立。
原 runtime/SDK/game 未修改。patch-stack 工具 5 tests 通过。

28 项 startup targeted tests 通过；feature all-target Clippy clean；完整 release
feature Rust 回归 **428 passed / 7 ignored**。标量回归覆盖精确 bounds、guest 写入、
未同步检测、mapped pointer 校验、失效地址失败不改 native 字段及 release/revoke。

## 实际游戏证据

`wire-out/native-mono-unity-objects-probe-04` exit 0，guard231/0，original Mono
未 attach。完成实际 ColorCurves 分配、m_CachedPtr 绑定、593 external handles；
1,204,046 metadata queries、130 image cycles、967 attributes / 2,821 field reads /
7 arrays，先前的 8 actual managed startup steps 仍完成。新对象身份/标量、所有旧
roots/arrays/fields 均强制 GC 验证成功，roots 与 views cleanup 通过。

下一个真实 invoke 是 `UnityEngine.Rendering.Universal.ColorCurves::.ctor`：
method-token `0x06000541`，instance void()，this 为新对象 token，params=NULL。
其 owned IL 已解析：Keyframe、Vector2、TextureCurve、TextureCurveParameter、
VolumeComponent ctor。TextureCurve 再创建 AnimationCurve，真实 injected span adapter
及 ScriptableObject callback 需接入，不能跳过或把 ctor 假装成功。

probe-01 缺 cached scalar cell；probe-02 使用了错误的 page unmap 而 cleanup 失败；
二者均不作为成功证据。共享视图现用 unmap_shared_data/revoke，且 root 先释放。
probe-03 成功停于 mono_thread_current；04 在同 main thread 身份受检后推进至真实 ctor。
