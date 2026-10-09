# 真实 Unity Mono 启动观测（2026-10-09）

本轮取得 AliceToPapaADV 实际启动的 Mono 入口参数、Unity 回调输入及 InternalCall 注册目标。
这些证据用于接入 M3，游戏仍完整使用原始 x64 Mono；宿主 Mono 后端及 M3 准入保持关闭。
没有真实游戏原生 Mono 加速数据。诊断运行的 wall time 不参与性能对照。

## 观测实现

`wire probe64 ... --mono-trace PATH` 显式开启，默认关闭。Process64 在 Mono DLL 初始/动态加载
时登记真实 PE 导出 PC，保留同地址的所有名称。在导出入口保存当前 thread/work/retired、
RCX/RDX/R8/R9/RSP、四个 XMM 的原始字节、返回地址及八个 Win64 stack 槽。
这些槽位不是从签名推断出的参数，不依据它们自动准入 ABI。

追踪必须排除覆盖入口的 IR 批处理段，并在下一个入口重新进入外层循环；不能仅在每个
25M budget slice 采样 PC。关闭时仍走原来的紧循环，不做每条指令的追踪查表。

`Memory::observe` 检查 READ/完整范围/guard/revocation，不消耗 guard、不变更 guest 权限、
不部分改写输出。shared 读取持 backing 的执行 lease，不把 native pointer 直接交给调用者。
owned/shared guard 页在观测失败后仍由真实访问触发一次原来的故障。

观察规模有界：最多 8192 个索引 PC、8192 份原始样本；优先全局前 1024 次入口及每 PC 的
前 8 次。计数继续完整累计。InternalCall 单独保留最多 32768 个 name/target pair，每名称
最多 8 个不同 target，名称最多 1024B；失败/超额有计数，不能把遗漏当未调用。

从实际 version=1 allocator 表和明确的 plugin/log/vprintf installer 参数登记 guest 回调 PC。
这只是观察地址，不向 ARM64 传这些 code pointers。UnityTLS 是 opaque 表，128B snapshot
不是完整布局，更不能凭其中的 qword 猜测 callback ABI。旧/新 allocator 名字保持不同。

## 真实运行结果

隔离 APFS 副本位于 worktree `wire-out/native-mono-unity-callback-trace/runtime`，没有改游戏
原始目录、SDK 或既有运行副本。输入 Mono SHA256：
`86a5a4de6bcd74a637d63c96d822dc989d4ffaed6ef1c15c0e6e1f528ae6708f`。

2,000,000,000 work 的诊断终点是 Budget，不是游戏已完成初始化/可玩成功：

- 原始路径可见窗口 1024×720，290 次 GPU draw、97 次 Present；
- 246 个入口 PC 活跃，6,249,665 次入口观察，2,408 份原始样本；
- 采样返回地址落在 UnityPlayer 的不同入口名称 113 个；不是全部外部调用总数；
- `mono_add_internal_call` 实际登记 10,824 个名称/10,824 个 guest 目标，采集失败 0；
- 嵌套类型 `/` 与候选 metadata 的 `.` 名称表示统一后，全部 10,824 项各有一个声明候选；
- 关联仍需核对签名、ref/out、Injected、hidden 参数及对象所有权，typed game gateway=0；
- plugin callback 入口 1 次，四个 allocator callback 入口均为 0；没有漏过入口的段执行；
- unindexed_callbacks=0，样本之外的入口仍计数；omit 只影响保存原始现场。

首次外部启动序列已确认：allocator installer → plugin callback → class userdata offset →
set_dirs → null-separated assembly paths → signal/crash chaining → defaults/options/profiler →
main args/data dir → root domain init。`mono_jit_init_version` 的实参是 `Unity Root Domain` 和
`v4.0.30319`。稍后装 UnityTLS、注册 InternalCall、打开程序集和查询 classes/methods。

`tools/mono/entry_trace_report.py` 使用原始导出 RVA 与观察 PC 一致性确定 Mono 映射：
`0x182940000`，size `0xa42000`；UnityPlayer 映射从 startup log 读取，不靠地址大小猜测。
返回地址筛选能识别采样的 Unity 边界，但尾调用可能产生多个入口，不能当作 CALL 次数。
大量 Mono 内部入口、fallthrough 和尾转移都计入 raw hits；这些数字不能当 bridge 频率或 CPU
占比。InternalCall registration 不证明执行了对应函数。

