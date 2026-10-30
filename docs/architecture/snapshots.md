# Toolhelp 进程与线程快照

新 Wasm 入口在 PE32 与 Win64 接入 CreateToolhelp32Snapshot、Process32First/Next、Process32FirstW/NextW 和 Thread32First/Next。两种 ABI 均接受 kernel32/kernelbase，契约集中在 `win32/catalog/snapshots.rs`。ANSI 导出使用 Process32First/Next，不另设 FirstA/NextA 别名。

快照只包含当前客体进程及未退出的客体线程；进程 ID 与当前 TEB 一致，线程列表及优先级在创建时冻结。之后的创建、退出或优先级修改不改变旧快照，快照也不保留线程资源引用。进程项包含映像文件名、创建时线程数与普通优先级基准；线程项包含 ID、所属 PID 和对应基准优先级。名称使用 CP932 或 UTF-16，未请求的类型返回错误 18。

CreateToolhelp32Snapshot 按 DWORD 处理 flags。当前接受 PROCESS、THREAD 及 INHERIT 的非空组合；这两类快照不使用 PID 参数。其余 flags 返回错误 50，容量耗尽返回错误 8，失败返回架构宽度的 INVALID_HANDLE_VALUE。每个 VM 最多 128 个存活快照，句柄在 `0x7ffe0000..0x7ffe1000` 单调分配，关闭释放容量但不复用编号。Get/SetHandleInformation 与 CloseHandle 共用当前句柄路由，防关闭标志返回错误 5。快照复制、模块和堆快照尚未接入；DuplicateHandle 对快照返回明确错误 50。

进程记录的 PE32 ANSI/Wide 尺寸分别为 296/556，Win64 为 304/568；线程记录两者均为 28。枚举先验证完整句柄，再读 dwSize；小于相应结构返回错误 24，足够时只写完整结构，保留其后字节。ANSI/Wide 共用一个进程游标；线程 First 从零开始，Next 从保存位置读取，初始 Next 也可以取第一项。输出成功后才提交游标，失败 First 不重置、失败 Next 不推进。列表耗尽返回错误 18。

记录、布局和游标 owner 从旧入口提取到 `win32/toolhelp.rs`，不依赖 CPU 或客体内存。旧入口保留自己的内存适配与原句柄区间。新入口绑定后按类型分派，保存返回帧后才写输出；输出可以覆盖原返回槽。成功保持 LastError，访存故障不提交 CPU/work；静态启动失败清理全部快照。
