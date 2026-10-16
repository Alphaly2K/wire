# R2 Critical section（2026-10-16）

## 独立旧行为基线

起点 `e425e3b`，旧 executable 为 `r2-inline-sync-wire/wire`，SHA256 `8ad9ef1977f5d38c5c54379f90a4da7e26b54dce1bd79a54528a02e7a5c6718c`。范围八项 export：三种初始化、Enter/TryEnter/Leave/Delete、SetCriticalSectionSpinCount。PE32 SetSpinCount 保持 ABI-only，其余七项 typed；Win64 八项 typed。

迁移前八项回归通过（七项新增与原 Win64 Ex flags 回归）：PE32 24 字节布局、忽略 Ex flags、允许删除 held 状态、Leave 饱和、逐字段故障部分写入、获取先于返回槽故障、真实线程最终释放后的 Wake::Retest；Win64 DWORD 输入与旧 spin 64 位返回 carrier、非 owner 竞争、只读结构的 Try/Enter 差异、40 字节写入预检与返回槽别名；实际 guest 线程在 1/100000 work 切片下阻塞和重试，核对递归、owner 和完成记录。证据 `r2-critical-sections-thread-baseline-tests.txt`。

新增可选双架构 `gateway-critical-section` fixture，不改变默认集合。初始化一次，循环 TryEnter/Leave，校验成功数、释放后 recursion、退休指令和 API 次数。Win64 原版预检通过；PE32 首版将 int3 计入退休数而拒绝，修正计数后 IR/raw 均通过，保留失败证据。有效目录 `r2-critical-sections-fixture64-baseline`、`r2-critical-sections-fixture32-counted-baseline`。

## 实现与正确性

基线提交 `6189e78`。八项 export 接入 CriticalSectionApi，删除 PE32 重复 ABI、Win64 arity/DWORD 与双方名称分支。两个架构的 adapter 分别持有 guest 结构 codec，窄借内存和当前 tid，保留原有字段顺序、权限、flags、归属与算术行为；阻塞、单个等待者唤醒、CPU 返回及 trace/API count 提交仍由进程负责。PE32 guard 追加在已有 scalar typed 家族之后。

领域八项回归与完整矩阵通过：release **486 passed / 7 ignored**，debug **333 passed**，显式 native **7 passed**，工具 **20 passed**；fmt、strict all-target/all-feature Clippy、release build 通过。冻结 `r2-critical-sections-wire/wire`，SHA256 `8dec5f742f5673856f6e2633423bef15fdaba0351b039a675b639f1b81281a66`，**5,995,720 bytes**，减少 **976 bytes**；273 个 Rust 文件（src/tests 265 个）全部与 manifest 哈希核对一致，证据 `r2-critical-sections-source-verification.json`。

## 性能与接受决定

稳定启动协议、三对轮换、39 条路径全部结果校验。critical section 网关 CPU Win64 **−3.853%**、PE32 IR/raw **−1.418%/−0.813%**；矩阵最大回退 **+2.234%**（data-alias），RSS 最多增加 **128KiB**，没有超过 3% 的路径。汇总 `r2-critical-sections-stable-summary.json`。

Alice 200M 三对均通过，CPU **−0.149%**、wall **−0.171%**，保持预算出口、隐藏 314×211 窗口及 Draw/Present 0，证据 `r2-critical-sections-stable-alice`。编译、正式计时和 native 游戏串行，直接复用原 runtime。此启动前缀未进入实际绘制。

2026-10-17 接受本批，契约累计覆盖 **301 个 export**。自动审批的若干启动调用因超时未执行，拆成较短串行调用后完成全部验证，没有将未执行检查计作通过。下一批继续地址等待。
