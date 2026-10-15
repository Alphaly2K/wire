# PE32 TLS/FLS 状态归属

## 范围与基线

代码基线为 `50442ec` 的 TLS/FLS 契约迁移；before 使用 `r2-locals-wire/wire`，SHA256 `e471128c0d9ce664dcedf491b3874af335ad205f22190501a9b31b0741271a93`。

将 ApiState 的 tls/fls 两个 `(TID, slot) -> value` map 与两个递增计数器收进 `win32/kernel/locals32::LocalSlots32`，字段私有，只提供 allocate/get/set/free。LocalKind 显式区分两个池；所有方法只接收 kind、TID、索引和值，不借用 ApiState、CPU、guest 内存、调度器或 LastError。原 ApiState 构造与 dispatch 函数入口保持；旧四个公开内部字段不再暴露，仓库调用已统一到组件方法。

保留 PE32 的原策略：索引从 1 递增、不回收；未分配索引可读写；按当前 scheduler TID 隔离；free 删除所有 TID 的同一索引；TLS getter 的清错、FLS getter 的保错、callback 说明和返回槽 fault 前的资源提交仍由 adapter 按原顺序处理。没有新增资源上限、callback、原子事务或共享锁。Win64 的槽位与执行 body 保持。

## 验证与冻结

复用上一批先在旧 handler 验证的六项独立 local-slot 测试，以及既有真实调度/线程隔离测试；focused 全部通过。完整 release **448 passed / 7 ignored**、debug Win32/AMD64/FPU **296 passed**、显式 native **7 passed**、工具 **19 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。

候选冻结在 `r3-locals32-wire/wire`，SHA256 `677aef9afa0d232c365407de32cb1cfe8b89e24832b8512bfc5c31f6e645534f`，5,948,536 bytes，比 before 增加 **11,536 bytes**。在继续编辑下一组独立行为基线之前，已核对冻结 manifest 的 **233 个**已注册 Rust 文件，SHA 全部一致，记录为 `r3-locals32-source-verification.json`。

## 性能与接受

稳定启动协议下 **28 条**性能路径全部校验。PE32 TLS 网关 IR/raw 的 CPU 中位数变化 **+1.796%/+2.715%**，Win64 为 **+0.724%**。其余路径除下述 context/IR 初筛外，最大回退 **+2.446%**；RSS 中位数最多增加 **80KiB**。

context/IR 的 50M 初筛为 **+3.673%**，三对分别 −0.063%/+5.024%/+4.809%。扩到 **150M**，用同一 before executable 作为 control，按 after/before/control、before/control/after、control/after/before 三种顺序各跑一组。长样本中位数为 **−1.542%**，三组 −23.367%/+0.449%/−1.542%；control/before **+3.201%**。第一组 after 6.343s、before 8.277s、同哈希 control 9.262s，存在明显离散，全部保留，不据此宣称结构调整带来该幅度加速。长样本没有复现持续回退，也没有三组均超过 5%。初筛与扩样按不同 work 分开汇总，见 `r3-locals32-stable-final-metrics.json`。

Alice 200M 三对六轮均到相同预算出口，保持隐藏 314×211 窗口、零 Draw/Present；CPU 中位数 **−0.568%**、wall **+0.178%**。使用现有游戏 runtime 和 fixed-argv-game-v1，没有复制或硬链接素材，也没有进入实际绘制。证据在 `r3-locals32-stable-alice/`。

据完整正确性、28 条性能路径、context 顺序扩样与启动前缀接受本批。编译、正式计时和游戏运行串行；下一组事件/信号量的四项新增独立基线及双架构 event fixture 已在本批冻结运行时上通过，其契约草稿尚未注册，不计入本批运行时交付。全部原始日志继续保存在 `wire-out/refactoring-2026-10-14/`。
