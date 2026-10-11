# 原生 URP/渲染核心组件继续推进（2026-10-11）

在已通过的 ColorCurves 真正反序列化上，新增实际 VolumeComponent 构造和参数字段通路。
不是构造函数跳过/返回假值，也没有启用完整 native game Admission。

## 接入范围

冻结 URP/Core/CoreModule image 身份；明确登记该 URP 全部 17 种其他后处理组件的
无参实例 ctor token/class/name/signature，加上渲染核心 ProbeVolumesOptions。
ColorCurves 仍使用独立的实际 AnimationCurve resource adapter。
只在当前 rooted instance 的 owner scope 内运行 actual managed IL 和原始 guest
ScriptableObject.CreateScriptableObject；Mono metadata 重入仍只能使用该 this identity。

构造后只枚举该锁定类声明的 VolumeParameter references，再枚举已初始化的参数继承链。
拒绝未知 image、byref、非参数 root fields、未初始化/不同类的参数、非 NULL 的未知 nested
reference、过深继承和容量超限。先 pin reachable children，再发布精确字段 cells。

标量与 nullable references 使用前一阶段的 checked native stores；enum 限定三份锁定 image
中的真实四字节 I4/U4 enum。Vector2/Vector3/Vector4/Color 必须来自精确 CoreModule class，
逐个核对纯 F32 fields、offset、size/alignment。内嵌数据保持原始 bits，新增测试覆盖 negative
zero、NaN payload 和所有 lanes。未知 struct/reference-bearing struct 不能复制到 guest。

## 实际游戏结果

成功输出 `wire-out/native-mono-unity-components-probe-04`（实施 worktree）：

- exit 0，实际执行并反序列化 FilmGrain、ShadowsMidtonesHighlights、ChannelMixer、Bloom、
  SplitToning、DepthOfField、ProbeVolumesOptions，加上前一阶段 ColorCurves；
- 331 cells，75 native writes，其中 10 reference barriers；331 field GC checks；
- 81 borrowed pinned roots，与 9 次实际 native allocation 分开统计；全部 root GC checks通过；
- ColorCurves graph checks 3，实际 managed finalizers/guest destroys 各 8；
- 130 images、1,294,441 metadata queries、25,363 metadata handles；
- guard 232 checks / 0 denied；original Mono 未 attach；
- CPU user 8.520792s、system 0.317697s、wall 12.727191s。只代表这段有限诊断，不能当整款游戏速度。

登记的其余组件尚未在本轮真实游戏序列中出现，不声称已执行全部组件。
probe-02 的 Bloom enum 被错误拒绝，随后核对其来自同样冻结的 CoreModule并修复。
probe-03 完成六个组件，probe-04 接上 ProbeVolumesOptions 的 Vector3 后通过。
失败探测不能作为成功证据；各次 binary/command/inputs/results 保留。

完整 release feature **435 passed / 7 ignored**；feature all-target Clippy `-D warnings` clean。
完整日志 `wire-out/native-mono-unity-components-final-tests.log`。

## 下一请求

实际进入 UnityEngine.InputSystem.InputActionAsset::.ctor，method token `0x06000192`，
instance void()、NULL params。实际 body 是 ldarg.0 / call ScriptableObject::.ctor / ret，
下一步要核对 InputSystem image 并接这类 ScriptableObject asset，然后接其序列化的
strings/arrays/references。其 constructor 和字段生命周期本轮还未实现。
生产 finalizer owner queue、全 Process safepoints 和完整游戏后台仍是独立未完成工作。
