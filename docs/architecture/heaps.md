# Heap 调用与资源边界

新 Wasm 入口在两种 ABI 支持 GetProcessHeap、HeapCreate/Destroy、HeapAlloc/Free/ReAlloc/Size/Validate、GetProcessHeaps 和 HeapQueryInformation。PE32 provider 为 kernel32，Win64 为 kernel32/kernelbase；新入口单独扩展 Win64 的私有堆、验证和枚举资格，共享 catalog 保留旧入口的可见性。DWORD flags 截断为 32 位，SIZE_T、地址和句柄保留架构宽度。

装载时建立进程自有堆身份，并写入 PEB.ProcessHeap；GetProcessHeap 返回同一身份。堆句柄是不透明的，不提供 RTL 堆头内部布局。所有线程共享进程堆与私有堆，线程退出保留分配。HeapDestroy 禁止销毁进程堆；私有堆销毁后释放全部数据页与身份页。静态启动拒绝时统一回收进程拥有的堆。

HeapCreate 接受 NO_SERIALIZE 和 CREATE_ENABLE_EXECUTE，初始提交大小按页取整；非零 maximum 按页取整，限制该堆活动块的容量总和。零 maximum 表示可增长，仍受 VMM 总额度约束。初始提交或单块请求上限为 256 MiB，每进程最多 64 个堆。所有公开堆与 COM task owner 共用 64 MiB 元数据计费额度，每个活动块按 256 字节计入分配记录及 chunk/free 索引的保守存储估算；数据页仍由 VMM 单独计费。释放、堆销毁和启动拒绝归还额度，移动 realloc 临时借用原块的一条记录，成功替换后保持总计费不变。maximum 使用完整 SIZE_T，允许大于 4 GiB；不为 maximum 立即提交内存。

普通分配按 16 字节切分，零尺寸请求同样返回可释放的对齐地址，HeapSize 保留逻辑请求大小。空闲范围按大小和地址索引，支持相邻合并及原址增长；普通数据区按 64 KiB 批量建立，大请求按页建立。全部释放后最多保留一个 64 KiB 空闲数据区。VMM 统计反映客体数据页，分配器索引另占 Wasm 线性内存。

Alloc 接受 NO_SERIALIZE/ZERO_MEMORY；ReAlloc 另接受 REALLOC_IN_PLACE_ONLY。原容量内调整和相邻空闲区增长保持地址，ZERO_MEMORY 只清零增长部分；普通移动保留旧内容，成功后释放旧块。IN_PLACE_ONLY 无法增长时返回零、保留 LastError 和原块。容量或额度不足返回零及错误 8，原块不变；GENERATE_EXCEPTIONS 与其它未支持标志返回错误 50。

Free 的 NULL 指针在有效堆上成功。无效堆返回错误 6，无效块的 Free/ReAlloc/Size 返回错误 87；Size 的失败值为对应指针宽度的全 1。Validate 验证当前堆身份与块成员，NULL 块表示验证堆，不读取客体字节；非成员返回 FALSE 并保留 LastError。GetProcessHeaps 返回实际堆数，容量不足不写数组，容量足够时完整预检并写出对应宽度的句柄。

所有客体字节仍由唯一 VMM 持有；VirtualFree/decommit 拒绝释放堆页，保护修改沿用 VMM 契约。清零或移动复制前检查整个读写范围，写入完成后才消费空闲区、修改尺寸和发布身份。故障保留 CPU、work 和原分配，可在修复保护后重试。返回帧先保存，枚举输出覆盖调用帧仍使用原返回目标。代码写入、数据区回收和销毁沿同一源码撤销协议；每次完成计一 work、零客体退休，不采样时钟。

HeapQueryInformation 支持 HeapCompatibilityInformation（0），在有效的进程堆或私有堆上返回 DWORD 0：当前分配器没有 lookaside 或 LFH 模式。信息长度使用完整 SIZE_T，至少为 4；可选 ReturnLength 按架构宽度写入 4。容量不足时不访问信息指针，返回 FALSE/122 并写出所需长度；其他信息类返回 FALSE/50，无效堆返回 FALSE/6。完整输出及短容量所需的 LastError 槽先统一预检，故障不修改任一输出；ReturnLength 与信息区重叠时先写长度，后写模式值。成功保持 LastError，返回槽别名使用捕获的调用帧。

HeapSetInformation、HeapCompact、HeapWalk 尚未绑定。固定 Local/Global 使用[同一进程堆](local-memory.md)。[FormatMessage](shell-and-messages.md) 的分配模式先预检输出指针，再初始化分配的块并发布指针；LocalSize 返回请求的字节数，LocalFree 归还块。CRT、COM 等调用者随各自接口接入。旧入口的独立 PE32 arena、Win64 slab 与页分配器保留为迁移来源，新入口不调用旧运行时或执行器。