## 纠正 allocator 接入假设

候选源码的 `mono_unity_install_memory_callbacks(MonoAllocatorVTable*)` 会调用 setter，因此
前轮曾把 Unity allocator 所有权桥当作本样本的首个启动依赖。本轮真实二进制证据推翻了
这个假设：

- 原始 installer RVA `0x270ba0` 的入口为 `c2 00 00`，立即 `ret 0`，不读取/保存表；
- 原始 `mono_set_allocator_vtable` RVA `0x74870` 只读取 version、返回是否为 1，不安装 callbacks；
- 整段启动四个回调零命中与该实现一致，不是仅凭零计数猜它们不重要；
- 当前 native candidate config 明确 undef `ENABLE_OVERRIDABLE_ALLOCATORS`；其 eglib 的
  `g_mem_set_vtable` 同样为空。候选 Unity installer 仍会读取 version，不能宣称完全同原 DLL。

这只适用于上述精确 sample fingerprint。未来该 profile 的 Unity installer gateway 应保持
原始立即返回行为，不把 guest 表指针传 ARM64、不额外解引用无效参数；这是匹配已确认的
原始语义，不是假成功。一般 managed/native memory、GC heap ownership、engine unsafe
buffers 仍需处理。本结论没有把其他 Unity 版本的真实 allocator callbacks 一概忽略。

原始字节证据在 `allocator-installer-disassembly.txt`，报告保存 hash/RVA/entry bytes。
不能只根据候选 C 声明决定样本的 allocator 行为。

## 下一阶段的实际依赖

按真实边界顺序推进单 VM provider、根域初始化、程序集/image/metadata 生命周期和 Unity
bindings。首次 installer 的空语义已知，plugin/UnityTLS/profiler/liveness 等回调仍需受检
封送。一般对象/reference/arrays/helpers、全 Process roots/safepoints、稳定 native TLS/guest
线程 owner、raw stores/barriers 和生产 services/scheduler 仍是主要工作。

不启用两个 VM、不逐函数回退、不永久 root、不直接开放整个 native Mono heap、不把查询或
声明扫描当 ABI 准入。基础原型成熟与真实游戏已切换是两个不同的验收事实。

## 验证及复现

本轮新增回归验证 owned/shared guard、不可读/越界/撤销、失败输出原子性、入口别名、逐预算
CPU 一致、IR 段内部入口、动态模块、取指故障、动态 callback PC、采样容量与 binding 容量。
完整 M2 重新执行成功：本次 noexec guard `checks=538 denied=0`（checks 会随 GC 时序变化）。
M3 preflight 仍 original_x64，293 queries/289 native symbols/21 leaf query/0 game gateway。
默认路径全量 Rust 365 项通过、7 ignored；native-mono-backend feature 全量 385 项通过、
7 ignored；all-target feature Clippy -D warnings clean。结果保留在本轮日志。

诊断命令及 binary hash 在 `command.json`，该执行器另冻结为 `wire-out/native-mono-unity-callback-trace/wire`。
二进制冻结后增加了每名称 target 容量限制；本次各名称均一个 target，没有触及限制，后续
容量行为由回归覆盖。不要用它与新二进制做 wall 时间对照。

```sh
python3 tools/mono/entry_trace_report.py \
  --entries wire-out/native-mono-unity-callback-trace/entries.tsv \
  --startup-log wire-out/native-mono-unity-callback-trace/startup.log \
  --sample-mono wire-out/native-mono-unity-callback-trace/runtime/MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll \
  --unity-player wire-out/native-mono-unity-callback-trace/runtime/UnityPlayer.dll \
  --managed-contracts wire-out/native-mono-m2-services/managed-native-contracts.tsv \
  --output wire-out/native-mono-unity-callback-trace/summary
```

`entries.tsv`/`api.tsv`/`startup.log`/`inputs.json`/`run.json` 保留原始结果。
`summary/` 包含输入 hash、边界 samples、实际 InternalCall 目标与 metadata 候选关联。
本轮不改 Mono patch stack，不重建 runtime dylib，不更改游戏后端默认选择。
