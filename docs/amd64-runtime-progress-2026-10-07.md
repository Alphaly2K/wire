# AMD64 / Unity / Mono 启动推进（2026-10-07）

这是接续 `72de02b` 的运行时检查点。普通 `wire run` 仍拒绝 AMD64；
实际启动使用 `wire probe64 --startup`。解释器是主路径，所有高地址访问仍经过 checked guest memory，
没有把 guest 机器码交给宿主 CPU，也没有引入 QEMU GPL 实现。

## 当前真实结果

AliceToPapa 隔离副本已从13条 EXE 入口指令推进到 **20,680,194 条退休指令、
20,749,873 work、65,725 次已完成 API 调用**（`linked-pass-67.txt`）。
已经实际执行 UnityPlayer 的 TLS / DllMain、EXE 入口，以及动态加载 Mono 的 TLS / DllMain；
合作调度的 worker 也在执行。当前停止于第三个线程的
`kernel32.dll!RaiseException`：code=`0x406d1388`、flags=0，已注册一个 vectored handler。
这是 MSVC 线程命名异常，需要实际执行异常处理链，不能直接当作普通函数返回。

游戏实际写出的 `Player.log` 包含：

```text
Mono path[0] = 'C:/AliceToPapaADV_1-0-0_Data/Managed'
Mono config path = 'C:/MonoBleedingEdge/etc'
```

当前证据还不足以验收托管游戏入口、正式窗口、D3D11/DXGI Present 或持续可玩。
启动指令数是兼容进度，不是性能收益。AMD64 仍使用逐指令 decode，没有64位安装期 IR。
既有 PE32 / Metal 路径不受这条诊断路线替换。

| 重放 | 退休指令 | 已完成 API | 最后边界 |
| --- | ---: | ---: | --- |
| pass-61 | 4,044,386 | 8,960 | `CVTDQ2PD` |
| pass-62 | 4,275,323 | 9,525 | `GetFileInformationByHandle` |
| pass-63 | 4,275,393 | 9,527 | `SystemTimeToTzSpecificLocalTime` |
| pass-64 | 4,275,857 | 9,536 | `GetTimeZoneInformation` |
| pass-65 | 4,277,163 | 9,548 | `CreateFileMappingW` |
| pass-66 | 16,277,942 | 53,280 | `SetThreadStackGuarantee` |
| pass-67 | 20,680,194 | 65,725 | 线程命名 `RaiseException 0x406d1388` |

指令与 API 计数受真实时间/线程调度影响，上表不构成相同路径的性能 AB 对照。

## 已实现的运行时范围

- PE32+ 静态及动态依赖、DIR64、64位 IAT、导出、delay import gateway；真实 TLS callback / DllMain。
  重复完整路径 LoadLibrary 增加引用；动态 TLS 模板为所有存活线程单独建立。
  新线程第一次调度才收集 DLL_THREAD_ATTACH，避免漏掉创建后、恢复前装载的 DLL。
  loader owner 防止另一个线程交错进入初始化通知。
- GetModuleHandleEx A/W 支持 FROM_ADDRESS、UNCHANGED_REFCOUNT、PIN；guest 模块额外引用可释放。
  最后一次真正卸载、失败 DllMain 的完整装载回滚仍拒绝，不把 FALSE attach 报成成功。
- Windows x64 整数/指针 gateway、RCX/RDX/R8/R9 和 shadow-space 栈参数、完整64位返回地址；
  guest callback 可以跨逐条预算暂停恢复，不复用32位 stdcall 清栈。
- 稀疏高地址 VMM 的 reserve/commit/decommit/release/protect/query、image/private/mapped 类型与 guard。
  宿主内存始终是数据；跨页检查失败不提交半次写入，guard 消耗保留其架构行为。
