# InputActionAsset 原生构造与精确字段（2026-10-11）

冻结 InputSystem image SHA256
`79aeb3ee230428c0c53102aa8392a306312b0350e1c745be9125dcec160f6dd1`。
核对 InputActionAsset class/name/token 0x06000192、instance void() 与 rooted this，
实际运行 IL 02289800000a2a 和原始 ScriptableObject guest callback。

构造后只映射 m_ActionMaps / m_ControlSchemes 两个 SZARRAY reference cells 与
m_IsProjectWide Boolean；先核对 initial zero，再复用逐字段 declaration、布局、
写屏障和 GC 一致性检查。其复杂 nonserialized fields 没有映射。
Canonical empty arrays 可被 instance registry 以独立 pinned root 接受，保持原 token
身份，bounds/length 仍由 array registry 独立持有；不发布任何 native header。
非空数组未准入，assets 检查点在其分配前停止。

实际游戏成功输出（实施 worktree）`wire-out/native-mono-unity-assets-probe-01`：

- exit0；actual asset ctor1、component ctors7，加上 ColorCurves；
- 334 cells/75 native writes/10 reference barriers，334 GC checks；
- 10 actual allocations、81 borrowed roots，各自 GC 检查和退休完成；
- 585 empty arrays、601 external strong handles；真实 curve cleanup8；
- 130 images、1,294,508 metadata queries/25,378 handles；guard232/0；
- original Mono 未 attach，full_game_backend=false。

完整 release feature436 passed/7 ignored，all-target Clippy -D warnings clean。
日志 `wire-out/native-mono-unity-assets-tests.log`。
新增回归核对 canonical empty array 去重、拒绝错位 token/身份替换、pin 失败回收、
header 不可读以及退休后拒绝。

下一请求是 mono_object_get_virtual_method：刚分配的 InputActionMap token
0x6fff005a0000 与 method token 0x6ffe403d2000。继续受检虚方法解析和实际
serialization callback，再实现有引用元素的数组。有限检查点没有完成输入系统或
完整游戏执行；CPU user10.196632s/wall16.916520s 不是端到端性能对照。
