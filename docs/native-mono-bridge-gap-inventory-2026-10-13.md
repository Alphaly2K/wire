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
