# 命令行、启动信息与处理器能力

新 Wasm 入口支持 GetCommandLineA/W、GetStartupInfoA/W、IsProcessorFeaturePresent。共享 catalog 唯一声明签名与 provider：PE32 为 kernel32，Win64 为 kernel32/kernelbase，静态导入和动态查询使用同一资格。

命令行查询返回装载时建立的进程自有地址，不另行分配或重新编码。A 使用 CP932、W 使用 UTF-16，均以 NUL 结束。地址在进程寿命内稳定，查询不会读取命令行存储；没有已装载进程时明确报告服务不可用。

STARTUPINFO 输出严格为 68/104 字节，cb 为对应大小。根进程默认其余字段为零；子进程返回创建时捕获的字段、desktop/title 的 A/W 编码副本，跨父子位宽按各自布局转换。标准句柄查询和运行期重定向使用独立的 GetStdHandle/SetStdHandle 契约。

IsProcessorFeaturePresent 接收 DWORD，返回当前虚拟 CPU 和执行环境的能力，与客体 CPUID 使用同一配置。支持 CX8、MMX、SSE/SSE2、RDTSC、DAZ、SSE3、CX16、RDTSCP、SSSE3、SSE4.1/SSE4.2；NX 反映 VM 的 DEP 状态。PE32 映像未启用 DEP 时返回 FALSE，AMD64 始终启用。AVX、XSAVE、RDRAND 和未知编号返回 FALSE，不按宿主 CPU 推断能力。

两种 ABI 均先保存参数与返回帧，再验证整个输出范围；输出与返回槽重叠时保留原返回目标，故障不提交 CPU/work，可在修复后重试。成功保留 LastError，每次完成计一 work、零客体退休，不采样时钟。

旧入口的架构适配器继续作为迁移来源保留原规则，包括 PE32 输出后读取返回槽、Win64 保存返回帧，以及原 PE32 {6,10}/Win64 {12} 的能力集合。新入口的能力查询遵循当前 Wasm CPU 配置。

CreateProcessW 在两种 ABI 的 kernel32/kernelbase 接受普通 STARTUPINFO，建立独立地址空间、环境和初始线程，支持任意父子位宽组合。lpApplicationName 与命令行首参数分别保存，argv[0] 不决定实际映像身份。环境可继承父进程快照，或使用显式 CP932/UTF-16 块；当前目录在共享磁盘上独立保存。PROCESS_INFORMATION 全部可写且装载成功后才发布进程和主线程句柄，失败不留下可查询子进程。

创建标志接受 CREATE_SUSPENDED、CREATE_UNICODE_ENVIRONMENT、NORMAL_PRIORITY_CLASS、EXTENDED_STARTUPINFO_PRESENT 及其组合。普通 STARTUPINFO 大小为 68/104，扩展记录为 72/112。扩展属性目前只接受非空 PROC_THREAD_ATTRIBUTE_HANDLE_LIST；在创建时读取调用者仍持有的列表，拒绝无效、重复、pseudo 或未标记继承的句柄。其他已注册属性返回 50；保留启动数据和非空桌面名称仍拒绝。存在活动文件 section/view 时拒绝创建。未实现方式返回明确错误；普通启动记录大小不匹配返回 87，坏映像为 193、缺少依赖为 126、缺少导出为 127。命令行当前最多 2047 个 UTF-16 单元，超出返回 206；显式环境上限 2 MiB、1024 个变量。

Web run(budget) 对整个进程组共享 work 预算并轮转执行，全部存活进程等待时返回 Waiting。work/retired 是当前 VM 的计数，totalWork/totalRetired 是组总量，processes() 返回每个进程的 ID、位宽、计数和退出码。多进程停止结果带 processId，入口返回触发正常退出；单进程返回协议保持原样。dispose() 释放整个组。

启用继承时，普通文件、管道、标准端点、进程及远程初始线程句柄按原数值、权限和标志复制；各进程独立关闭，底层文件游标、共享预约和字节队列保持同一对象。显式列表仅选中其中的身份，列表以外的句柄不继承；未提供列表时选择全部可继承句柄。当前选中本地线程、同步对象、token 或快照会返回 50。跨进程命名同步对象与注册表共享尚未接通。

STARTF_USESTDHANDLES 要求启用继承，三个原始标准句柄值复制到子进程的启动记录、进程参数及 GetStdHandle 槽；这些值本身不额外创建句柄，后续 I/O 按实际继承的身份检查。标准输入输出共享宿主队列，子进程关闭本地输入不会清空父进程队列。无继承时子进程使用默认虚拟标准端点。父进程退出不关闭子进程持有的端点或停止其执行。
