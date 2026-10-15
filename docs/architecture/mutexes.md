# Mutex 调用边界

七项 export 的 provider、scalar ABI 与执行策略集中在 `win32/catalog/mutexes.rs`。PE32 的 CreateMutexA/W、ReleaseMutex 绑定 MutexApi；CreateMutexExA/W、OpenMutexA/W 维持 ABI-only 的 error 120 和原 stdcall 清栈。Win64 七项均 absent。未迁移等待接口，未新增 mutex 能力。

PE32 保留 ApiState 的共享 handle/name map 与 scheduler TID。新建时按初始所有权设置 count；同名 create 复用已有 handle，不授予新的所有权，也保留跨 event/semaphore 类型的历史名称策略。Wait 的递归 acquire 继续由原等待 helper 执行。

ReleaseMutex 只接受当前 owner，非 owner 返回 0/error 288，无效或错类型 handle 返回 0/error 6。递归 count 降到零后先清 owner、唤醒一个 waiter，再由公共 gateway 提交 EAX、读取返回地址并同步 TEB error；返回槽 fault 可发生在释放之后。成功不额外清错，没有引入宿主锁或重排 wake/frame 顺序。

独立测试通过实际 guest 线程切换检验所有权和逐层释放，另固定故障提交、ABI-only 字节数与 Win64 absent。原创 `gateway-mutex` 循环零超时 acquire/release，检查 EAX=n、retired=9n+14、API calls=2n+1。基线与迁移验收见[本批记录](../reports/refactoring-mutexes-2026-10-16.md)。
