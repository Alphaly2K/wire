# 真实 Unity ColorCurves 构造函数（2026-10-11）

原生 Mono 的实际游戏初始化现已完成 `ColorCurves::.ctor` 的有限检查点。
实现位于 `src/x64/process/mono_startup/animation.rs`，包含四个固定 ARM64
InternalCall adapters、同 owner 的受检 Win64 重入、真实托管对象图验证和退出清理。
完整游戏 backend/Admission 仍为 false；M3 尚未完成。

## 构造函数和 ABI

仅准入已核对 SHA 的 CoreModule、RenderPipelines.Core.Runtime 和
RenderPipelines.Universal.Runtime。最后者的 method token `0x06000541` 必须对应
真实 instance `ColorCurves::.ctor`，返回 void、无参数，this 必须是已经强 root 的
精确对象身份。构造函数与其调用的 Keyframe、Vector2、TextureCurve、
TextureCurveParameter、VolumeComponent/ScriptableObject IL 全部实际执行，不跳过 ctor。

固定回调对应原始 UnityPlayer 的登记目标：

| InternalCall | Win64 目标 | 适配 |
| --- | --- | --- |
| AnimationCurve.Internal_Create_Injected | `0x18006c410` | ref span；返回真实 guest curve pointer |
| AnimationCurve.Internal_Destroy | `0x18006c2a0` | 原始 guest 资源释放 |
| ScriptableObject.CreateScriptableObject | `0x1800c1e30` | 仅当前强 root 的 native this 转为 owned guest token |
| Object.GetOffsetOfInstanceIDInCPlusPlusObject | `0x1800c3690` | 实际执行 guest int32()，核对结果为 8 |

安装前核对方法 token、static/instance、InternalCall 标志、参数和返回类型。
Keyframe 由 native `mono_class_value_size` 和七个字段核对为 **28B / align 4**，
不是 32B。Span 核对为 16B / align 8，begin=0、length=8（值类型 field offset
包含 16B object header）。IL wrapper 保持数组 pinned；只向 guest 提供 owned
只读数据副本，不暴露 native array/header/pointer。成功与错误都撤销临时映射，
返回值不得指向临时 payload、callback stack 或可执行区域。

ScriptableObject 的重入仅允许 NULL 或当前已经强 root 的 exact this token
存入 active callback-stack slot；object_get_class 仅允许此实例，沿用既有 class
身份。没有扩大成任意对象或跨线程重入。

## 对象图、GC 和清理

验证 ColorCurves 的八个字段分别连接 TextureCurveParameter → TextureCurve →
AnimationCurve。前四条曲线 length=2，后四条 length=0；前四条 Keyframe 的全部
原始位与预期一致，zeroValue/range/loop、active 和 native cleanup 标志也受检。
每个 AnimationCurve.m_Ptr 都对应一个真实 guest 创建返回值，对象图身份在强制
BDWGC 后保持。

这是有限诊断，不代替生产环境的 foreign finalizer/owner queue。退出仍保留 parent
强 root，先对八个真实对象执行 managed GC.SuppressFinalize，再在当前 owner 调用
其真实 managed Finalize body；每个 body 必须进入原始 guest Internal_Destroy
一次，才允许释放根和关闭 VM。没有对仍存活资源的 foreign finalizer 假报成功；
关闭之后禁止 guest resume。

## 实测与范围

独立 fresh 游戏副本 `wire-out/native-mono-unity-curves-probe-04` exit 0：

- 完成真实 ctor、8 次 guest 创建、1 次 ScriptableObject callback；
- 对象图验证 3 次，包含强制 GC 后验证与清理前验证；8 次真实 managed finalizer /
  guest Destroy 通过；
- 1,204,046 metadata queries、25,096 handles、130 images、593 external strong
  handles；既有 8 个 top-level managed 初始化步骤和所有旧 roots/arrays/fields 验证通过；
- noexec guard 231 checks / 0 denied，原始 Mono VM 未 attach；
- CPU 5.216115 秒，wall 8.321201 秒，仅为有限诊断；没有整款游戏性能对照结论。

冻结 wire SHA256：`5bd70995c06a9da0a7e0a178d1ab7545d5ef05cca78e9a813048990946bb8b80`。
继续使用 23-patch runtime 和 corelib support，SHA 与 objects 阶段相同，未改原游戏、
SDK、runtime 或图形库。

31 项 startup targeted 回归、feature all-target Clippy，以及完整 release feature
回归 **431 passed / 7 ignored** 通过；完整输出保存为 probe-04/release-tests.log。
新增回归覆盖
owned this 的重入、只读 Keyframe payload、NaN 原始位、临时 payload/stack 逃逸、
映射撤销和元数据字符串向量扫描。

## 继续运行发现的下一边界

probe-01 缺 Object 静态初始化 InternalCall；probe-02 在 Unity strstr 的 16B
读取越过 exact-length metadata name view 时失败。现仅对 owned 只读名称加最多
一条向量所需的零填充，读不到邻接 host/metadata 数据，其他 shared views 的边界
检查不放宽。

probe-03 已进入字段反序列化，在 UnityPlayer `0x1807b07ae` 向 owned object
`+0x30` 写 1B 时触发未映射检查；这些失败探测不作为通过证据。当前 `curves`
有限检查点在 ctor 完成及 GC 验证后结束，尚不支持任意 initialized field data。
下一工作是按实际字段类型/所有权建立标量与引用视图，并把写入同步到真实托管字段、
保持 alias 和 GC write barrier；不能映射假 object header 或泄露 host reference。

之后仍需推进其他 Unity InternalCalls、异步/线程/委托、Naninovel 标题/输入/对话/
存读档，并进行端到端性能和长跑验收。
