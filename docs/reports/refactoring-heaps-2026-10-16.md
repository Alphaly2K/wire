# Heap 契约迁移

## 独立旧行为基线

范围为 GetProcessHeap、HeapCreate/Destroy/Alloc/Free/ReAlloc/Size/Validate、GetProcessHeaps，以及 PE32 ABI-only 的 HeapSetInformation/HeapQueryInformation/HeapCompact/HeapWalk，共 13 项。运行时基线 `21f7b74`，before 为 `r2-vmem-wire/wire`，SHA256 `91ea119a86746258c54dc19bd9582411c14282748c3a747a824dfc4481553568`。Win64 只保留原 GetProcessHeap 与 Alloc/Free/ReAlloc/Size 五项、kernel32/kernelbase provider，其余继续拒绝；双架构 allocator 和原资源归属保持。

两项新增旧行为基线通过：PE32 固定私有堆容量/归属、失败 LastError、成功保持旧错误、GetProcessHeaps 的单堆结果与忽略输出故障、输出覆盖返回槽，以及四项 ABI-only 的独立清栈和输出不变；Win64 固定完整 handle/SIZE_T、DWORD flags、非法 heap/flags 与坏返回帧在 HeapFree 前失败且不提交 CPU/API count、未迁移实现范围。既有相关八项 heap 回归通过，日志 `r2-heaps-baseline-tests.txt`、`r2-heaps-baseline-related-tests.txt`。

原创可选 `gateway-heap` 先获取进程堆并分配 23 字节，再循环 HeapSize 累加每次原始分配长度。n=max(1, work//8)，两端返回 23n、API calls=n+2；Win64 retired=7n+21、work=8n+23，PE32 retired=7n+18。独立 PE inventory 核对三个导入及 IAT 地址。Win64 12K smoke 首次通过；PE32 首次预期多计了不退休的终止 INT3，修正预期后在原 PE 字节上通过 raw/IR smoke。失败日志 `r2-heaps-baseline-pe32-smoke.txt` 保留；默认 case 集合保持。
