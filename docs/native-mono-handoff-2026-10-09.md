# 原生 Mono 持续推进交接（2026-10-09）

用户已授权自主决策、自己测试修复和本地提交，继续推进原生Mono计划；不push。另一个agent
先前处理SSE，故本任务一直使用独立worktree。用户后来告知SSE完成，已将main更新
合入Mono工作分支并补齐shared RMW接入；主checkout核心代码仍保持原运行路径。
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
| 73c8207 | Mono创建线程的process-owned服务队列、owned参数结果、取消/错误/GC回归 |
| e52848c | 生命周期绑定的aligned共享byte buffer allocator、realloc/aliases/真实backing owner |
| da87bb1 | 将已完成的main SSE/cache更新合入独立Mono分支 |
| 8495b3c | 新RMW快路径接入shared regions，同lease受检read/update/write |
| f540ba7 | guest exact/interior roots快照、真实handle事务回滚与持续attach下GC回收 |
| d55bb1f | 批量提取289/293候选C签名、71实际定义和Unity callback表 |
| b064c11 | 真实Unity启动入口/回调追踪、10824项实际InternalCall目标及allocator空实现修正 |


具体实现、边界和复现命令在worktree的 `docs/native-mono-services-and-gc-2026-10-09.md`。
新增M3状态/预检说明为该worktree的 `docs/native-mono-m3-lifecycle-2026-10-09.md`。
新ownership/接口说明：`docs/native-mono-shared-buffers-2026-10-09.md`、
`docs/native-mono-guest-roots-2026-10-09.md`、`docs/native-mono-c-api-contracts-2026-10-09.md`。
真实启动输入、InternalCall targets和allocator修正见worktree `docs/native-mono-unity-entry-trace-2026-10-09.md`。
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

最新完整M2 guard `checks=538 denied=0`（checks随GC时序变化）；M1 core/interop/threads/gc/layout通过，5个executable
memory反向用例以86拒绝；链接审计304对象/668依赖/0 unmatched。独立M2 Clippy clean。
最新main SSE基线及入口追踪上的feature完整Rust回归385项通过、7 ignored；all-target feature Clippy clean。
本轮默认路径完整Rust365项通过、7 ignored；M2真实Mono/原始DLL/程序集inventory重新执行成功。
构建stack工具3项回归通过。registry debug bugfix及shared RMW整合修复均单独提交。

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

## 当前新增实测与下一阶段

Mono自己的Thread.Start创建两个worker，通过容量4固定owner复用Runtime64 handles/GuestFs。
36个job/6次等待期间GC、不同线程LastError2/87、失败输出sentinel和byte[]保持均通过。
12个in-flight route撤销：pending/closed send/reply均返回50，所有owned captures释放、0job执行。
queue不携带native/managed pointer，caller byte[]全程pin。SetThreadErrorMode remote仍拒绝50，
避免错改owner上下文；production Process/TLS/execution token/async服务仍未接。

共享buffer arena在真实checked IL中逐byte写/核对65537B，grow到131071B后尾部zero，alias互见；
readonly/bounds/stale addresses fault、realloc失败保留旧数据、actual backing view阻止VM cleanup。
它是engine/unsafe-IL bytes allocator，不是UnityAllocatorVTable整体替换，也不是general Mono heap。
main新增modify_int快路径原先跳过shared，已接入单lease检查并通过定向回归。

GuestRoots模块用v2强弱handles保留刚暴露对象；完整paused寄存器+明确stack/static/heap槽
snapshot可收敛roots，exact及interior pointers均识别。失败range拒绝整份snapshot；新增root
第二次获取失败时释放第一个新句柄、保留旧root，真实tracker无泄漏。三对象在creator始终
attach时经过16次GC，live snapshot保活、空snapshot全部回收。必须接Process所有root sources
与暂停点，缺源时不能使用synchronize丢root。registry actual owner阻止提前cleanup。

C compiler清单：293实际query中289候选native C签名、71本轮实际函数定义；冻结406个transitive
依赖。源码声明不证明原始Win64 ABI，所有game gateway准入0。两个allocator名字不同：
旧unity_mono_install_memory_callbacks(MonoMemoryCallbacks*)与真实mono_unity_install_memory_callbacks
(MonoAllocatorVTable*)不可混用。后者包含version/malloc/realloc/free/calloc。UnityTLS仍opaque、
vprintf含异架构va_list，不能直接传guest pointer或宣称完整callback表已知。

