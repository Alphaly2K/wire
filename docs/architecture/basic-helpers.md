# 基础字符串、指针与进程查询边界

本批已接入并验收，实现提交 `2999453`。独立旧行为、完整正确性和性能证据见[验收报告](../reports/refactoring-basic-2026-10-18.md)。

| 家族 | export 数 | PE32 | Win64 |
| --- | ---: | --- | --- |
| 基础字符串 | 14 | kernel32 typed | absent |
| 指针检查、cache flush、MulDiv | 5 | kernel32 typed | absent |
| 进程、亲和性与处理器查询 | 10 | kernel32 八项 typed、两项 group-count ABI-only | kernel32/kernelbase 四项 processor/group-count typed，其余 absent |

三组契约分别声明调用 ID、provider、参数与返回约定，架构入口保持参数读取、返回 frame、LastError 和 trace 提交。`lstrlen` 与 A/W 版本一样有原 Wine spec 的 4 字节 stdcall ABI，独立测试固定该行为。

字符串适配层只借参数与 GuestMem。A 版本 copy/length 保留原始 byte，W 版本保留 UTF-16 unit；bounded copy 按 signed count 处理，容量为 1 时可不读 source 而只写 NUL，非正容量保留输出。先读入 source 再写目的，支持既有重叠别名；concat 继续在读取 source 后检查目的终止符。普通读取的 1M-unit 上限与失败输出保留不变。比较继续使用原 text reader，大小写只采用可表示为单个 UTF-16 unit 的 uppercase 映射，保留 null/显式 count 和无效 unit 的原策略。

指针适配层只借受检内存。零长度 BadRead/BadWrite 不访存，失败返回 BOOL 且不写 LastError；BadCode 使用原 inspect_code。FlushInstructionCache 只接受当前进程伪句柄，其余参数继续忽略，代码写入仍由内存/IR invalidation 处理。MulDiv 保留带符号最近舍入，除零或溢出作为成功路径返回 UINT_MAX，保留 LastError。

PE32 进程查询只借句柄视图、调度器与内存，OpenProcess 返回资源创建请求，由原入口分配句柄。固定 PID、access mask 和忽略 inherit flag 的策略保持。查询先检查句柄与权限，再检查输出；affinity 先检查两个输出，再按原顺序写入。SetThreadIdealProcessor 失败返回 UINT_MAX；未新增进程执行能力。

processor count 的 WORD carrier 由适配层消费，保持 Win64 trace 中原始 64 位参数。PE32 仍对完整 32 位 carrier 检查 0/0xffff，Win64 按原实现只消费低 WORD；两端成功均保留 LastError。group-count 两项在 PE32 仍为 ABI-only/LastError 120，Win64 返回单组 profile。API 返回槽别名仍分别使用 PE32 handler 后读取和 Win64 预先保存 frame 的顺序。

WMI 的内部 BSTR 分配继续使用同一 Bstrings32 owner 与进程 heap；去掉转借完整 ApiState 的 common_api facade 后，资源和错误顺序保持。common_api 的混合名称分发与模块已删除。
