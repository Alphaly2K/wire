# 原生 Mono：GC、Windows 文件服务和元数据 ABI（2026-10-09）

本轮继续独立分支 `codex/native-mono-shared-memory` 的 M2 原型，没有修改 CPU、SSE、
IR 或主 checkout 的游戏运行路径。**游戏原生 Mono 后端和 M3 准入仍为 false。**
输出目录为该 worktree 的 `wire-out/native-mono-m2-services/`；SDK、游戏及主 checkout
的构建工具仅作为只读输入。没有 push。

## BDWGC 的解释器栈缺陷

真实类库的完整反射枚举曾崩溃，SDK 类库中的 StringBuilder 也出现异常。根因是候选
Mono 的 `mono_gc_set_gc_callbacks` 为空：IL 解释器的局部引用位于独立 `mono_valloc`
数据栈，不在 native pthread 栈中，BDWGC 没有扫描它们。这不是 IL 优化开关导致的故障。

补丁 0012 保存 mini_gc_init 安装的 callbacks，在已有的线程 roots 扫描中调用
`interp_mark_func`，用 `GC_push_all_eager` 立即把活跃槽值加入标记工作。扫描跨度由
解释器自己的活动栈指针和 fragment position 决定，不永久注册整个 mmap 或托管堆。

回归包含：128 个 4096B 数组强制 GC 后 checksum=8128；StringBuilder 重复10000次、
每128次 GC 后仍有30000字符；完整反射枚举100个 P/Invoke；native Mono thread 退栈后
保持 attach 和空闲，8次 GC 清除旧局部数组的 weak handle。回收前不读取 weak target，
避免宿主寄存器意外保活。现有 M1 core/interop/threads/gc/layout 均通过。

## 预编译的 typed Windows 服务

补丁 0011 在 ImplMap 解析之后、普通库加载和 dllmap 之前提供一次安装的 method-qualified
resolver。只绑定明确 MonoImage、声明类型、方法名、完整类型签名、charset、Winapi flags
匹配的静态 ARM64 入口。同名错误签名返回 EntryPointNotFound。不会生成机器码。

首次调用前必须安装 provider：先执行失败的 File.Exists 再安装，会留下缓存的失败 wrapper。
Unix Mono 的 SetLastError wrapper 捕获 errno；入口在所有宿主操作之后写 errno，由 Mono
保留到每线程 LastError TLS。两个真实 native Mono thread 的2/87错误在同步等待后仍独立。

入口复用现有 Runtime64 文件和版本服务。Cpu64 只装载 Win64 参数/返回 frame，包含第5个及
后续 stack arguments，不执行 guest 指令。测试服务所有者与 GuestFs root 由 TLS scope 保留；
撤销后 cached static entry 返回错误50，不执行文件访问。这个 TLS 原型不是生产的跨线程
共享文件句柄、Process64 execution token 或 owner-thread scheduler。

核对并绑定的真实类库方法包括：

- `Interop.Kernel32.GetFileAttributesExPrivate`、`SetThreadErrorMode` 和 Path.GetFullPathName；
- Environment 的 Platform、OSVersion 和真正的 InternalCall `GetNewLine`；
- MonoIO 的 Windows separators、GetCurrentDirectory；
- MonoIO Open、Close、Read、Write、Seek、GetLength、GetFileType、Flush。

全部 InternalCall 安装前核对实际 signature 及 InternalCall implementation flag；输出和
数组只接收受控 native Mono marshaling/已核对类库。Read/Write 校验 byte[]、长度和范围，
短期 pin 后每次最多64KiB，通过 checked scratch 复制；不把 native pointer 转为 guest 地址
后直接解引用。**这仍不是任意 native-pointer 或任意游戏 IL 的通用 sandbox。**

