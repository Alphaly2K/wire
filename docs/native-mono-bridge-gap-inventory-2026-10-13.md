# Mono 桥接缺口全量盘点

现使用离线 Cecil 扫描与实际运行时 lookup 结合，而不是等游戏逐次报缺失。
盘点只读程序集 metadata/IL，不执行游戏方法、不生成 wrapper、不解析 PInvoke DLL。
原 Unity registration 表用于核对 guest targets；原生 lookup 用于确认实际安装状态。
每条记录以程序集、MethodDef token 标识，避免重载或同名程序集混淆。

probe-156 的初始结果：

| 项目 | 数量 |
| --- | ---: |
| 原始程序集 | 153 |
| InternalCall 声明 | 11,537 |
| PInvoke 声明 | 1,393 |
| 当前 loaded image snapshot | 131 |
| 原生 lookup resolved | 1,135 |
| 原生 lookup missing | 10,314 |
| 不在 loaded snapshot，状态未知 | 88 |
| missing 中纯 scalar 静态候选 | 1,193 |
| 原 Mono 具名导出 | 1,238 |

全量声明包含 Unity 的 Android、物理、编辑器辅助及游戏未使用模块，不能把全部
missing 当作当前启动必需 API。resolved 也不是语义或运行覆盖证明；其中包含
原生 Mono 自带的 corelib InternalCalls。PInvoke 一律只记声明，不为盘点加载插件。
普通 managed IL 入口的特殊 ABI、跨域/线程/GC 生命周期和动态反射调用需要另外验证。

复现时先用 `probe_startup.py --bridge-audit`；其余真实游戏参数与普通 assets probe
相同。audit 是显式诊断选项，普通运行不进行全量扫描；此轮 timing 不能用于性能对照。
`mono_lookup_internal_call` 对缺失声明打印的 out-of-sync 警告来自诊断查表，不能
当作这些方法真的被游戏调用。snapshot 保存 runtime lookup、engine registrations
及带 SHA256 的 owned image identities。

再运行：

```sh
python3 tools/mono/bridge_inventory.py \
  --managed wire-out/native-mono-probe-state/runtime/AliceToPapaADV_1-0-0_Data/Managed \
  --runtime-audit wire-out/native-mono-unity-assets-probe-156/bridge-audit \
  --sample-mono wire-out/native-mono-probe-state/runtime/MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll \
  --observed-api wire-out/native-mono-unity-assets-probe-155/api.tsv \
  --output wire-out/native-mono-bridge-inventory-2026-10-13
```

输出 `bridge-gaps.tsv` 的完整类型、primitive/enum/value/reference/pointer shape、
original targets、lookup 状态、静态 IL 引用次数；`families.tsv` 按类汇总；
`engine-bindings.tsv` 保留无声明对应的 registration；`mono-exports.tsv` 将原导出
与 source dispatch candidates、已观察调用分开记录。Source candidate 不代表
签名/线程/生命周期已支持，IL 引用次数也不是执行频率。所有 matched runtime
method identities 必须有与离线输入相同的 SHA256，重复 token 或 kind 改变拒绝。

先成批补齐 Application、SystemInfo、QualitySettings、Time、Screen、Input、
AudioSettings/AudioListener、Cursor、Graphics/GL 与 GraphicsSettings 的纯标量
接口。普通整数可能表示 GCHandle 或 allocator capability，所以不把所有 scalar
候选无条件自动接入。对象引用、异步/线程、缓冲区和句柄接口仍按语义族实现。
下一处实际游戏边界是 SetupCoroutine.InvokeMoveNext 的 Boolean 输出槽。

本次工具的 identity/重载/未知状态/ambiguous target 回归及既有 harness 共 7 项
通过，Rust **531 passed / 7 ignored**，release build 与 all-target Clippy 通过。
源码和扫描器进入版本控制；原始游戏 binary/程序集及全量结果只留在 wire-out。

## 通用桥接复测

固定标量 Spec 的 probe-158 曾新增 361 个 resolved。用户随后要求验证可推广机制，
因此未把这批逐方法 Spec 提交；实现改为预编译 ARM64 入口池与运行时元数据描述符。
实际引擎注册表提供地址，MethodDef/signature/enum/value-layout/ParamList 提供 ABI。
engine-self 由 UnityEngine.Object 继承关系、exact IntPtr 与真实 `_unity_self` 参数
共同识别。普通托管类不新增函数名单。

probe-160：自动新增 **3,391** 个注册，resolved **4,526**、missing **6,923**、
unknown **88**；原 11,537 个 icall/1,393 个 PInvoke 声明数量保持。实际执行
Quaternion 与 Transform 的数学/旋转接口，协程输出也成功。完整回归 535 passed /
7 ignored；guard denied=0。报告位于 `wire-out/native-mono-bridge-inventory-generic-2026-10-13`。

