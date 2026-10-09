# 原生 Mono 持续推进交接（2026-10-09）

用户已授权自主决策、自己测试修复和本地提交，继续推进原生Mono计划；不push。另一个agent
可能处理SSE，故本任务一直使用独立worktree，不改CPU/SSE/IR，不合并主分支代码。
主checkout仅新增本文，方便后续任务找到成果。

## 唯一工作区

- Worktree：`/Users/alphaly/.codex/worktrees/native-mono-shared-memory/wire`
- 分支：`codex/native-mono-shared-memory`
- 当前实施提交：`05f5bfd`（后续文档提交只更新交接）
- 实施计划输入：主checkout `wire-out/unity-native-mono-plan-2026-10-09/plan.md`

已有阶段提交：

| 提交 | 结果 |
| --- | --- |
| d7f79c4 | 默认关闭的受控shared Memory及8项故障/生命周期回归 |
| 3940ceb | pinned真实数组、两native Mono threads、primitive checked IL、真实类库及object/array叶ABI |
| 9b513d5 | BDWGC解释器栈roots修复、typed Windows查询/流服务、MSVC MonoClass ABI opt-in |
| cd84974 | 原创PE↔ARM64 IL混合float/double/int64、24B hidden sret/ref/out及托管异常 |
| c321e7d | 选择性IL优化、缓存unchecked转换拒绝、wrapper/image inline边界、冻结AB/BA工具 |
| c383320 | 同image受检静态IL调用、真实Unity绑定metadata清单、完整patch stack重复构建 |
| 536f390 | 精确reference槽：write barrier/strong root、替换与GC回收、权限/撤销/root mismatch保护 |
| b8eed5a | pinned Int32[]受检length/element/byref、shared alias、signed bounds与退休回收 |
| 5e7906d | 固定owner-thread queue、100请求/300嵌套guest activations、等待GC与撤销 |
| 4838a09 | 三轮child-domain load/reset/unload、native ID复用下generation安全退休 |
| 4b4b97d | 原始UnityDLL核对native method/signature/type的74项叶ABI |
| 99318f4 | one-use预编译委托入口池、真实闭包/嵌套/自我撤销GC与回收 |
| 40c581d | 独立修复完整debug回归发现的registry then_some提前减法溢出 |
| 3ce4834 | 默认关闭的M3单VM生命周期、活跃lease阻止cleanup、实际query预检 |

具体实现、边界和复现命令在worktree的 `docs/native-mono-services-and-gc-2026-10-09.md`。
新增M3状态/预检说明为该worktree的 `docs/native-mono-m3-lifecycle-2026-10-09.md`。
历史M1/M2文档仍保留，不要把历史kernel32/Unix故障当作当前fixture状态。

## 当前已实测

真实Windows类库的File.Exists/Directory.Exists/FileStream、UTF-16、seek、EOF、180123B
分块读写、append、独立最终180124B字节核对，以及owner撤销/LastError两线程均通过。
BDWGC正确保留活跃IL locals，并在原线程仍attach时释放退栈locals；不是注册整个堆或stack
永久保活。原始MonoClass叶函数在MSVC位字段opt-in后匹配，完整metadata ABI仍未准入。

受检IL现许可358/613个Mint opcode，包含scalar/struct/cpblk/initblk、同image静态CALL、
精确reference cells与登记的pinned Int32[]。reference替换、NULL清空、root/data mismatch、
scalar覆盖拒绝及数组readonly/byref/alias/bounds/guard/revoke/GC reclaim均通过。
这些限定子集之外的reference/arrays/objects/helper、virtual/delegate/calli仍未准入。

两个真实Mono worker经固定owner queue运行100请求、300个嵌套guest activations，10次等待
期间GC与managed locals保活通过；native suspend模式仍preemptive，不声称iOS coop调度。
预编译4槽delegate pool one-use，真实闭包可嵌套重入及callback里自我撤销+GC；容量耗尽
拒绝，所有退休root可回收。三轮child-domain static counter隔离、异常、shared payload
退休和unload通过；native domain ID复用了两次，bridge独立generation阻止旧token复活。