实测 Windows File.Exists/Directory.Exists、相对路径、大小写、UTF-16、结构体大小36、
missing2、escape5、invalid-level87、失败不部分改写输出。真实 FileStream 完成读取、seek、
EOF、180123B分块读写、GC、追加及 File.ReadAllBytes；宿主独立复核隔离目录最终180124B的
每个字节。反射调用 MonoIO.Read 的无效 handle 返回6、无效 range返回87，原数组不变。
`/etc/hosts` 解析到 guest `C:\etc\hosts`，测试中不可见。owner撤销后的文件打开也失败。

实际 mscorlib 的 CLI metadata scanner 读取 ImplMap/ModuleRef/MethodDef/Param/FieldMarshal，
记录完整类型、flags和原始MarshalAs blob，生成 `pinvoke-contracts.tsv`。100条 P/Invoke 与
可选 managed reflection inventory 一致。100不是覆盖率，也不是原计划290个Mono导出查询。

## MSVC 位字段 ABI

补丁0013由 native C编译器提供 MonoClass 六个不可变字段的精确地址和大小。原始用户Mono
叶函数在原有 checked Cpu64 中运行；原始DllMain、VM、GC从不初始化，imports绑定拒绝入口。
未知地址不会通过扩大metadata映射来掩盖。

首次检查发现name/image/parent/type等偏移不匹配。原因是Windows编译器位字段 ABI：
Clang默认与MSVC对MonoClass的guint位字段和相邻guint8字段采用不同分组规则。补丁0014仅为
MonoClass提供显式 `ms_struct` opt-in，构建参数为 `--msvc-class-abi`；默认不改变该布局。
这不是在未知位置插入8B填充，也不是证明所有Mono内部结构都已经兼容。

| 字段 | ARM64默认 | MSVC opt-in / 原始DLL |
| --- | --- | --- |
| parent | 0x28 | 0x30 |
| image | 0x38 | 0x40 |
| name | 0x40 | 0x48 |
| namespace | 0x48 | 0x50 |
| byval MonoType | 0xb0 | 0xb8 |

opt-in重建后原始 `mono_class_get_name/namespace/image/parent/type` 的返回pointer全部与native
C API一致，原始 `mono_type_get_type` 同样一致。对象/数组header、length/data和unbox原回归
仍通过。version/field无效时profile接口返回NULL且size清零。未opt-in的诊断会记录不匹配并
拒绝准入，results不会把诊断运行成功写成元数据兼容。

## 验证和继续条件

最新开启opt-in的M2运行 `checks=105 denied=0`，所有原型通过。M1的五个反向 executable
mapping测试退出86，core/interop/threads/gc/layout全部通过。链接审计仍为304对象、668依赖、
0 unmatched；没有选入GPL atomic stack/malloc设施。原型Cargo Clippy `-D warnings`通过。
这些证据不替代最终发行许可、签名或iOS验收。

```sh
python3 tools/mono/build.py prepare --output wire-out/native-mono-checked-runtime
python3 tools/mono/build.py runtime --output wire-out/native-mono-checked-runtime --msvc-class-abi
python3 tools/mono/audit_build.py --output wire-out/native-mono-checked-runtime
WIRE_MONO_REFLECTION_INVENTORY=1 python3 tools/mono/probe_m2.py \
  --runtime-output wire-out/native-mono-game-classlib \
  --output wire-out/native-mono-m2-services --checked-memory --native-services \
  --sample-mono '<原始 mono-2.0-bdwgc.dll>'
python3 tools/mono/probe.py run --output wire-out/native-mono-game-classlib
```

runtime/classlib/support的准备方法见M1/M2文档。构建hash、source hash、类库hash和每个命令结果
保留于ignored输出。`windows_vfs_bound=0`表示游戏Process未接入；fixture的VFS已经验证。

M2还需完成 reference/array/helper 的受检 IL覆盖、write barriers、生产roots与scheduler，
更多Win64↔ARM64 typed float/struct/sret与callback、完整metadata/vtable/engine生命周期。
Windows服务还只有已测子集。0010全局禁用IL优化仍是原型限制，必须在保留检查后改成选择性
配置；没有真实游戏加速数据。完成这些契约之前不启用游戏后端。

