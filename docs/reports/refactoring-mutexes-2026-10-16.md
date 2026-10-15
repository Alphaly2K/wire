# Mutex 契约迁移

## 独立行为基线

计划收敛七项 export：CreateMutexA/W、CreateMutexExA/W、OpenMutexA/W、ReleaseMutex。当前 PE32 只有普通 create/release 三项实现，四项 Ex/Open 保持 ABI-only；Win64 七项均 absent，不新增互斥量能力。

两项新增基线已在旧 handler 上通过。使用实际 guest 线程切换验证初始所有权、递归 acquire、同名 create 不转移所有权、非 owner 的 error 288、逐层 release 及后续线程 acquire；另固定 release 在返回槽 fault 前已提交，以及四项 ABI-only 的 error 120 和独立硬编码 stdcall 字节数。既有跨 event/mutex/semaphore 同名身份与 close 回归保留。

代码基线为 `f778193`，before 使用 `r2-sync-objects-wire/wire`，SHA256 `0fb177ed37d8dd3de9b68923127762cedfc87ebd924d9ca57bb9d94ed8075632`。日志为 `wire-out/refactoring-2026-10-14/r2-mutex-baseline-tests.txt`。

原创 `gateway-mutex` 先创建无 owner 的 mutex，循环零超时 acquire/release 并累加返回值，独立预期为 EAX=n、retired=9n+14、API calls=2n+1。12K 的 PE32 raw/IR smoke 在 before 上均通过；19 项工具检查通过。默认性能 case 不变；七项契约草稿尚未注册，运行时迁移未完成。
