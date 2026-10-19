# Win64 Toolhelp 快照资源边界

`x64::runtime::snapshot::Snapshots` 私有拥有记录表、进程/线程枚举游标、handle flags 和单调编号。单个 Snapshot 及其字段不向组合根暴露。Runtime64 只提供当前 guest 的可执行文件名和未退出线程的 `(id, priority)` 迭代器；owner 验证 flags/容量后才收集这些值，不访问宿主进程列表，也不保留 Threads 引用。

CreateToolhelp32Snapshot 保留 DWORD flags、忽略 PID 的原有限语义，只接受 process/thread/inherit 组合。unsupported flags 返回 50，优先于资源耗尽的 8。最多 128 个存活资源，句柄 `0x7fff1000..0x7fff2000` 单调分配，关闭释放容量但不复用编号。继承位映射到 handle flags；PROTECT_FROM_CLOSE 返回 5，token 的独立规则仍返回 6。Get/SetHandleInformation、DuplicateHandle 身份判断和 CloseHandle 通过只读 membership/flags 或显式 owner 方法协调，原查找及早退提交顺序保持。

线程列表与优先级在创建时复制，之后新增线程或修改优先级不会改变旧快照。process ANSI/wide 共用一个 process_done；线程 First 从零、Next 从当前 thread_at 读取，初始 Next 也从零开始。每次先检查完整 64-bit 句柄，再读取调用者的 dwSize（ANSI 304、wide 568、thread 28）；size 不符为 24，列表耗尽或该类型未请求为 18。读 size 的 guest fault 优先于列表不可用。

编码仍由原私有 process_entry/thread_entry 完成，保持 CP932/UTF-16 名称、guest PID 1、线程数量及优先级映射。输出成功后才更新游标；失败 First 不重置、失败 Next 不推进。owner 只借 Memory、参数、原名称与当前线程 LastError guest 地址。返回帧由 gateway 预先保存，因此输出覆盖返回槽仍返回原 PC；成功保留 LastError，guest fault 不提交 CPU/API count。

三个独立旧行为测试先在原实现通过，覆盖限额/退休/flags、ANSI/wide 游标共享、guard/边界故障、saved frame 与线程快照隔离。原创 `gateway-snapshot` 只创建一个 process 快照，循环真实 Process32FirstW copyout，消费 BOOL、PID 和 dwSize，最后关闭并消费 BOOL。n=max(1,work/12)，RAX=570n+1、retired=11n+19、work=12n+21、API=n+2；单次及 12K 输入已在旧冻结实际校验，inventory 固定三个独立 kernel32 OFT/IAT 导入。此批保留七项 legacy provider/ABI 入口，不增加功能或 catalog export。

完整检查、19 条性能协议、raw 漂移复核及三对启动前缀已验收，具体成本与原始证据见[验收记录](../reports/refactoring-snapshot-owner-2026-10-19.md)。
