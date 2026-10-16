# Local/Global 内存调用边界

Local/GlobalAlloc、ReAlloc、Free、Size、Lock、Unlock、Flags 共 14 个 export，现有 Local/Global 成对入口共用相同行为和资源。在 `win32/catalog/local_memory.rs` 中声明唯一 provider、签名、实现策略与七个 LocalMemoryApi ID；trace 与动态查询继续保存原 export 名称。该家族与 TLS/FLS 的 LocalApi 分开。

PE32 的 Alloc/Free/Size 使用原进程 GuestHeap，与 HeapAlloc、CRT、FormatMessage 共享；Lock 直接返回输入，Unlock 返回零，普通成功及分配失败保持 LastError。Alloc 继续拒绝 movable bit，保留其它原 flags 行为。ReAlloc/Flags 四项维持 ABI-only error 120、原清栈和输出不变。适配器只借内存、allocator 和进程堆 handle，不持有总 ApiState。

Win64 维持 allocations 页记录与 local_blocks 归属集合；接受 kernel32/kernelbase 全部 14 项。Alloc flags 子集为 0/0x40，ReAlloc 子集为 0/2/0x40/0x42；DWORD flags 截断，SIZE_T、地址和失败 Free 返回值保持完整 64 位。保留原块内扩展、零填充、允许移动时的 copy/map/unmap 顺序、16MiB 限制、invalid handle error 6、不支持 flags error 50、有效 Unlock error 158 及 invalid Flags 返回 0x8000。

公共 Win64 gateway 在 Free/ReAlloc 前保存返回 frame；坏返回帧不会先释放 block。原 Local/Global allocator 与 guest slab heap 的资源归属保持，原 FormatMessage、CommandLineToArgvW 等生产的 local block 继续可由 Free 释放。PE32 与 Win64 的锁定、失败输出和错误约定分别保留。

PE32 adapter 位于 `win32/x86/local_memory.rs`；Win64 原 body 位于 `x64/runtime/local_memory.rs`，按 typed ID 选择分支，删除名称及后缀解析。独立旧行为基线及共享资源循环见[验收记录](../reports/refactoring-local-memory-2026-10-16.md)。