## 后续：typed浮点、结构体与异常

原创PE与ARM64 IL已完成三组混合double/float/int64往返、两组24B结构体往返，以及guest
callback之后返回托管ArgumentException。Win64按参数位置使用XMM0/XMM1/R8；结构体hidden
sret占RCX，后续input在RDX、double在XMM2、int64在R9，返回RAX仍为sret地址。ARM64入口
使用明确的ref/out签名，只有scalar字段通过checked Memory复制，不暴露native原始指针。
包括负数、超32位整数、uint32 wrapping和nested callback；未修改CPU/SSE实现。

这些是固定原创签名的闭环，尚不是所有Unity_Injected ABI的动态dispatcher或通用delegate
入口池。异常通过mono_runtime_invoke的独立exception out返回；owner token以RAII恢复，
没有跨C ABI Rust unwind。完整服务/GC/布局回归继续通过，noexec guard `checks=106 denied=0`。

## 后续：选择性 IL 优化

补丁0015取消prepare对global mono_interp_opt的修改。每个受检方法在TransformData中固定
禁用inline/cprop/super/bblock，并把已采用受检转换的状态随InterpMethod发布。scope内每次
opcode dispatch拒绝此前缓存的未受检转换；原有permit table和checked访存不变。

第一次负例发现runtime-invoke wrapper会提前inline一个未受检方法，从而绕过callee entry。
因此prepare在VM初始化前额外保留wrapper和跨image的inline边界，防止未来scope激活时边界
已经消失。同image的普通IL仍可优化。测试含cached primitive及cached unsafe store：第二次
调用在readonly native backing上被拒绝，原值71没有改成99。全部14个故障和typed/GC/服务
回归继续通过，M2 guard `checks=105 denied=0`。这不扩大unsafe/helper/reference的许可范围。

冻结0015前后的dylib，同一driver、IL及类库，进行AB/BA/AB三对进程运行，每进程预热后7次
20000000迭代，合计每配置21个计时样本。算术+scalar数组workload checksum均为1523661319。
wall中位数从0.297357秒降至0.138959秒，配置比值2.140。
每次运行noexec guard均为denied=0。完整hash、命令、CPU/wall及原始样本在
`wire-out/native-mono-scope-opt-perf/results.json`。这是受控pure IL配置对照，没有与原始x64
Mono或真实游戏比较；不能宣称游戏整体已经快了2倍。

复测工具为`tools/mono/perf_interp.py`，传入冻结的旧runtime、同一M2 driver output与新runtime。
早前0010的全局禁用优化说明保留为历史阶段；当前未受检的trusted类库已经恢复普通优化。

## 后续：受检静态调用与实际绑定清单

0016只加入同image的静态IL `MINT_CALL`：目标不得是wrapper、P/Invoke、InternalCall、native/
compiled code、vararg或缓存的未受检转换。callee进入后继续执行同一permit与访存检查。
`SUB1_I4/I8`仅访问已验证的解释器locals，经核对后加入许可。现在许可349/613；这个数仍不
是游戏兼容率。动态virtual/delegate/calli及外部helper仍未放行。

回归通过递归5层的parent byref、checked child guest写入、readonly child fault、cached target
拒绝及经同image helper到Marshal的拒绝。原始14个访存故障加一个child readonly fault共15，
无部分改写；最新完整M2含metadata inventory的guard为 `checks=410 denied=0`。

`probe_m2.py --managed-inventory <Managed目录>`在native VM中只读取实际程序集metadata，
不调用游戏entrypoint、游戏方法或Unity初始化callbacks。样本153个DLL解析11537个InternalCall、
1393个P/Invoke，unresolved=0，7921条方法名含_Injected。输出`managed-native-contracts.tsv`
包含assembly、完整declaring type、method token、参数和返回类型、impl/method flags，以及
P/Invoke的原始库/entry/flags；所有行 `abi_verified=0`。声明数量不等于使用频率，完整类型
解析也不替代Win64↔ARM64/native marshaling/layout验证。原始153个输入hash保留于results。