- 合作线程的真实执行、挂起/恢复、等待、退出状态、线程句柄别名、独立 TEB/TLS/FLS、
  DLL thread attach/detach、FLS 析构、独立 MXCSR/x87 控制字。
  event/semaphore/critical section/SRW/InitOnce 使用真实对象和所有权，不伪造等待成功。
- confined 文件系统、同步文件 I/O、共享与访问权限、句柄复制/关闭、文件偏移、匿名管道、
  环境变量、命令行、guest 用户目录、系统/时间/拓扑、CP932/UTF-16 和有界 locale 查询/回调。
  COM/WinRT apartment 初始化与引用平衡已实现，不等于完整 COM 或 WinRT 对象支持。
- PSAPI/K32 模块查询使用实际 guest PE 元数据。内置 provider 是逻辑模块；没有 PE 时
  ModuleInformation 返回不支持，逻辑 system32 文件名不声称磁盘上存在 DLL。
- FormatMessage A/W 的有界系统消息 / FROM_STRING 与 LocalFree 所有权；复杂 inserts/语言仍失败。
- ETW provider 注册/注销、disabled-session 查询与写入验证。当前没有 tracing session，
  不发假的 enable callback，也不声称已记录事件。
- `.pdata` 静态函数表、RtlAdd/DeleteFunctionTable、RtlInstallFunctionTableCallback 和
  RtlLookupFunctionEntry。动态查找真正调用 guest callback，检查返回条目和输出 image base。
  已修复 Mono 初始化因 RtlInstallFunctionTableCallback 失败产生的断言，未忽略原来的 RaiseException。

## CPU 与浮点边界

AMD64 整数宽度/窄寄存器/REX/RIP-relative、FLAGS、乘除、位操作、条件移动、锁定操作、
CMPXCHG8B/16B、预算可暂停的 REP 与 CPUID 单 guest CPU profile 已覆盖当前启动需要。
所有 AMD64 FLAGS 仍完整计算，不沿用未经验证的32位 FLAGS 丢弃优化。

SSE 的 moves/shuffles、基础 packed/scalar arithmetic、compare、sqrt、reciprocal estimate、
整数/浮点互转采用 checked operand width 与 MXCSR。APFloat 处理舍入、DAZ/FTZ、
NaN/indefinite 和异常；多个 lane 聚合异常，unmasked #XM 时只提交 MXCSR 状态，不提前提交目标寄存器。
新增六种 packed integer/float 转换使用类型对应的 lane 规则；legacy m128 要求16字节对齐，
CVTDQ2PD 的 m64 只读8字节、允许不对齐。
Unicorn 对部分 float→integer 转换不更新 MXCSR，因此寄存器结果做差分，异常状态另用独立架构向量检查。

x87 当前只补 FNSTCW / FLDCW / FNSTSW / FNINIT / FNCLEX / WAIT，
状态保留既有 FP80 payload，不降成 F64。**尚无完整 AMD64 x87 算术、栈或环境指令**。
既有 Fpu ip/dp 为32位；控制指令不修改它们，后续完整64位 FPU 指针必须单独扩展。

## 文件元数据、时区与映射

GetFileInformationByHandle、GetFileTime 和常用 GetFileInformationByHandleEx 的
Basic / Standard / AttributeTag / Id 查询来自实际文件 descriptor。
文件大小保持64位，volume/inode/hard-link identity 不用随机句柄冒充；重命名后仍查询打开的文件。
多个可空 FILETIME 输出在写入前全部 preflight。

默认时区转换使用 libc 的日期对应 tzdata；显式 TIME_ZONE_INFORMATION 支持固定偏移、
相对/绝对转换日期和南半球规则，毫秒与跨年保留。反向转换拒绝春季 gap，显式 overlap 选标准时间；
默认时区反向 overlap 目前采用宿主 mktime 的选择。
GetTimeZoneInformation 从宿主当年偏移/夏令时转换生成规则；无法用一个 Windows TZI 表达的
基准偏移变化/多次夏令时区间返回不支持，不声称支持 Windows registry 时区库或动态历史 TZI。
固定时区及纽约/悉尼的独立子进程验证已通过。