最新真实Unity启动诊断在worktree `wire-out/native-mono-unity-callback-trace`，命令/binary/hash/input/log
及entries/api.tsv已冻结。原始x64 Mono运行2B work到Budget：可见1024×720窗口、290draw/97Present；
不是native后端或已稳定可玩。246个PC活跃/6249665 raw entry hits/2408 samples；113个不同入口
名称的sample返回PC落在UnityPlayer，含尾调用，不能当全部外部CALL次数或性能占比。

实际mono_add_internal_call登记10824名称/10824guest目标，采集失败0；嵌套类型/与metadata的.
表示统一后全部一一对应声明候选，ABI准入仍0。这些可用于下一步的真实typed Unity bindings。
工具 `tools/mono/entry_trace_report.py` 根据实际export RVA验证Mono映射0x182940000/0xa42000，
从log读取UnityPlayer映射，并输出各input hash、边界sample、注册target及声明候选清单。
Memory::observe保留owned/shared guard/权限/撤销/失败输出语义；新回归覆盖入口别名、批处理
段内部、动态模块、callback PC、取指故障、逐budget CPU一致及sample/binding容量。

关键修正：精确原始DLL的mono_unity_install_memory_callbacks入口RVA0x270ba0为c2 00 00，
直接返回，不读表；mono_set_allocator_vtable RVA0x74870仅检查version，不装callbacks。
本轮四个allocator callback入口均0，plugin callback1；disassembly保留在诊断目录。
该样本不用先做Unity allocator回调桥。Native candidate config undef ENABLE_OVERRIDABLE_ALLOCATORS，
eglib g_mem_set_vtable也为空，但candidate Unity installer仍读version，不能直接声称同原始行为。
未来该精确profile的Unity installer需保持立即返回、不额外解引用guest参数；其他版本不能照此忽略。
一般managed heap ownership/engine buffers/GC/roots仍未解决。当前Mono patches仍19个，未重建dylib。

下一轮优先实际Unity provider/根域初始化和typed callbacks收敛，避免只增加孤立fixture；还需
一般managed objects/arrays/helpers、raw reference store ownership、全Process roots/safepoints、
稳定native Mono TLS/guest execution owner、Unity allocator/TLS/log/plugin/liveness callbacks。
完整ABI及生产services/scheduler未准入，native game backend/M3仍false，没有真实游戏加速数据。
不得仅把Admission字段设true来接入；要以actual binding及真实参数/return/owner证据建立gateway。

不以同时启用两个VM、按函数回退旧VM、禁用GC、假API成功、开放未知native pointer或生成
宿主机器码绕过准入。大量未实现项仍是继续工作，不是已完成。继续在独立分支工作并更新本文。

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

## 继续推进进展（独立 worktree，主代码未合并）

实施路径仍为`/Users/alphaly/.codex/worktrees/native-mono-shared-memory/wire`，分支
`codex/native-mono-shared-memory`。新增提交：b67d9ea（真实bindings/profiler）、7a3a062
（UnityTLS/image）、e8f2aba（typed metadata），随后rooted string提交已在该分支完成。

真实Unity已经完成10824注册entry核对、82个image/assembly循环、5643 metadata查询，
53B输入native string创建/stack交接/强制GC保活和退休。最新证据位于worktree
`wire-out/native-mono-unity-strings-probe-01`，guard178/0、无original Mono attach。
首次pending invoke确认System.Type.GetType(String)，正在继续reflection对象桥接。
完整game Admission仍false，未宣称完整游戏原生执行/加速。主checkout代码未改；
浮点和翻译缓存未改，其他agent可继续其工作。下一位接手应使用该分支及其最新handoff，
不要从本主checkout旧provider或孤立fixture重新开始。

## 真实绑定及 GC/shutdown 已接入（本轮）

最新实现仍在本独立分支；实际 10,824 注册名/PC 与 original trace 逐项匹配，两个真实
CoreModule Boolean InternalCall 完成 native Mono → 固定 ARM64 adapter → Win64解释器往返。
其他10,822项仍deferred，完整game Admission false。legacy/new profiler NULL handle均已处理；
真实GC8回调/0xfe1、cleanup再GC8回调及shutdown1次已实测，guard89/0。
证据 `wire-out/native-mono-unity-profiler-probe-04`；详情
`docs/native-mono-unity-bindings-profiler-2026-10-09.md`。继续UnityTLS及managed image加载。
用户最新要求不碰其他agent的浮点/翻译缓存代码；本轮没有涉及这些实现。

## UnityTLS / 82 native image cycles 已实测（继续中）

