# WaitFor* 契约迁移

## 旧行为基线

四项 export 为 WaitForSingleObject/Ex、WaitForMultipleObjects/Ex。PE32 前三项 typed、MultipleEx 仍为 ABI-only；Win64 四项均支持 kernel32/kernelbase，USER 消息等待和 Winsock 等待不在本次契约范围。

运行时基线为 `df2d4ff`，冻结 before 为 `r2-mutex-wire/wire`，SHA256 `66ad662479909066aacc1642b25f4217a229bcdff30451d70a3e2518dd43739b`。三项新增独立测试在旧 handler 上通过：PE32 空数组、65→64 count clamp、MultipleEx error 120/20 字节 stdcall、SingleEx 忽略 alertable 及 acquire 在返回槽 fault 前提交；Win64 DWORD 截断、完整 64 位 handle、正确/错误 DLL、数组后续元素 fault 前不 acquire、不提交 CPU/API count。

既有 APC 用例扩为 SleepEx、SingleEx、MultipleEx 三种实际 guest 线程场景，以逐指令预算验证 callback 的 64 位参数、TID、线程 error mode 和返回 0xc0。旧网络 wait 回归也通过。日志为 `r2-waits-baseline-tests.txt` 和 `r2-waits-baseline-apc.txt`。

原创 `gateway-wait` 创建 manual/signaled event，循环零超时 Wait 并累加返回值。独立预期为返回 0、PE32 retired=6n+15、Win64 retired=6n+18/work=7n+19，两架构 API calls=n+1。before 的 12K Win64 与 PE32 raw/IR smoke 全通过，20 项工具检查通过。仅新增可选 case，默认测量集合不变；本批使用已修正的 event fixture，后续按实际输入哈希配对。

## 候选接入

四项接口已接入 WaitApi，删除重复 PE32 ABI、Win64 arity/DWORD 与 WaitFor* 名称路由。Win64 冷绑定到独立 ControlImport，single/multiple 和 alertable 由 ID 决定；旧 Winsock 分支与进程控制 body 保留。PE32 两个 typed guard 保留原 single/multiple body，未统一架构行为或新增 MultipleEx 实现。[调用边界](../architecture/waits.md)记录故障、阻塞与回调提交顺序。

31 项同步领域回归、完整 release **457 passed / 7 ignored**、debug Win32/AMD64/FPU **305 passed**、外部 native **7 passed**、工具 **20 passed**、fmt 与 strict all-target/all-feature Clippy 通过。冻结 `r2-waits-wire/wire`，SHA256 `006ec9bb9ffb0c85be19c42d39b9893e660ce31e12fc46d9ae7f9b9f74252bbe`，5,964,392 bytes，比 before 增加 **12,784 bytes**。继续准备下一批 Delay 基线前，冻结 manifest 的 **245 个 Rust 文件**（src/tests 为 **237 个**）全部核对一致，见 `r2-waits-source-verification.json`。

## 性能对照

稳定启动协议的 36 条路径全部结果校验：AMD64 integer/memory/SSE 700M、七项 gateway 100M、V2 FP/bit/code-data alias 300M；PE32 四类 CPU 300M raw/IR 和七项 gateway 50M raw/IR，各三对串行。原始轮次与汇总在 `r2-waits-stable-*` 和 `r2-waits-stable-initial-metrics.json`。Win64 Wait 网关 −2.072%，PE32 Wait IR/raw −1.941%/−0.219%，RSS 中位数最多增加 112KiB；未改 ISA/IR 热循环，普通 integer/IR 的 −10.084% 与明显离散样本全部保留，不归因为此次等待分发优化。

local/raw 初筛 +5.031%，三对分别 +6.335%/+1.573%/+2.137%，触发 150M 的 before/after/control 三标签顺序轮换调查。control 直接引用 before 路径，没有复制二进制。三个顺序全部结果校验；CPU 中位数 **+0.376%**，同哈希 control/before **+0.764%**，after/control **−0.385%**。首个 before 7.323s、其余 before 9.841–9.945s 的离散样本全部保留，没有过滤；不同 work 分开汇总。原始轮次为 `r2-waits-stable-local-investigation-order-{0,1,2}`，汇总为 `r2-waits-stable-expanded-metrics.json`。其余路径最大回退 +2.662%，扩样 RSS 中位数增加 48KiB。

Alice 200M work 的三对原始 guest Mono 启动前缀保持预算出口、隐藏 314×211 窗口、Draw/Present 0。CPU 中位数 2.604898s → 2.588449s（**−0.631%**），wall 2.667425s → 2.645057s（**−0.839%**），RSS 中位数减少 112KiB。沿用原 runtime，没有复制游戏；日志为 `r2-waits-stable-alice`。该前缀尚未进入实际绘制。

按既定行为、初筛和扩样门槛接受本候选。所有短样本、离散与长样本保留，不将固定网关变化推导为游戏 FPS 收益；未重排 APC/阻塞或扩大 API 实现范围。
