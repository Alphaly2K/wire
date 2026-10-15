# Interlocked 与 SLIST 边界

六项契约已接入 `win32/catalog/interlocked.rs`。PE32 六项 Typed；Win64 仅 InitializeSListHead Typed，其余五项 Absent。两个架构适配器只借用参数和内存，旧分发入口负责返回 frame、LastError 同步和 trace；不再维护重复的手写 ABI 与字符串 handler。

| export | PE32 | Win64 |
| --- | --- | --- |
| InterlockedIncrement / Decrement | kernel32；LONG 按 32 位回绕，返回新值 | absent |
| InterlockedExchange | kernel32；写入并返回旧值 | absent |
| InterlockedCompareExchange | kernel32；比较相等才写入，始终返回旧值 | absent |
| InitializeSListHead | kernel32；清零 8 字节、要求 8 字节对齐 | kernel32/kernelbase；清零 16 字节、要求 16 字节对齐 |
| InterlockedFlushSList | kernel32；返回旧 Next、清空 Next/Depth，16 位 Sequence 加一并回绕 | absent |

这些 API 在当前合作式 guest 调度中单次执行不让出线程。此处保留已有调度模型；未来若并行运行 guest 线程，需要另行建立共享内存原子操作与内存序语义。

PE32 的 LONG 参数及返回值保持 32 位位模式，SLIST_HEADER 为 Next:u32、Depth:u16、Sequence:u16。比较失败的 CompareExchange 只要求可读内存；匹配后的写入仍检查写权限。SLIST 先检查对齐，再按原顺序读取和写入，不增加统一的“预检全部指针”步骤。

InitializeSListHead 使用 Void 返回约定；两端继续保留原有零 EAX/RAX trace carrier。其余 LONG 操作保留原 32 位结果位模式，Flush 返回 guest 指针。返回元数据不在公共 gateway 上额外改写寄存器。

PE32 handler 先写输出，公共 gateway 随后读取返回地址。因此 output 与 return slot 重叠时，返回地址可以被本次输出覆盖。checked memory fault 保留原来的 EAX、ESP 和 LastError。Win64 gateway 提前保存返回 frame；16 字节输出即使覆盖 return slot，也使用已保存的返回地址。故障保留 CPU、API count 与 LastError，指针保留完整 64 位。

独立基线见 `tests/win32/interlocked.rs` 和 `tests/x64/interlocked.rs`，在旧 handler 上三项全部通过；既有 kernel 测试另外覆盖 sequence 回绕、返回链及 comparand/exchange 顺序。原创 `gateway-slist` 使用 guest 栈上的对齐 header，无文件、时钟或 native backend 依赖：PE32 retired=`6*n+11`，Win64 retired=`5*n+11`、work=`6*n+11`，两者均检查返回零与 n 次 API 调用。默认性能 case 集合保持原样。

迁移前日志在 `wire-out/refactoring-2026-10-14/r2-interlocked-baseline-*`。首个 PE32 fixture 把末尾 INT3 错计入 retired，失败日志已保留；修正后的 raw/IR smoke 均通过。

完整正确性、25 条性能路径、SSE 硬件计数调查、thread-ID 局部成本及 Alice 启动结果见 [迁移验收](../reports/refactoring-interlocked-catalog-2026-10-15.md)。
