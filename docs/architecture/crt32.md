# PE32 CRT 调用与复用契约

Wasm 入口绑定旧实现的全部 66 项 CRT 函数与 12 项可写数据导出。provider 沿用 msvcrt、ucrtbase、msvcr*.dll 和 api-ms-win-crt-*，导出名称区分大小写；AMD64 保留原 absent 范围。具体能力和失败协议在[接口台账](../api-migration.json)的 wasm_capability 中逐项记录。

普通 PE32 调用使用 cdecl，先保存全部实际参数和返回地址，成功只弹出返回地址，参数由调用者清理。exit、_exit 和 quick_exit 不读取返回槽。服务接受时计一次 work，不计客体退休；回调使用同一 JIT 和预算，返回推进不重复计费。

字节、unsigned parse、x87 数学和堆排序分别复用 `src/win32/crt_memory.rs`、`src/numeric/x87.rs` 和 `src/win32/crt_sort.rs`。客体内存、时间采样和调用续点由当前 Wasm 适配器负责。

`memcpy`/`memmove`/`memset`/`memcmp` 与 `_mbsnbcpy` 的上限为 64 MiB，超限明确拒绝。memmove 保留重叠复制；memcmp 按无符号字节比较，先检查两个完整范围。strlen 和 strtoul 读取最多 1 MiB；strcmp/strncmp 使用原逐字节比较和终止规则。toupper/tolower 只处理 C locale ASCII，其余整数（含 EOF）保持原值；多字节 helper 沿用 CP932 lead-byte 规则。

errno 是进程拥有、按线程独立的稳定可写数据，初值零，最多保留 4096 个线程单元。strtoul 保留原 base/符号/前缀与 endptr 规则，溢出返回 ULONG_MAX/errno=34，非法 base 设 errno=22；成功保留 errno。输出和 errno 先全部预检，再写 endptr 和错误值，输出覆盖调用栈也使用已保存的返回帧。

floor、ldexp、pow 在 x87 栈返回；_CIpow 从 ST(0)/ST(1) 取操作数，消费两个槽位后压回结果；_ftol/_ftol2/_ftol2_sse 截断 ST(0)，弹栈并返回 EDX:EAX。舍入、NaN、范围错误和数值精度继续使用原共享算法。需要 errno 的数值调用先在副本计算，errno 输出成功后才提交 FPU。仅 time/_time32 采样 FILETIME，返回 Unix 秒数；1970 年前或超出正 i32 范围返回 -1，非空输出写相同值。

`_controlfp_s` 查询或修改 x87/SSE 异常掩码、舍入、x87 精度/infinity 及 SSE denormal 模式，保留 denormal exception mask。两组控制不一致时报告 ambiguity 位；保留异常状态，按当前 CW 更新 x87 ES/B 汇总。输出成功后才提交两组状态；NULL 输出、非法精度和值返回 EINVAL。应用类型和默认文件模式保留经校验的配置，_fmode 与导入和访问器共用存储；FILE 操作及按应用类型选择错误 UI 尚无后端。

十二项原始数据导出由真实运行时页支持，IAT、GetProcAddress、FROM_ADDRESS 和对应 __p__ 访问器共用 provider 的稳定可写单元。每个 provider 独立拥有变量，进程拥有参数、环境和程序路径字符串。数据页随暂存依赖图一起发布，地址选择避开尚未发布的映像；失败的装载不发布数据身份。静态启动拒绝通过同一运行时数据页 owner 归还原始变量和惰性向量。

`_timezone` 初始为进程时区的标准偏移秒数，`_daylight` 表示是否配置季节规则；二者与 Windows 日历和 NLS 使用同一装载前配置。初始化后的单元保持客体可写，Win32 时间查询不重写其内容。

__getmainargs 和 argv 访问器复用已有命令行解析器，提供独立的 CP932/UTF-16 参数向量；argc 是解析后的实际个数，程序路径访问器返回独立可写指针单元。环境初始化保存当前虚拟环境的窄/宽快照，后续 Win32 环境修改不重写 CRT 快照。全部输出先预检，允许覆盖原参数或返回槽。配置支持 no-arguments/unexpanded 模式；通配符展开和非零 startup new-mode 返回 EINVAL。_initialize_narrow_environment/_initialize_wide_environment 幂等。

_initterm/_initterm_e 按需读取最多 4096 项可变函数指针，跳过 NULL；回调可以改写后续项。_initterm_e 在首个非零结果停止，后续访问故障保留已经完成的调用。qsort 复用有界堆排序，最多一百万条记录、总长 64 MiB；比较器使用普通客体 cdecl 调用，保存调用者完整架构状态。每次交换先检查两个完整读写范围，交换和比较结果分别保存进度，后续故障不重复已完成的交换。两类续点支持短预算、让出、线程切换及 DLL 初始化内调用。

_crt_atexit/_crt_at_quick_exit 保存每个 provider 的 normal/quick LIFO 队列，总计最多 4096 项。_cexit 依次执行 normal 队列后返回，允许回调继续注册函数；exit/quick_exit 分别执行对应队列后进入已有进程退出、FLS 和 DLL detach 流程；_exit 跳过 CRT 函数队列。进入下一函数的栈写入失败不移除尚未调用的项，已完成函数不重放。同一 provider 的递归 shutdown 明确拒绝。_lock/_unlock 提供 128 个 provider 内部递归锁，使用当前线程身份、保存的等待调用和现有线程唤醒规则。

未实现的旧 shim 使用明确协议：__acrt_iob_func 返回 NULL/errno=EBADF，setvbuf 返回 EOF/EBADF；_initialize_onexit_table 初始化真实十二字节表，_register_onexit_function 返回 -1/ENOSYS，调用者拥有的表注册/执行尚未接通。非 NULL 的 invalid-parameter/usermatherr/executable-TLS 退出 callback，以及非零 new-handler 模式明确报告 unavailable；NULL 清空请求和 mode zero 可接受。_crt_debugger_hook 在无调试器配置下无副作用返回。它们不提供原来固定成功所暗示的 FILE、回调或分配能力。
