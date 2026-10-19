# PE32 kernel 对象所有权

`win32::kernel::objects32::KernelObjects32` 私有拥有原混合 HANDLE 表、命名对象索引和递增分配位置。ApiState 保留公开 handles 视图、alloc_handle/create_named 兼容入口，HandleObj 的原公开路径通过 re-export 保留。对象表不再暴露原始 HashMap、命名索引或分配计数器；现有 get/get_mut、count 与 contains 查询继续可用。

命名 event/mutex/semaphore 继续共享原命名空间，保留跨类型同名返回已有 handle 的行为。复用时不执行 make closure，由 ABI facade 设置 LastError=183；新建时保持原 LastError。分配从 0x100 开始，每次加 4，单调分配及原溢出行为保持，没有更换为新 handle arena。

CloseHandle 由 owner 同时删除对象与指向该 handle 的名字，原删除/析构与名字清理顺序保持。FindClose 先检查 Search 类型，再执行独立移除，不与 CloseHandle 混用。wait/auto-reset/Mutex recursion/线程退出仍按原 gateway 和 scheduler 顺序执行，owner 没有持有 CPU、内存或 LastError。

文件、搜索、event/mutex/semaphore、线程与进程仍是原 HandleObj 变体。进程查询 adapter 只借 KernelObjects32 和 scheduler；[文件 adapter](files.md)随后移入架构目录，仅借 filesystem/对象/错误槽及 GuestMem。Win64 独立 handle owner 保持。

基线 `b5182db`。原对象变体、分配、同名逻辑、gateway 和进程查询主体归一核对保持。release Win32 248、线程间 owner 迁移 1 项，fmt/release lib/bin strict Clippy/build 通过；现有独立测试覆盖共享名字、类型拒绝、关闭寿命及返回栈 fault 前的对象提交。没有新增与实现镜像的测试或重复性能矩阵。证据位于 `wire-out/refactoring-2026-10-14/r3-objects32-{source-checks,verification}.json` 及 validation 日志。
