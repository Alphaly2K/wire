# 真实 Unity typed metadata 阶段（2026-10-09）

在82个真实native image/assembly循环之后，Unity已完成 **5,643** 次metadata查询，建立
**955** 个typed句柄和 **687** 个owned名称。证据
`wire-out/native-mono-unity-metadata-probe-05`，guard **176 checks / 0 denied**。

包含class-from-name、method/nested-type迭代、method name、generic/inflated判断、signature、
param/return type及type equality，并接入真实root corlib借用。Class/Method/Signature/Type
分型token不能互换；native NULL保持guest NULL。iterator parent/kind必须匹配，native
iterator指针只保留在registry，不写入guest。名称复制到exact shared READ-only region。

Class只映射offset232的 **8B userdata scalar shadow**，initial native userdata必须NULL；
相邻字节、header、未知字段及EXECUTE都拒绝访问。guest cache写入保存于owned cell，**不会
写入native MonoClass userdata**；本阶段只准入与此字段无关的metadata查询。未来依赖
embedding userdata的managed callback需另行连接协议，不能把这个shadow当作完整class ABI。
所有其余metadata句柄unmapped。没有把native metadata/vtable或机器码地址公开给guest。

Shadow/name映射保持到native cleanup之后，再unmap/revoke；native metadata由原生image/
domain生命周期保留。NULL栈root初始化现在可用，仍只接受有效main-stack位置、NULL或
已strong-root的main Thread token。当前domain查询逐次核对native root domain身份。

真实推进已止于 `mono_string_new_len`：root token、owned UTF8源53B。没有提前执行未知
string/object ABI或managed runtime-invoke。完整game Admission仍false，非完整游戏性能数据。
继续native string强根、受检字段及stack-reference交接；原生Mono worker queue尚未接入。
浮点、SSE、翻译缓存、native Mono source/patch/dylib 均未修改。

CLI边界新增metadata。已有images默认仍止于第一个class query之前。
最新metadata targeted 10 tests及feature all-target Clippy（-D warnings）已通过，包含
NULL/domain增量。完整feature回归继续进行；上一阶段default365/feature394各7 ignored
已通过，不将上一阶段结果充当后续对象实现验证。
