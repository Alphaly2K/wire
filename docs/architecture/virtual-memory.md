# 虚拟内存调用边界

新 Wasm 入口在 PE32/Win64 接通 VirtualAlloc/Free/Protect/Query 及 Ex 八项服务，直接使用同一 Memory 的 reserve/commit/decommit、protect/query、guard 和映射所有权协议。`win32/catalog/virtual_memory.rs` 声明 provider 与 ABI：PE32 接受 kernel32，Win64 接受 kernel32/kernelbase；SIZE_T 使用客体指针宽度，DWORD flags/protect 保持 32 位。Ex 接受当前进程完整宽度的伪句柄或[真实进程句柄](process-control.md)：Alloc/Free/ProtectEx 要求 PROCESS_VM_OPERATION，QueryEx 要求 PROCESS_QUERY_INFORMATION。无效句柄返回错误 6，权限不足返回 5，校验先于指向的缓冲区访问。

核心使用单份稀疏 backing。页面总量上限为 2 GiB，单次 map/commit 上限 256 MiB；普通映射、PE merge、运行时和 DIB 分配共用容量检查，纯 reserve 不计 committed bytes。分配或提交失败不留下部分页面。映像、运行时、只读文件视图和 DIB 具有 owner，普通 VirtualFree/decommit 协议只允许 guest allocation，不能释放这些对象的 backing。

读写和取指检查完整客体地址、页存在性、权限和 guard；实际受检访问消费 guard，编译器观察不会消费。跨页写入先检查全部范围，失败保留字节；写入、保护变更与解除映射统一撤销源码证明和内联 TLB authority。PE32 按映像保留 opt-in DEP，Win64 始终启用 DEP。

VirtualProtect 核心协议要求页面全部 committed 且属于同一 allocation，返回首页面的旧保护。MEM_MAPPED 的新权限不能超过建立时的权限：文件视图只允许读，DIB 允许读写；可缩减和恢复，不能增加 execute。guard 等合法保护修饰位仍按 Windows 检查，密封的 runtime 只读区域保持不可增权。

VirtualQuery 的核心输出包含完整 allocation identity、region/state/protection/type，可编码为 PE32 的 28 字节或 Win64 的 48 字节 MEMORY_BASIC_INFORMATION。DIB 以 MEM_MAPPED、PAGE_READWRITE 发布，其映射由[位图对象](gdi-operations.md)释放；文件视图的更新由 section owner 完成并保留 guest 保护状态。

VirtualProtect 先检查完整四字节旧保护输出，再验证目标页面，旧保护写出成功后提交新保护。输出与目标页面、参数或返回槽可以别名；两种 ABI 均在输出前保存参数、返回地址和清栈量。受检读写或 guard 故障保留 CPU、work 和输出前缀，修复后可重试；LastError 写入故障也保留尚未完成的调用。

VirtualQuery 分别返回 28/48 字节布局，只写实际结构大小。短容量在 PE32 返回错误 87，在 Win64 返回 24；PE32 NULL 输出返回 87，其余受检输出故障按暂停/修复协议处理。成功保持原 LastError，普通 API 错误写入当前线程的 LastError 并返回零；完成计一 work，不增加客体退休，不读取平台时钟。

FlushInstructionCache 也已接入两种 ABI，接受同一当前进程伪句柄或具有 PROCESS_VM_WRITE 的真实进程句柄；两种 ABI 均可通过 kernel32/kernelbase 绑定和动态查询，SIZE_T 参数保持完整位宽。源码写入和权限/映射变更已经撤销 JIT 源码及访存 authority，flush 无需扫描或清空整个代码缓存。错误进程句柄返回 6，权限不足返回 5，范围参数不触发访存。共享目录的旧架构策略保持，新运行时通过明确 provider 资格扩展该入口。

宿主检查接口 `wire_protect(owner, address, size, protection)` 在暂停时使用相同 VMM 保护协议，不提供 Win32 的旧权限输出和 LastError 提交。分配当前支持 MEM_RESERVE、MEM_COMMIT 和 MEM_TOP_DOWN；large pages、reset、write-watch 及远程进程尚未接入。

ReadProcessMemory、WriteProcessMemory 在两种 ABI 通过 kernel32/kernelbase 提供，当前只接受本进程的伪句柄或真实句柄。读取要求 PROCESS_VM_READ，写入同时要求 PROCESS_VM_WRITE 和 PROCESS_VM_OPERATION；无效或权限不足分别返回错误 6、5。长度和可选传输计数均为完整 SIZE_T，零长度不访问数据指针。

全部远端范围在传输前检查相应权限，任何不可访问页、只读写目标、guard 或溢出均返回 FALSE/ERROR_PARTIAL_COPY，计数为零且数据保持原值；远端检查不消费 guard。调用者数据缓冲区和计数输出使用普通受检访问，故障暂停可修复；错误计数和 LastError 统一预检，失败输出不留下部分结果。重叠范围按 memmove 语义复制，成功后写出计数，保留 LastError。写入通过同一 VMM 撤销旧 JIT 源码证明，不能通过此接口写入只读映像或 view，也不临时改变目标保护。其他进程尚无地址空间或执行后端。
