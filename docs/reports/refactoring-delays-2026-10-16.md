# Sleep 与线程让出契约迁移

## 旧行为基线

三项 export 为 Sleep、SleepEx、SwitchToThread；PE32 只接受 kernel32，Win64 接受 kernel32/kernelbase。运行时基线 `80be38b`，before 为 `r2-waits-wire/wire`，SHA256 `006ec9bb9ffb0c85be19c42d39b9893e660ce31e12fc46d9ae7f9b9f74252bbe`。

两项新增独立测试在旧 handler 上通过：PE32 返回槽 fault 时 EAX 已写、线程尚未让出、frame 和 LastError 保持，以及 SleepEx 忽略 alertable；Win64 DWORD 高位截断、Sleep/SleepEx/SwitchThread 的不同 control、正常返回 frame/计数、返回槽 fault 前 CPU/API count 均未提交和 wrong DLL 拒绝。既有实际线程 zero-delay yield、三种 APC callback 及带预算暂停的 timed sleep/timeout 测试也通过。日志前缀 `r2-delays-baseline-`。

原创可选 `gateway-delay` 循环调用 SleepEx(0,0)，返回累加 0。PE32 独立预期 n=work//6、retired=6n+7、API calls=n；Win64 n=work//7、retired=6n+10、work=7n+10、API calls=n。before 的 12K 双架构与 PE32 raw/IR smoke 全通过，20 项工具检查通过；默认 case 集合不变。

## 候选接入

三项接口已接入 DelayApi，删除 PE32 重复 ABI 与名称分支、Win64 重复 arity/DWORD 与名称控制分支。Win64 冷绑定 ControlImport，三种原控制 body 和 trace/counter 顺序保持；PE32 只切换 guard，原 wake、deadline、yield-after-return 和 ignored alertable 保留。[边界说明](../architecture/delays.md)记录架构差异。

33 项同步领域回归、完整 release **459 passed / 7 ignored**、debug Win32/AMD64/FPU **307 passed**、外部 native **7 passed**、工具 **20 passed**、fmt 与 strict all-target/all-feature Clippy 通过。冻结 `r2-delays-wire/wire`，SHA256 `6b0f99d96603c04488a2f751274ce04d7b54a48cd55abc2d0fef92088b6ff621`，5,965,096 bytes，比 before 增加 **704 bytes**。冻结 manifest 的 **246 个 Rust 文件**（src/tests **238 个**）全部核对一致，见 `r2-delays-source-verification.json`。

本批性能对照选择 24 条相关路径：AMD64 integer/memory/SSE 700M，delay/wait/local/thread-ID/clock 五项 gateway 100M，V2 code/data alias 300M；PE32 四类 CPU 300M raw/IR 与 delay/wait/local 三项 gateway 50M raw/IR。全部各三对，沿用稳定启动协议，PE32 utility/discard 策略保持。覆盖新增控制、刚迁移的等待、既有 TLS/hot route、解释器和缓存；本批不改状态 owner 或其它资源 handler。

## 性能与接受决定

24 条路径全部结果校验，最大 CPU 中位数回退 **+1.063%**（PE32 bit/raw），RSS 中位数最多增加 **16KiB**，未触发扩样门槛。Delay 网关 Win64 **−0.717%**，PE32 IR/raw **−3.387%/−1.001%**；Wait 和 local 的交叉对照均低于 1.1% 回退。PE32 integer/IR 的 −11.071% 与离散轮次全部保留，未改 ISA/IR 热循环，不归因为调度优化。日志为 `r2-delays-stable-*`，汇总为 `r2-delays-stable-initial-metrics.json`，没有跨批次或输入合并。

Alice 200M work 的六轮原始 guest Mono 启动前缀保持预算出口、隐藏 314×211 窗口与 Draw/Present 0。CPU 中位数 2.590229s → 2.598644s（**+0.325%**），wall 2.652490s → 2.656970s（**+0.169%**），RSS 约增加 208KiB；沿用原 runtime，没有复制游戏。该前缀尚未进入实际绘制，日志为 `r2-delays-stable-alice`。

按既定行为和性能门槛接受本候选；没有更改调度/回调语义或扩大 API 能力。固定网关的变化不推导为游戏 FPS 收益。