原始UnityDLL在未初始化其VM的情况下，额外核对method/signature/type 74项叶ABI，含instance
正例及ref/out。完整metadata/vtable/Unity callbacks仍未证明。M3状态模块只负责准入/单VM/
失败关闭gate/lease阻止cleanup，不加载VM、不拦截导出，也没有启用实际game profile。

真实游戏153个Managed DLL扫描得到11537条InternalCall和1393条P/Invoke，unresolved=0；
7921条名称含_Injected。仅扫描声明，未执行游戏方法。所有条目ABI准入为0，不是使用频率。
原始用户Mono SHA256：`86a5a4de6bcd74a637d63c96d822dc989d4ffaed6ef1c15c0e6e1f528ae6708f`。

选择性IL优化纯托管对照：冻结新旧native Mono，同一IL/driver/类库、三对AB/BA、每配置21
个20000000迭代样本，checksum1523661319；中位数0.297357→0.138959秒，约2.14×。
这是配置microbenchmark，没有真实游戏native backend加速数据。游戏后端、M3准入仍false。

最新完整M2 guard `checks=495 denied=0`；M1 core/interop/threads/gc/layout通过，5个executable
memory反向用例以86拒绝；链接审计304对象/668依赖/0 unmatched。独立M2 Clippy clean。
新增M3 feature完整Rust回归349项、默认336项通过，各7 ignored；Root feature Clippy clean。
构建stack工具3项回归通过。registry一行debug bugfix单独提交，CPU/SSE/IR没有改动。

## 保留的输出

全部位于上述worktree的ignored `wire-out/`：

- `native-mono-checked-runtime`：锁定source、19个patch、无JIT ARM64 runtime与链接审计；
- `native-mono-game-classlib`：同runtime的symlink、真实类库及预编译interp支持；
- `native-mono-m2-services`：完整M2 driver、results、logs、隔离VFS、contracts及输入hash；
- `native-mono-scope-opt-perf`：冻结AB/BA的命令、hash、CPU/wall及原始样本；
- `native-mono-m3-preflight`：profile及293个Mono query分项表，289个native符号、21个已测leaf query；
- `native-mono-backend-{default,feature}-tests.log` 与 `native-mono-backend-clippy.log`：最新完整Rust验证。

主checkout `wire-out/native-mono-m1` 的源码archives/开发AOT builder、Unity SDK和游戏均只读。
不要重新下载，不修改SDK/游戏，不重新构建已冻结的开发builder。构建后等完成再audit/运行，
不得拿旧results.json证明新的patch。prepare如今认可完整patch stack的精确source prefix，
后续patch覆盖旧hunk仍可重复；未知source编辑保留并失败，不能reset它们。

## 下一阶段

继续general object/reference/array/helper和正式shared allocation，精确槽/Int32[]已完成其fixture。
下一步先扩展native Windows服务的process owner：当前TLS Scope仍是fixture，需要使Mono自己
创建的线程也能通过固定owner queue复用同一Runtime64 handles/VFS，参数转为owned data，
queue不携带native/managed原始指针；等待持正确GC状态并保留caller的byte[] pin。
再收敛正式guest execution token/Mono TLS/GC roots/scheduler；在actual binding清单上建立完整
Win64↔ARM64参数/return/marshaling分类与gateway生命期，然后才接provider/loader及Unity
VM初始化。不要因代码工作量大就宣布阻塞；确有外部输入或不可解决契约时记录证据。

不以同时启用两个VM、按函数回退旧VM、禁用GC、假API成功、开放未知native pointer或生成
宿主机器码绕过准入。大量未实现项仍是继续工作，不是已完成。保持独立分支并更新本文。

worktree写入需sandbox escalation，既有auto-review一直允许授权的本地开发。默认Git签名
因pinentry交互失败，采用单次 `git -c commit.gpgsign=false commit ...`，不改全局配置。

