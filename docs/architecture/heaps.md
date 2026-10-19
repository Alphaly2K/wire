# Heap 调用与资源边界

13 项 heap export 的 provider、签名、实现策略与 HeapApi ID 集中在 `win32/catalog/heaps.rs`。PE32 九项 typed、四项 ABI-only；Win64 GetProcessHeap/HeapAlloc/HeapFree/HeapReAlloc/HeapSize 五项 typed，接受 kernel32/kernelbase，其余保持 absent。参数中 DWORD flags 保持 32 位，SIZE_T、地址及 handle 使用架构位宽。

PE32 adapter 位于 `win32/x86/heaps.rs`，只借 GuestHeap、内存、进程堆 handle 和 LastError；根 ApiState 继续拥有原 allocator，CRT、Local/Global、TLS 等原有用户共享该资源。保留私有堆最大容量、地址/空闲块复用、原 flags 子集和失败错误。GetProcessHeaps 仍返回一个进程堆，忽略输出访存故障，输出覆盖返回槽时仍按旧 gateway 读取新返回值。四项扩展沿用 error 120 和既定清栈，不新增实现。

Win64 `x64/runtime/heap.rs::Heap` 私有拥有 slab/block，并执行 allocate/free/call；算法不再借 Runtime64。`allocation::Pages` 显式借通用页游标、原分配记录、当前线程错误槽和 Trace；普通通用分配 facade 与 Heap 共用原 map→游标/记录提交和失败诊断→LastError 顺序。slab 归属、空块保留/回收和 checked memory 顺序保持：复用块先清零再登记，回收先 unmap 再删除记录，ReAlloc 保持复制/写入/释放顺序。只接受当前进程堆与 flags 0/1/8/9，invalid heap/flags 或 block 保持原故障。超 16MiB 请求沿用 error 8，SIZE_T 不截断；成功继续保留原 LastError。公共 gateway 先保存返回 frame，再执行 allocator，所以坏返回帧不会释放有效 block。

owner 提取基线 `1155a3e`，Heap 主体经显式依赖替换、页分配主体展开原 trace_note 后核对保持。完整 release Win64 269、fmt/release lib/bin strict Clippy/build 通过，不重复性能或游戏矩阵。证据 `wire-out/refactoring-2026-10-14/r3-heap64-owner-{source-checks,verification}.json`；通用页记录仍由组合根持有，其它分配领域不在本批合并。

Local/Global、CRT、原生 callback/COM 的分配路径仍沿用各自的既有资源约定。此批只收敛 heap 契约及调用边界，没有合并两套 allocator。旧行为及原创 HeapSize 负载见[验收记录](../reports/refactoring-heaps-2026-10-16.md)。
