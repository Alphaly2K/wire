# Interlocked 与 SLIST 边界

新 Wasm 入口在 PE32 和 AMD64 均提供八项服务，使用 kernel32/kernelbase 的统一调用帧；架构资格独立于共享目录的旧策略。调用不采样时钟，成功计一 work，不增加客体退休数，保留 LastError、FLAGS 与浮点状态。

| export | PE32 | AMD64 |
| --- | --- | --- |
| InterlockedIncrement / Decrement | LONG 按 32 位回绕，返回新值 | 相同 |
| InterlockedExchange | 写入 LONG 并返回旧值 | 相同 |
| InterlockedCompareExchange | 比较相等才写入，始终返回旧值 | 相同 |
| InitializeSListHead | 清零 8 字节，要求 8 字节对齐 | 清零 16 字节，要求 16 字节对齐 |
| InterlockedPushEntrySList / InterlockedPopEntrySList | 32 位 Next、16 位 Depth/Sequence | 完整指针、16 位 Depth、48 位 Sequence；保留 header 第二字的低四位 |
| InterlockedFlushSList | 返回旧 Next，清空 Next/Depth，Sequence 加一 | 相同，保留第二字的低四位 |

SLIST 的位布局与序列更新复用便携的 `win32/slist.rs`，内存适配直接使用新 VMM。Depth 和 Sequence 分别按字段宽度回绕，二者不互相进位。entry 与 header 要求相同架构对齐，push 拒绝 NULL entry；非空 pop 校验读出的后继对齐。空 pop 返回 NULL，不写 header，因此只需读取权限。

LONG 参数及返回保持 32 位位模式，AMD64 的高位参数不参与比较。比较失败的 CompareExchange 只要求读取权限；匹配后的写入仍检查写权限。初始化只写出完整 header，不读取原值。

两种 ABI 都在访问数据前保存参数和返回帧，输出覆盖原返回槽时仍使用保存的目标。push 在写 entry.Next 和 header 前预检两处写入，故障不发布部分链接；pop/flush 在完整输入读取与写入检查后提交。故障保留 CPU、work、退休数与 LastError。所有写入沿同一 VMM 源码失效路径撤销生成代码。

这些 API 在当前合作式客体调度中单次执行不让出线程，其间没有客体回调。当前实现没有并行运行客体线程，也未提供宿主共享内存的并行原子操作协议。未对齐或畸形 SLIST 链接保持原调用并报告明确接口错误，尚未投递相应的 Windows 对齐异常。