新增 `parameter_modes`、`parameter_names` 与 `abi-groups.tsv`，按数据表示汇总：

| ABI 类别 | 当前 missing |
| --- | ---: |
| scalar | 1,295 |
| value / byref | 863 |
| reference / string / array | 309 |
| pointer / capability | 4,456 |

pointer 类别包含引擎 self、输出句柄、span 内嵌指针和异步协议等不同语义；后续按
统一协议处理。PInvoke 保持独立声明清单。细节和验收边界见
`native-mono-generic-bridge-feasibility-2026-10-13.md`。

probe-163 在统一原生内存语义修正后完成 6 次 MemCmp 和 PhysicsEvents 的普通
值参数调用，进入 NativeInputSystem.NotifyUpdate 的输入事件 IntPtr 边界。
本批新增的三个 UnsafeUtility memory 注册没有重新计入上方 probe-160 snapshot
统计；实际运行和静态盘点分别保留原始采样编号。完整回归 539 passed / 7 ignored，
build、Clippy、8 项 inventory/harness 回归通过。

probe-164 至 probe-166 接入同一 invoke 下的输入快照、engine-object SZARRAY 参数
和 wrapper-derived v2 对象句柄返回，自动注册数 **3,391 → 3,438 → 3,501**。
真实执行已通过两次输入更新、GetAllCamerasImpl_Injected、targetTexture/targetDisplay；
下一实际缺口为 Display.RelativeMouseAtImpl。上方 missing 分组保留 probe-160 的
全量快照，未将后续新增注册混入旧统计。本批完整回归 541 passed / 7 ignored，
build、all-target Clippy 与 8 项 inventory/harness 回归通过。

常用 numeric/value 类族与 wrapper-derived 引擎指针参数使 probe-168 自动注册数
达到 **3,852**。Display、相机射线与 3D/2D Raycast 已实际执行，继续到资源字段中的
GENERICINST value 边界。完整回归 542 passed / 7 ignored，release build 和 all-target
Clippy 通过。全量静态 snapshot 仍使用上方明确编号的旧采样，未重算 missing 分组。

## 成批盘点对象与内存协议

扫描器现同时输出 actual TypeDef/闭合类型字段、委托 Invoke 签名、unsafe IL 使用点
和 native wrapper 调用/转换器。闭合 generic value 的字段先代入实际 T，再递归计算
shape；同一类型的 Module/AssemblyRef 归一到 resolved module 身份，避免重复计数。
展开有深度、文本长度与类型总数上限；unknown/open-generic 保持显式记录。
扫描仍只读 metadata/IL，不执行游戏方法。

报告目录：`wire-out/native-mono-protocol-inventory-2026-10-13`，复现沿用上方命令，
`--runtime-audit` 指向 probe-160；icall resolved/missing 数量仍来自该旧快照。
新增文件：

| 文件 | 用途 |
| --- | --- |
| managed-types.tsv | 类型、闭合实例、Serializable、layout 声明、对象图策略 |
| managed-fields.tsv | actual 字段类型、generic substitution、pointer/ref/delegate 形状 |
| managed-callbacks.tsv | 完整委托签名，按类型闭合参数 |
| native-wrappers.tsv | 原生边界调用点及同包装 IL 中的转换器 |
| unsafe-il.tsv | 指针字段/操作数、ldind/stind、cpblk/initblk、calli 与函数地址 |
| field-protocols.tsv | 按共用协议统计，区分 public 候选与 admitted graph 候选 |

本样本得到 **47,350** 个类型记录、**185,429** 个字段记录（含闭合实例）、
**3,583** 个委托签名、**20,428** 个 native wrapper 调用点和 **71,179** 个 unsafe IL
使用点。类型策略中 32,464 为 native-only，9,274 为 serializable-data，1,329 为
engine-object，3,584 为 opaque-delegate，699 unresolved。布局 offset/size 来自
metadata 声明；运行时 ABI 仍由实际 Mono 布局验证，不拿离线 offset 直接执行。

这次找到的是共性过度投影：`InputActionState` 本身不可序列化，公开的
`UnmanagedMemory.memory/basePtr` 不应因 public 标志进入资源对象图。它与委托内部
状态留在原生 Mono，以 canonical GC root 身份跨边界。顶层和嵌套引用图均采用
相同规则；明确的 embedding field 操作继续要求 typed view，裸指针仍拒绝。
实际 Serializable reference 和显式容器保留原字段/数组同步与写屏障。

