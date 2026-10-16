# R2 Critical section（2026-10-16）

## 独立旧行为基线

起点 `e425e3b`，旧 executable 为 `r2-inline-sync-wire/wire`，SHA256 `8ad9ef1977f5d38c5c54379f90a4da7e26b54dce1bd79a54528a02e7a5c6718c`。范围八项 export：三种初始化、Enter/TryEnter/Leave/Delete、SetCriticalSectionSpinCount。PE32 SetSpinCount 保持 ABI-only，其余七项 typed；Win64 八项 typed。

迁移前八项回归通过（七项新增与原 Win64 Ex flags 回归）：PE32 24 字节布局、忽略 Ex flags、允许删除 held 状态、Leave 饱和、逐字段故障部分写入、获取先于返回槽故障、真实线程最终释放后的 Wake::Retest；Win64 DWORD 输入与旧 spin 64 位返回 carrier、非 owner 竞争、只读结构的 Try/Enter 差异、40 字节写入预检与返回槽别名；实际 guest 线程在 1/100000 work 切片下阻塞和重试，核对递归、owner 和完成记录。证据 `r2-critical-sections-thread-baseline-tests.txt`。

新增可选双架构 `gateway-critical-section` fixture，不改变默认集合。初始化一次，循环 TryEnter/Leave，校验成功数、释放后 recursion、退休指令和 API 次数。Win64 原版预检通过；PE32 首版将 int3 计入退休数而拒绝，修正计数后 IR/raw 均通过，保留失败证据。有效目录 `r2-critical-sections-fixture64-baseline`、`r2-critical-sections-fixture32-counted-baseline`。正式性能验收待实现后执行。
