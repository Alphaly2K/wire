# 生命周期绑定的共享 byte buffers

`native-mono-backend` 默认关闭。`src/mono/buffers.rs` 提供受控引擎缓冲区 arena，尚未绑定
Unity allocator vtable，也没有映射一般托管 heap。它是 NativeArray/unsafe IL 原始缓冲区接入的
基础；不能把任意 Mono/GC malloc 整体改用它，再让未适配 native helpers 绕过 lease 写入。

每个 allocation 使用明确 power-of-two 对齐（最大4096）、zeroed owned HostRegion，canonical
地址与 native data pointer 相同，注册范围仅含实际 bytes，禁止 execute。BufferHandle 使用
进程级不复用序号，不能跨 arena 释放；每个映射检查确切 region owner，传错 Memory 在任何
操作前失败。native view 访问持 VM CallLease 和 region lease，不返回 borrowed byte slice。

实际 backing owner 持 SharedBacking ResourceLease，而不是把 lease 放在可提前 drop 的 handle
表中。因此 revoked mapping/native view 尚保留 storage 时仍阻止 VM cleanup。free 可以在
Failed/Stopping 期间执行，先 revoke 整个 backing、再移除全部已知 aliases；arena 被误 drop
时其余 Memory mappings 一律 revoked，仍保留真实 storage/VM lease，直到最后 owner 被释放。

reallocate 先核对旧 mapping ownership和容量，受检读取需保留 bytes，准备/复制新allocation后
才退休旧地址、aliases和token。失败保留旧内容/handle；growth尾部为0，alignment保留，
新旧共存瞬间另受 Memory 容量限制。arena容量不是无界分配；零长度/错误alignment显式拒绝，
尚未实现Unity对malloc(0)/calloc overflow等不同API的封送语义。普通buffer free/realloc不是
managed object释放，也不表示任何引用槽可以当普通bytes覆盖。

五项Rust行为测试验证真实native/guest/alias互见、精确bounds/execute拒绝、失败realloc、
readonly/one-shot guard、旧token/view/aliases退休、传错Memory/arena、并发native access以及
shutdown等到actual storage owner消失。实际ARM64 Mono checked IL在65537B buffer中逐byte
写入/求和，native独立核对所有bytes，guest alias互见；grow到131071B后数据正确且尾部zero。
readonly/bounds/stale pointer均返回AccessViolationException，failed realloc保留旧buffer，
retire后native view仍阻止cleanup直到drop。不是只测试state计数。

最新完整M2在noexec guard下通过（checks=540、denied=0）。fixture使用synthetic lifecycle
admission，仅为验证这些owners；真实game profile仍original_x64、M3/backend均false。
原有reference/array/domain/delegate/线程/Win32服务/metadata回归仍通过。完整feature测试和
feature Clippy结果保留于ignored wire-out日志；该模块不替代生产GC roots和scheduler。

下一步把暴露给guest的managed对象的临时强root、暂停寄存器/stack/static/heap root snapshots
接入实际owner生命周期，再将Unity callbacks/typed exports接provider。注册整个native heap为
永久root，或只在一个fixture里保存object，不足以满足生产GC ownership。