重复prepare的旧实现曾逐补丁reverse-check，0015覆盖0010的hunk后即失效。现在从锁定archive
在临时树重建整套补丁，只认可现有source属于一个完整patch prefix，再应用后续patch；未知
编辑保留并拒绝继续。build前复核patch hashes和每个修改source file hash，configuration也
记录这些hash。重叠hunk的重复/升级、未知编辑保护、旧patch内容变动拒绝，3个工具回归通过。
真实source prepare连续执行也通过。修复既不重置source tree，也不重新下载SDK/开发工具。

## 后续：精确 rooted reference slots

0017新增显式version-2 adapter的reference store/load（op5/6）；version1仍拒绝reference。
每个8B槽位独立持有一个strong v2 handle，输入值须为NULL或已登记weak handle仍指向的
确切对象。完整range/permission/guard检查之后，在HostRegion lease内调用真实BDWGC
`mono_gc_wbarrier_generic_store`；成功替换后才释放旧root。load同时核对bytes与strong
root target一致，不能把普通bits或被非类型化写入破坏的数据当作对象。

GC只扫描Mono自己的handle存储，不直接扫描HostRegion bytes；直接GC_add_roots注册
shared bytes会绕过每次native访问必须持lease的契约，所以未采用。普通scalar write/
copy/fill覆盖引用槽也被拒绝。fixture刻意使用非page对齐guest地址，host槽位仍8B对齐。

实测：readonly、one-shot guard、偏移/普通slot、未知bits、NULL/root不一致、scalar
fill覆盖、owner revoke均返回AccessViolationException且失败不改root。创建、验证、替换
和清空在分别attach并退出的native threads进行，main stack不观察managed地址。释放
factory句柄后slot仍保活；替换后旧对象的weak target清空、新对象存活；清空后新旧两个
对象均可回收。该结果不是永久保活整个堆。完整M2与M1仍通过，noexec denied=0；链接
审计304对象、668依赖、0 unmatched，Clippy clean。精确运行hash与调用数在results/log。

这只准入显式登记的独立引用槽。guest x64 raw stores尚未接入typed reference ownership，
引用数组、object fields、一般helper以及生产roots/scheduler仍未完成；不能把此fixture
当完整unsafe-IL sandbox或打开真实游戏原生后端。

## 后续：精确 pinned Int32[] 受检访问

0018显式version3加入array length、Int32 load/store和element address；此前版本在受检
scope里仍拒绝array，不产生raw fallback。注册时用native API核对真实rank=1、element
class=System.Int32和length，retained pinned owner保证payload稳定。每次array操作先核对
确切object identity及live handle，再解析signed Int32 index；对象header不直接解引用。

ldelem/stelem走已有Memory检查，ldelema仅返回已映射的element byref，后续ldind/stind仍
走相同权限检查。底层payload的两个guest别名共享16B backing且所有host访问持lease。
它只覆盖显式登记的Int32[]，没有准入newarr、reference/struct/multidimensional arrays。

实测长循环sum、Int32负值和bit pattern、guest别名双向可见、readonly write/byref、
one-shot guard、-1/length/INT32_MIN/MAX三个element路径、NULL、真实但未登记array、
错误宽度opcode拒绝及撤销的所有array/byref/alias路径。退休mapping/owner后weak target
清空，原main stack未观察managed地址。失败写入采用与原payload不同的值，独立核对
payload没有改变。循环触发的MINT_CHECKPOINT只执行既有runtime interruption checkpoint，
核对后加入许可，继续保留Mono自己的exception边界。

完整M2 `checks=447 denied=0`，M1全通过，五个executable mapping反向测试退出86；审计
304 objects/668 dependencies/0 unmatched。Clippy clean。一般array/object/helpers、
生产thread/roots与typed guest引用写入仍未完成，游戏backend/M3继续为false。