批量实施单位改为布局、引用/屏障、owned memory/句柄、callback/thread 四类协议。
同类缺口在离线清单里一起审查和实现，然后做完整回归与游戏验收。动态创建的类型、
原生插件和实际线程/lifetime 仍在运行时核对，静态清单不把这些行为自动判成已支持。
字段协议的 pointer graph 候选为 17 个闭合记录（9 个独立字段声明），callback graph
候选为 27 个独立声明；不把所有 native-only 指针字段都当成跨架构工作量。

本批完整 release feature 回归 **543 passed / 7 ignored**，release build、all-target
Clippy（`-D warnings`）和 **11 项** inventory/harness 回归通过。Cecil fixture 实际
验证闭合嵌套 generic/数组、委托签名、NonSerialized、native-only 策略和身份去重，
fixture 的方法不执行。

随后同批接入 actual delegate Invoke 的运行时方法准入和按需 embedding field view，
整组委托复用普通 invoke 参数/GC/异常协议；scalar/enum/value/reference 字段按实际
field identity 描述，其他字段不随显式访问展开。字段返回引用前，数组与已准入子图
先完成投影。原生委托 fixture 覆盖闭包、多播顺序、GC 和异常，guard denied=0。

这批后续最终完整回归为 **545 passed / 7 ignored**。probe-171 已真实通过 Runtime
delegate Invoke，继续至 Canvas；新增实际缺口属于直接引擎对象、异常引用与异步
operation 协议，而不是更多托管构造函数白名单。

随后整组接入直接引擎引用、opaque Exception，以及 AsyncOperation / instantiate
生命周期（13 个配套接口和创建入口），共用实际 registry target 和 metadata 签名。
probe-172 实际完成 async 创建/绑定与 Mesh 创建；退出 operation created=1 /
released=1 / live=0，guard denied=0，GC scope depth=0。仍有字符串 span、Mesh 数组
上传和 BuiltinUpdate 缺口，最后帧为加载页；不能将此阶段记为完整游戏验收。

后续批次按这些共用形状扩展：16 参数跨 ARM64/Win64 ABI、actual scalar array
protocol、wrapper-derived UTF16 输入与混合 byref payload。此处原 lookup missing
统计仍来自 probe-160 快照，不将新的安装数或实际调用结果混入旧快照。异步清理
顺序、来源追踪规则与每轮实际验收详见 `native-mono-generic-bridge-feasibility-2026-10-13.md`。

## 最新全量 snapshot 与显式数值布局

probe-174 对全部已加载程序集重新进行 lookup；同一报告目录已刷新为该 snapshot，
上文 probe-160 的历史数字仍保留对应编号。最新 **resolved=5,139 / missing=6,310 /
unknown=88**，153 个程序集、11,537 个 icall、1,393 个 PInvoke 和静态类型/字段
总数不变。`summary.json` 的 input hashes 指向 probe-174；observed API 使用正常
收尾的 probe-172，避免从 abort 的采样取得未落盘 trace。

本次 scalar/value 与字符串/数组协议最终完整回归为 **550 passed / 7 ignored**。
probe-173 实际通过 SetName 与 SetArrayForChannelImpl，随后拒绝重叠值字段；probe-174
通过显式数值 union 后到达原型数组数量上限。两次 returncode=-6，不能使用缺失的
最终 guard/GPU 统计；最后帧仍为加载页。

显式 union 以 actual TypeAttributes.ExplicitLayout 和全部实际字段验证为准：每个
字段必须为有界、对齐、非 Boolean 数值，所有 payload 字节均被覆盖。通过后折叠为
一个完整 inline payload，避免将整数视图和字节视图重复同步。重叠引用、指针、
Boolean、padding、普通非显式布局继续拒绝。静态清单中有 48 个全数值 explicit
候选；实际尺寸、对齐、覆盖与 closed-type 检查仍在 runtime 进行，无 Color32 特例。

typed array registry 的旧 256 个原型上限改为 4,096 个身份和 64MiB 累计 payload
双预算；canonical token 范围与对象根服务使用同一个规则，数组 arena 与后续 arena
不交叠。按实际 rank/stride/layout 检查与保守 reference traversal bound 保持。

这批最终 **551 passed / 7 ignored**，release build、all-target Clippy 与 **16 项**
metadata/harness/build-tool 回归通过。probe-175 自动注册 4,020 个入口，275 个
typed arrays 正常验收，Mesh 上传与 BuiltinUpdate 已实际执行；guard denied=0、
async live=0、GC depth=0。新的整组协议边界是 render-context / command-buffer
生命周期，以及带实际 null guards 的对象 marshal 包装；当前游戏仍为加载页。
