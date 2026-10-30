# 基础字符串、指针与进程查询边界

新 Wasm 入口已在两种 ABI 接入七项指针辅助服务，均通过 kernel32/kernelbase 提供，且不采样时钟。服务成功计一 work，不增加客体退休数；参数与返回帧在数据写入前完整保存。相关 LONG/SLIST 服务见 [Interlocked 契约](interlocked.md)。

| 服务 | 新入口契约 |
| --- | --- |
| IsBadReadPtr、IsBadWritePtr | 完整指针与 SIZE_T；零长度不访存，权限/范围失败返回 TRUE，保持 LastError；guard 检查失败会消费该页 guard |
| IsBadCodePtr | 检查一个字节的执行权限，保留 PE32 DEP 规则；guard 返回 TRUE 且不消费 guard |
| FlushInstructionCache | 接受完整当前进程伪句柄或具有 PROCESS_VM_WRITE 的真实句柄，具体行为见[虚拟内存契约](virtual-memory.md) |
| MulDiv | 三个有符号 32 位整数，最近舍入且中点远离零；除零或结果溢出返回 −1，保持 LastError |
| EncodePointer、DecodePointer | 沿用可逆的 identity 策略，保留架构完整指针宽度，不访问客体内存，保持 LastError |

MulDiv 数值算法已提取到 `win32/pointer_values.rs`，新旧内存适配共用，运算不依赖旧执行器。范围检查只查询对应页和权限，不复制大块数据；AMD64 大于 4 GiB 的长度不会被截成 DWORD。Flush 的错误处理独立于其余不修改 LastError 的辅助调用。

进程/亲和性查询已在两种 ABI 接入，身份、权限、继承、复制、Ex 内存调用和等待见[进程句柄与查询](process-control.md)。新入口的处理器能力与拓扑查询见[系统信息](system-profile.md)。

基础字符串 14 项在两种 ABI 通过 kernel32/kernelbase 提供，旧入口和新入口共用处理体。A 版本 copy/length 保留原始 byte，W 版本保留 UTF-16 unit；lstrlen 采用 A 规则，空指针长度为零。lstrcpyn 按有符号 DWORD 容量处理，容量为 1 时只写 NUL，非正容量不访存并返回完整目的指针。先读取 source 再写目的，支持重叠；concat 在 source 快照后检查目的终止符。完整输出预检失败不改变输出和调用者，成功保持 LastError。

普通长度/复制读取上限为 1M unit；lstrcmp 的文本读取上限为 32768 unit，A 输入先按 CP932 解码，随后按 UTF-16 unit 字典序比较。忽略大小写时只采用可表示为单个 UTF-16 unit 的 uppercase 映射，保留原始代理单元。lstrcmp 的空指针按空文本处理，返回 −1/0/1 的 DWORD 表示。

CompareStringOrdinal 要求非空指针；显式非负 count 最多 1M unit，包含嵌入 NUL，零 count 不访存。count=−1 按 32768-unit 终止文本读取，其他负值无效；返回 1/2/3。非法 count 或超过读取上限返回零并写错误 87；实际内存访问故障保留原服务帧与 work，修复后可重试。参数和返回地址先于输出捕获，输出与返回槽重叠时继续使用保存的返回地址。
