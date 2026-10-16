# Heap 调用与资源边界

13 项 heap export 的 provider、签名、实现策略与 HeapApi ID 集中在 `win32/catalog/heaps.rs`。PE32 九项 typed、四项 ABI-only；Win64 GetProcessHeap/HeapAlloc/HeapFree/HeapReAlloc/HeapSize 五项 typed，接受 kernel32/kernelbase，其余保持 absent。参数中 DWORD flags 保持 32 位，SIZE_T、地址及 handle 使用架构位宽。

PE32 adapter 位于 `win32/x86/heaps.rs`，只借 GuestHeap、内存、进程堆 handle 和 LastError；根 ApiState 继续拥有原 allocator，CRT、Local/Global、TLS 等原有用户共享该资源。保留私有堆最大容量、地址/空闲块复用、原 flags 子集和失败错误。GetProcessHeaps 仍返回一个进程堆，忽略输出访存故障，输出覆盖返回槽时仍按旧 gateway 读取新返回值。四项扩展沿用 error 120 和既定清栈，不新增实现。

Win64 原 slab/block allocator 留在 `x64/runtime/heap.rs`，按 HeapApi ID 调用；原通用页分配记录、slab 归属、空块保留/回收和 checked memory 顺序保持。只接受当前进程堆与 flags 0/1/8/9，invalid heap/flags 或 block 保持原故障。超 16MiB 请求沿用 error 8，SIZE_T 不截断；成功继续保留原 LastError。公共 gateway 先保存返回 frame，再执行 allocator，所以坏返回帧不会释放有效 block。

Local/Global、CRT、原生 callback/COM 的分配路径仍沿用各自的既有资源约定。此批只收敛 heap 契约及调用边界，没有合并两套 allocator。旧行为及原创 HeapSize 负载见[验收记录](../reports/refactoring-heaps-2026-10-16.md)。
