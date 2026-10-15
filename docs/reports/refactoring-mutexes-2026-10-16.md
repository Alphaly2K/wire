# Mutex 契约迁移

## 独立行为基线

计划收敛七项 export：CreateMutexA/W、CreateMutexExA/W、OpenMutexA/W、ReleaseMutex。当前 PE32 只有普通 create/release 三项实现，四项 Ex/Open 保持 ABI-only；Win64 七项均 absent，不新增互斥量能力。

两项新增基线已在旧 handler 上通过。使用实际 guest 线程切换验证初始所有权、递归 acquire、同名 create 不转移所有权、非 owner 的 error 288、逐层 release 及后续线程 acquire；另固定 release 在返回槽 fault 前已提交，以及四项 ABI-only 的 error 120 和独立硬编码 stdcall 字节数。既有跨 event/mutex/semaphore 同名身份与 close 回归保留。

代码基线为 `f778193`，before 使用 `r2-sync-objects-wire/wire`，SHA256 `0fb177ed37d8dd3de9b68923127762cedfc87ebd924d9ca57bb9d94ed8075632`。日志为 `wire-out/refactoring-2026-10-14/r2-mutex-baseline-tests.txt`。

原创 `gateway-mutex` 先创建无 owner 的 mutex，循环零超时 acquire/release 并累加返回值，独立预期为 EAX=n、retired=9n+14、API calls=2n+1。12K 的 PE32 raw/IR smoke 在 before 上均通过；19 项工具检查通过。默认性能 case 不变。

## 契约接入

七项接口已接入 MutexApi，删除 PE32 重复手写 ABI 与名称 handler。三项 typed、四项 ABI-only，Win64 保持 absent；原 handle/name map、scheduler TID、递归 count 和 wake 顺序保持。适配器按 ID 选择 A/W，执行阶段不解析函数名；ABI-only 继续由公共 legacy policy 处理。[调用边界](../architecture/mutexes.md)记录错误和返回槽 fault 的提交顺序。

两项 focused 回归、完整 release **454 passed / 7 ignored**、debug Win32/AMD64/FPU **302 passed**、显式 native **7 passed**、工具 **19 passed**、fmt 与 strict all-target/all-feature Clippy 通过。冻结 `r2-mutex-wire/wire`，SHA256 `66ad662479909066aacc1642b25f4217a229bcdff30451d70a3e2518dd43739b`，5,951,608 bytes，比 before 减少 **3,488 bytes**；继续编辑下一组基线前，**236 个** Rust 文件已核对与冻结 SHA 一致。

## 性能与接受决定

稳定启动协议下的 33 条路径、各三对全部结果校验：AMD64 CPU 700M、六项 gateway 100M、V2 FP/bit/code-data alias 300M，PE32 CPU 300M raw/IR 与六项 gateway 50M raw/IR。最大 CPU 中位数回退为 module/raw **+1.712%**；mutex IR/raw 为 **+0.714% / +0.681%**，RSS 中位数最多增加 **64KiB**。没有路径达到 3% 的初筛调查门槛。个别普通 PE32 CPU 循环出现 −9% 至 −19% 的下降，全部样本保留；此次未改 ISA/IR 热循环，不将这些变化归因为 mutex 优化。

六轮 Alice 200M work 启动前缀均保持预算出口、隐藏 314×211 窗口、Draw/Present 0。CPU 中位数 2.614348s → 2.671959s（**+2.204%**），wall 2.679146s → 2.756107s（**+2.873%**），RSS 约增加 208KiB。沿用原 runtime，没有复制游戏；该前缀尚未实际绘制。按行为与性能门槛接受本候选。

全部证据位于 `wire-out/refactoring-2026-10-14/`：`r2-mutex-stable-initial-metrics.json` 保存 33 条路径及所有轮次；`r2-mutex-stable-alice/compare.json` 保存游戏前缀；`r2-mutex-source-verification.json` 保存冻结源码核对。下一批 Wait 基线测试在冻结后准备，未混入本次运行时验收。

本批静态审计另发现 AMD64 event fixture 的 OFT/IAT 重叠，生成两项重复导入。冻结输入的真实循环、返回值、work/retired/API 次数校验均通过，本次保留原输入与证据；后续单独修正 fixture，并按新哈希划分后续测量。
