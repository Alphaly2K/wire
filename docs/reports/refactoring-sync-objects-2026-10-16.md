# 事件与信号量契约迁移

## 独立行为基线

本批计划迁移 16 个 export：九个 Event 和七个 Semaphore。等待、mutex、计时器及调度控制接口保持 legacy，命名对象与句柄 owner 保持现状。基线 executable 复用 `r3-locals32-wire/wire`，SHA256 `677aef9afa0d232c365407de32cb1cfe8b89e24832b8512bfc5c31f6e645534f`。

四项新增测试先在原字符串 handler 上通过。PE32 固定 ReleaseSemaphore 的 count clamp、负数 release、输出 fault 前的资源提交、SetEvent 返回槽 fault 前提交、PulseEvent 清信号，以及六项 ABI-only export 的 error 120 与独立硬编码 stdcall 字节数。Win64 固定两个 kernel provider 的 ANSI/Ex 调用、DWORD 高位截断、共享对象/别名状态、previous-count 输出和 PulseEvent absent。原有 named/cross-kind/close、auto-reset/wait-all、访问权限及 Win64 输出 fault 不增计数等回归继续保留。

扩展原创 `gateway-event` 为双架构 fixture：创建一个 manual-reset event，循环 SetEvent/ResetEvent 并累加返回值。Win64 额外核对 API count，PE32 保留既有 raw/IR 协议。12K smoke 在原 executable 上三条路径均通过；默认 case 集合保持，未复制游戏或引入外部反汇编 fixture。

日志为 `wire-out/refactoring-2026-10-14/r2-sync-objects-baseline-{tests,amd64,pe32}.txt`。基线提交为 `04c84dd`，fmt 与 19 项工具检查也通过。

## 契约接入

16 项 provider、scalar ABI 与架构策略接入 SyncObjectApi。PE32 十项 typed、六项 ABI-only；Win64 十五项 typed、PulseEvent absent。删除两端重复的手写 ABI/arity/DWORD 与字符串 handler；Win64 typed adapter 委托原 Events owner，PE32 继续维护原 handle/name map 和调度 wake。两端的 error、命名冲突、权限、count、输出/返回 frame fault 顺序保持，详细边界见[事件与信号量](../architecture/sync-objects.md)。

四项 focused 回归、完整 release **452 passed / 7 ignored**、debug Win32/AMD64/FPU **300 passed**、显式 native **7 passed**、工具 **19 passed** 与严格 all-target/all-feature Clippy 通过。冻结 `r2-sync-objects-wire/wire`，SHA256 `0fb177ed37d8dd3de9b68923127762cedfc87ebd924d9ca57bb9d94ed8075632`，5,955,096 bytes，比 before 增加 **6,560 bytes**。在编辑下一组 mutex 基线前，已核对 **235 个**已注册 Rust 文件与冻结 SHA 一致。

## 性能与接受

稳定启动协议下 **31 条**双架构性能路径全部校验。Win64 event 网关的 CPU 中位数变化 **−21.989%**；PE32 同一 event fixture 的 IR/raw 为 **−0.673%/−0.227%**。这是固定网关循环的结果，未将其换算为游戏 FPS。其余路径除下述 module/raw 初筛外，最大回退 **+2.725%**；RSS 中位数最多增加 **256KiB**。

module/raw 的 50M 初筛为 **+3.743%**，三对 +6.206%/+3.818%/+1.981%。扩到 **150M**，加入与 before 相同 executable 的 control，按 after/before/control、before/control/after、control/after/before 各跑一组。长样本中位数为 **+0.527%**，三组 −1.495%/+0.527%/−0.508%；control/before **+0.070%**，after/control **+0.457%**。没有复现持续回退，初筛与全部扩样按不同 work 保留在 `r2-sync-objects-stable-final-metrics.json`。

Alice 200M 三对六轮均校验预算出口、隐藏 314×211 窗口与零 Draw/Present；CPU 中位数 **+0.306%**、wall **−0.337%**。复用原 runtime 与 fixed-argv-game-v1，没有复制或硬链接素材，未进入实际绘制。证据在 `r2-sync-objects-stable-alice/`。

fmt check、完整正确性、31 条路径、module 顺序扩样与启动前缀全部完成，接受本批。编译、正式计时和 native/game 运行保持串行。下一组 mutex 的两项新增基线已在旧 handler 上通过，未注册草稿不属于本批运行时交付。
