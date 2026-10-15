# Waitable timer 契约迁移

## 独立旧行为基线

九项 export 为 CreateWaitableTimerA/W、CreateWaitableTimerExA/W、OpenWaitableTimerA/W、SetWaitableTimer/Ex、CancelWaitableTimer。运行时基线 `d5d484f`，before 为 `r2-delays-wire/wire`，SHA256 `6b0f99d96603c04488a2f751274ce04d7b54a48cd55abc2d0fef92088b6ff621`。PE32 九项均为 Wine ABI 已知但未实现；Win64 继续支持 kernel32/kernelbase 的原 timer 子集。

两项新增基线在旧 handler 上通过：PE32 固定全部九项 error 120、独立硬编码清栈字节及输出不变；Win64 覆盖 A/W 命名与 create/open/extended alias access、DWORD 截断与完整指针、权限和既有 LastError，以及 due-time 访存故障先于 invalid handle/period/APC 检查且不提交 CPU/API count。既有 manual/auto、consume/cancel、deadline、重复命名对象 wait-all 与权限回归保留。日志 `r2-timers-baseline-tests.txt`。

原创可选 `gateway-timer` 只循环取消未 arm 的 timer，累加 BOOL 返回 n，不依赖到期时间。独立预期 n=work//6、retired=5n+17、work=6n+18、API calls=n+1；独立 PE inventory 核对恰好两项导入和原 IAT 地址。before 的 Win64 12K smoke 通过，20 项工具检查通过；默认 case 集合保持。

## 候选接入

九项接口接入 TimerApi；PE32 全部 ABI-only，Win64 typed。删除原 timer arity/DWORD 表和 group 15 名称路由，普通 catalog scalar 路由按 ID 执行原 body。A/W/create/open/extended 来自 ID，Events 状态 owner 与 deadline、权限、错误和故障提交顺序保持，见[调用边界](../architecture/timers.md)。

首次编译在 native module 的 GetProcAddress 特例中发现残留 `timers::arity` 引用，失败日志 `r2-timers-focused-tests.txt` 保留。移除这份重复名单后，由既有 kernel/catalog lookup 契约处理；同步领域和模块领域复测通过，日志分别为 `r2-timers-focused-tests-fixed.txt`、`r2-timers-module-tests.txt`。

完整 release/debug/native 行为检查通过后，strict Clippy 又捕获同处残留的 unused import。只删除该 import，未更改 body；后续 fmt/Clippy 使用 `r2-timers-*-final.txt` 日志，原失败日志保留，行为测试无需为这一删除重跑。

35 项同步、12 项模块领域回归、完整 release **461 passed / 7 ignored**、debug Win32/AMD64/FPU **309 passed**、外部 native **7 passed**、工具 **20 passed** 通过；最终 fmt、strict all-target/all-feature Clippy 与 release build 通过。冻结 `r2-timers-wire/wire`，SHA256 `cf7f8d6f6bf3f24c48086bad3d8569fd89a11f591123d15a8513731a33488027`，5,965,464 bytes，比 before 增加 **368 bytes**。准备下一批虚拟内存基线前，manifest 的 **247 个 Rust 文件**（src/tests **239 个**）全部核对一致，见 `r2-timers-source-verification.json`；后续草稿与测试不混入本候选。

## 性能与接受决定

25 条相关路径、各三对全部结果校验：AMD64 CPU 700M、六项 gateway 100M、V2 code/data alias 300M，PE32 四类 CPU 300M raw/IR 与 delay/wait/local 三项 gateway 50M raw/IR。稳定启动、utility/discard 和输入哈希协议保持，汇总为 `r2-timers-stable-initial-metrics.json`。最大 CPU 中位数回退 **+2.525%**（PE32 FP/raw），RSS 中位数最多增加 **128KiB**，未触发扩样门槛。Win64 timer 取消网关 **−18.748%**；普通 PE32 integer/SSE IR 的大幅下降与离散轮次全部保留，未改 ISA/IR 热循环，不归因为此次 timer 迁移。

Alice 200M work 的六轮原始 guest Mono 启动前缀保持预算出口、隐藏 314×211 窗口、Draw/Present 0。CPU 中位数 2.597193s → 2.604198s（**+0.270%**），wall 2.658017s → 2.661562s（**+0.133%**），RSS 约增加 48KiB。沿用原 runtime，没有复制游戏，日志为 `r2-timers-stable-alice`；该前缀尚未进入实际绘制。

按既定行为和性能门槛接受本候选，不扩大 timer 实现范围，固定网关变化不推导为游戏 FPS 收益。
