# 真实 Unity rooted string 阶段（2026-10-09）

证据 `wire-out/native-mono-unity-strings-probe-01`：真实 Unity 创建一个53B UTF8输入的
native System.String，完成其main-stack引用交接；强制native BDWGC后，v2 strong handle
仍保持相同对象及不可变UTF16 payload。guard **178 checks / 0 denied**，无original Mono attach。
82个image cycles、5643 metadata queries及10824个注册entry仍与原始capture一致。

分配、handle创建和GC验证均在同owner callback scope内；FFI不持有guest Memory借用。
所有有限诊断对象都由provider保活到cleanup，未完成production guest root扫描/safepoints。
只映射MonoString offset16的length及offset20的UTF16数据（含terminator），exact READ-only
owned snapshot；header、host pointer、尾部越界、WRITE/EXECUTE均fault。每个string使用独立
opaque token，不把guest address传入native GC barrier；barrier仅允许NULL或registry验证
strong-rooted identity写入当前main-stack受检slot。没有开放heap/interior byref。

native strong roots在domain cleanup前释放；owned views在cleanup后unmap/revoke，幂等退休。
回归覆盖forged token、wrong range/permissions、duplicate identity、retirement顺序和double-release。
provider targeted **11 tests passed**，feature all-target Clippy `-D warnings`通过。
此前metadata完整feature回归 **395 passed / 7 ignored**；该数字不包括本阶段新增测试。

首次真实pending invoke已解析为 **System.Type::GetType(System.String)**，锁定corelib
method token `0x060011d6`，static、非byref String参数、class返回，参数是已strong-rooted
string token。此阶段停在invoke之前，不声称它已执行；下一步接入其ReflectionType对象。
完整game Admission/native backend仍false。浮点、翻译缓存、Mono source/patch/dylib未修改。