## 真实 Unity 原生有限初始化已接入（05f5bfd）

本轮实现提交 `05f5bfd`，分支仍是 `codex/native-mono-shared-memory`。真实 UnityPlayer
现可通过独立、默认关闭的 native startup provider 建立原生根域，继续完成主线程/两处栈
reference 交接、域配置、embedding hostname 与异常策略；在 InternalCall 注册前清理并停止。
原始 x64 Mono 只作为 fingerprint/export-table 输入，未映射其 PE 页或执行 TLS/DllMain。
完整 game Admission/native backend 仍 false；不是正式游戏原生运行或已获得游戏加速。

CLI `--native-mono-startup-probe DYLIB`，有限 boundary `root|main-thread|pre-bindings`。
工具 `tools/mono/probe_startup.py --boundary pre-bindings --skip-etw` 新建 APFS clone，保存
frozen wire、inputs/hash/command、log、api.tsv 和 results。原游戏/SDK 不写入。预编译
corelib support 复制到 clone；运行时无 support 编译、无 JIT/trampoline 生成。

原始样本 `mono_unity_install_memory_callbacks` 的 `RET 0` 必须保留 RAX，不读 guest 表。
root/profiler/thread 返回不透明 unmapped token；主线程 v2 strong handle 留到诊断结束，
只许可受检 main-stack slot 保存 thread token，不把 guest slot 传入 native write barrier。
全部非 budget outcome/错误都会关闭 probe，先释放 handle 再 cleanup，关闭后禁止 resume。
该有限 exposure 不代替生产 guest root sources/safepoints 或通用 object/reference 支持。

实测 Unity 请求 `mono_profiler_load("etw")`，原始 Windows ETW initializer 有真实代码，
candidate 没有；未显式 skip 时 exit 1/root 未建。显式 skip 仅用于诊断并记录平台差异，
不得计作 profiler 兼容实现。plugin finder 是预编译拒绝入口：若激活即明确 fail-stop，
不返回假值、不继续 native library fallback；本轮成功运行未激活，不证明后续不需要。

最终输出为 worktree `wire-out/native-mono-unity-startup-final-{root,main-thread,pre-bindings}`，
三个阶段均 exit 0，guard **85 checks / 0 denied**、无 original Mono attach。另有
`native-mono-unity-startup-final-etw-rejection`，预期 exit 1，guard 60/0。有限 headless 段
仅约 3.31M guest work，没有渲染；耗时不能同完整游戏启动比较。
最终 Rust 回归 default **365 passed / 7 ignored**、feature **389 passed / 7 ignored**，
feature all-target Clippy `-D warnings` clean。Mono 19-patch stack/dylib 未变。

详细文档位于 worktree `docs/native-mono-unity-startup-provider-2026-10-09.md`。
下一步接真实 10,824 注册 entry 的 typed registry/固定 ARM64 adapters，以及 production
owner-thread guest reentry。不能把未知 guest target 直接交给 native mono_add_internal_call。
随后是 profiler GC callbacks、UnityTLS、managed image/assembly loading；完整 ABI、生产
roots、Windows services/scheduler 仍需逐项验证。不要反复从孤立 fixture 重新开始。

## 真实绑定及 GC/shutdown 已接入（本轮）

最新实现仍在本独立分支；实际 10,824 注册名/PC 与 original trace 逐项匹配，两个真实
CoreModule Boolean InternalCall 完成 native Mono → 固定 ARM64 adapter → Win64解释器往返。
其他10,822项仍deferred，完整game Admission false。legacy/new profiler NULL handle均已处理；
真实GC8回调/0xfe1、cleanup再GC8回调及shutdown1次已实测，guard89/0。
证据 `wire-out/native-mono-unity-profiler-probe-04`；详情
`docs/native-mono-unity-bindings-profiler-2026-10-09.md`。继续UnityTLS及managed image加载。
用户最新要求不碰其他agent的浮点/翻译缓存代码；本轮没有涉及这些实现。