`native-mono-unity-images-probe-01`完成实际82 open/load/close/get-image；native refs退休，
原始Mono未执行，guard178/0。UnityTLS两个struct/ref叶入口有真实表函数指针对照；其他31项
typed fail-stop，网络未实现。新边界pre-images/images；后者止于首个class query前。
当前feature完整Rust394通过/7 ignored，feature Clippy clean，默认回归正在运行。
详见 `docs/native-mono-unity-images-2026-10-09.md`。继续typed metadata，不碰浮点/翻译缓存。

## Typed metadata 已推进至string构造（继续中）

`native-mono-unity-metadata-probe-05`实际5643 query/955 typed handles/687 owned names，guard176/0。
82次真实image生命周期及GC/TLS/cleanup仍通过，当前domain身份已核对，NULL stack roots可清。
止于mono_string_new_len，args rootToken + UTF8源53B。详情
`docs/native-mono-unity-metadata-2026-10-09.md`。Class userdata只映射8B owned scalar shadow，
不写native userdata，不准入依赖此字段的managed callbacks/完整metadata ABI。
下一步继续string强根和受检视图及stack-reference；default365/feature394是上一images阶段
完整验证，不代表后续对象代码。仍不碰浮点/翻译缓存；用户授权持续自己测修提交、不push。

## Rooted string 已完成，继续首个 managed invoke

`native-mono-unity-strings-probe-01`：真实53B输入string经native GC验证身份/payload，guard178/0。
首次invoke为System.Type.GetType(String)，不是先前推测的Assembly.LoadFrom。该阶段仍stop-before。
详见`docs/native-mono-unity-strings-2026-10-09.md`。继续reflection object/typed managed invoke。
metadata完整feature回归395 passed / 7 ignored；string targeted11与all-target Clippy通过。

## 真实 managed GetType 已执行，继续 classlib 初始化

`native-mono-unity-managed-types-probe-03`：1次native GetType，RuntimeType/string强制GC
验证通过，17546 metadata queries/1926 handles，guard176/0。下一invoke是CoreModule
ClassLibraryInitializer.InitAssemblyRedirections static void()，owned IL/依赖已查明。
详见`docs/native-mono-unity-managed-types-2026-10-09.md`，不要停在这个小里程碑。

## Classlib redirections 已执行，继续 UnityLogWriter

`native-mono-unity-classlib-probe-02` 实际执行 InitAssemblyRedirections static void()。
20-patch 独立 runtime 修复 no-JIT remoting field wrapper 的缺失 IL，六套 native fixture/
五条 noexec 路由、完整feature397/7ignored、Clippy和build5tests通过，guard176/0。
详情 `docs/native-mono-unity-classlib-2026-10-09.md`。新 runtime/support pair 与旧 pair
严格分开；旧19-patch源码与marker已恢复核对，旧dylib未变。
下一真实invoke是 ClassLibraryInitializer.Init（0x060022fe）→ UnityLogWriter.Init。
完整game backend仍false，持续推进，不碰浮点/翻译缓存。

## Unity 日志、增量 GC 与同步上下文已实测（继续中）

实施分支新增 ae456bf：修复 native Mono interpreter 静态引用/引用结构体漏 write barrier。
真实 collation 普通 GC 通过而 Unity incremental=3 失败的根因已定位，新 21-patch runtime
8 套 fixtures 在普通/增量配置均通过，5 条 noexec 反向路由仍拒绝。旧 runtime 未修改。

`native-mono-unity-synchronization-probe-10` exit 0，guard 231/0，原始 Mono attach 未发生。
实际日志 null/empty/Unicode/Console 4 路、异常注册、项目路径 String 初始化、同步上下文
5 步均执行；Current/Awaitable/managed thread ID 1 的共享身份与 GC 后 TLS 验证通过。
130 image cycles、43623 metadata queries、585 empty arrays/strong handles、7 RuntimeTypes/
12 strings 全部相关 GC/cleanup 验证。class userdata getter/setter 共用 guest scalar cell，
不把 guest cache pointer 写入 native MonoClass。feature 全量 401 passed/7 ignored、Clippy
clean；后续方法真实 probe/16 项定向另测。没有改浮点操作/翻译缓存。

证据与限制见实施 worktree `docs/native-mono-incremental-gc-2026-10-09.md` 和
`docs/native-mono-unity-logging-2026-10-09.md`。下一 actual invoke 是 SubsystemsModule
SubsystemDescriptorStore.ClearManagedDescriptors（0x06000057），static void()，已有
owned IL。继续自己测修提交，不因这个小里程碑停下；完整游戏 backend/Admission 仍 false。