文件映射目前支持**未命名、file-backed PAGE_READONLY / FILE_MAP_READ**，包括 MapViewOfFileEx、
64位最大尺寸/偏移、精确视图范围、Flush/Unmap、句柄复制与保护关闭。
Section 保留真实 descriptor，视图以独立引用延续生命周期；guest WriteFile 按 volume/inode
更新全部重叠视图，不改变 guest 页的 READ / NOACCESS / guard 权限。
VirtualQuery 输出 MEM_MAPPED，VirtualFree 不可释放视图，VirtualProtect 不可升级成可写/可执行。
活跃 section/view 阻止 guest truncate，避免旧范围失效。单 view 限制256MiB、总页容量512MiB。
宿主外部进程修改文件尚不实时同步，当前样本使用稳定的隔离资产；这不是完整共享 section 服务。
可写/COPY/EXECUTE、命名 section、分页文件 backing 和 SEC_IMAGE 仍明确返回不支持。

SetThreadStackGuarantee 的零请求真实查询当前保证值0；非零请求返回 ERROR_NOT_SUPPORTED，
保留输入大小。目前没有 overflow-handler reserve 或 stack-overflow SEH，不能报告保证成功。

## Mono 原生 hook 的判断

可以把 Mono C API 接到宿主构建的 Mono 解释器，减少模拟 x64 Mono/GC/JIT 基础设施的成本。
接入点应是 guest 导入/GetProcAddress 的显式 bridge，而不是修改宿主机器码或把 Windows DLL 改名装载。
必须成组处理 domain/image/method/object handles、GC root/pinning、UTF-16 和托管异常；
Unity 注册的 internal calls 是 guest x64 函数，需要从宿主托管解释器重入 guest 调度器；
P/Invoke、反向回调、TLS/线程附着、GC safe points 同样需要 ABI 和所有权桥接。
Unity fork 与对应 class libraries 的版本也需要匹配；仅替换 mono_jit_init / mono_runtime_invoke 不够。

这条路线尚未实现或作为当前成功证据。本轮没有安装或链接宿主 Mono。
只读参考的 Unity Mono 分支文件声明 runtime MIT、部分组件 BSD，构建工具另有 GPL；
没有复制这些源码入库。未来引入宿主 Mono 时须核对具体版本和实际组件的许可及 notices。
no-JIT 与使用数据 IR 也不自动证明 App Store 审核一定通过。

## 复现与接续

原样本只读；运行目录是 `wire-out/games64/alice/runtime/`。
EXE / Unity / Mono 的 SHA-256 仍与 [恢复样本记录](x86-64-sample-2026-10-06.md) 一致。
每次重放的完整输出保存为 `wire-out/games64/alice/linked-pass-N.txt`，不提交游戏资产。

```sh
cargo build --release
cd wire-out/games64/alice/runtime
../../../../target/release/wire probe64 AliceToPapaADV_1-0-0.exe \
  --max-instr 50000000 --startup
```

下一阶段先实现真实 AMD64 异常分发/上下文与 unwind，继续 `0x406d1388` 的 guest 处理路径。
随后用程序集执行、正式窗口和实际 Present 验收；不以 LoadLibrary 成功、错误退出0或空窗口当作跑通。
完整 release 回归通过249项：8 unit、37 differential、9 FPU、26 IR、1 Wasm、109 Win32、59 AMD64。
两项默认 ignored 的真实 Metal 设备测试另行通过，合计251项。
日志为 `/private/tmp/wire-amd64-tests.txt`、`wire-amd64-metal.txt`；
Clippy release all-targets `-D warnings`、format、diff whitespace 检查通过。
`scripts/package-macos.sh` 重建 `wire-out/Wire.app`，bundle 与 release binary 校验一致。
旧文档的13条入口/无 Win64 runtime 是历史状态。
