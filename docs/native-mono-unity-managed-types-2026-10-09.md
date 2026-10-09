# 真实 Unity managed GetType / reflection 阶段（2026-10-09）

`wire-out/native-mono-unity-managed-types-probe-03`实际执行第一次
**System.Type.GetType(System.String)**，原生Mono返回System.RuntimeType，经v2 strong root
保持到有限诊断cleanup。Unity继续完成 **17,546 metadata queries / 1,926 typed handles**。
强制native BDWGC后，String及RuntimeType身份、string payload及MonoType关联仍相同。
guard **176 checks / 0 denied**；没有执行original Mono attach。

调用前核对锁定corelib method token0x060011d6、class身份、GetType名称、static/non-generic/
non-inflated、一个非byref String参数和System.Type返回。guest参数array复制为native-owned
reference array；exception输出使用native local，guest slot先检查权限。没有guest pointer
传入native Mono。未准入异常对象时明确拒绝；不伪造exception=NULL继续执行失败调用。

RuntimeType只映射offset16的8B **typed metadata token**，READ-only。此token不是native
MonoType地址；native object header、vtable、host pointer和相邻字段均不可访问。对象和metadata
保留独立身份及生命周期；NULL结果仍NULL。roots在domain cleanup前释放，views在cleanup
之后unmap/revoke。forged identity、字段边界/权限、alias identity、退休顺序及幂等释放有测试。

下一次真实invoke为UnityEngine.ClassLibraryInitializer.InitAssemblyRedirections，CoreModule
method token0x060022ff，static void()。已抓取owned IL及依赖：AppDomain.CurrentDomain、
ResolveEventHandler构造、add_AssemblyResolve，以及CoreModule中的managed lambda。
当前阶段停在它之前。原生IL disassembly只读取元数据，不执行pending method/cctor。

provider targeted **12 passed**，feature all-target Clippy -D warnings通过。完整feature回归
此前metadata版为395 passed/7 ignored，后续增量完整回归应独立记录。
完整game Admission仍false；这不是正式游戏原生运行或完整性能对照。
下一步执行受检classlib初始化，然后接后续Unity托管初始化/真实InternalCall。
浮点、翻译缓存、native Mono源/patch/dylib未改变。
