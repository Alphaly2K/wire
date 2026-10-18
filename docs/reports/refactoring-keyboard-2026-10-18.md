# R2 键盘状态与布局（2026-10-18）

状态：2026-10-18 接受，实现提交 `386f38a`，独立基线提交 `453d2a8`。

## 独立基线

before 为基础 helper 已验收冻结 `r2-basic-fields-wire/wire`，SHA256 `b6ac77e4eff26b9dc39540dfa69ee823383728f2105452db5f700b5ef4804b79`，5,917,192 bytes。范围为三个状态 API 与十三个布局/type/map/key-name/Unicode API。

四项独立回归在旧名称分发上通过：PE32 十三项 layout 继续 ABI-only、LastError 120、无输出及原 stdcall 清栈；状态 API 保留 LastError、handler 后读取返回槽、跨页输出 fault 与错 provider 的 unknown ABI。Win64 固定状态 API 在 kernel32/kernelbase/user32 的静态路由，但动态查询仅 user32；DWORD 高位截断、saved return slot、Unicode signed 容量优先于 flags/HKL/指针验证、完整 HKL 与输入/输出别名均固定。

首轮 PE32 测试直接修改 ApiState.last_error，遗漏入口从 TEB 重读，预期失败。改由 guest SetLastError 初始化后通过，运行时行为未改。两份日志保留为 `r2-keyboard-baseline-tests.txt` 和 `r2-keyboard-baseline-tests-corrected.txt`。

原创可选 `gateway-keyboard`（双架构）与 `gateway-keyboard-map`（Win64）在旧冻结上完成 12K 预检，checksum、退休数、API 次数及 Win64 work 全部核对，PE32 raw/static IR 均验证；证据 `r2-keyboard-fixture-baseline64/32`。输出使用 stack buffer，不写 fixture 代码页，不注入宿主按键。工具 20 项通过，默认 case 集合保持。

## 实现与验收

16 项 export 汇入 KeyboardApi，参数、provider、架构执行策略与动态 lookup 差异统一声明。ApiId 追加在末尾，保留原变体顺序。PE32 三项状态适配器只借内存、Keyboard owner 和 tid，经 User32 的窄方法转借，状态字段保持私有；十三项布局 API 保留 ABI-only。Win64 三项状态和十三项布局走 typed adapter，删除旧键盘 arity/DWORD/provider 特例和消息名称分支；不引入逐调用名称解析。

纯 scan/VK、character 和 key-name 核移至 `win32/user/keyboard_layout.rs`，与旧实现逐段比较一致，保留 Hangover/Wine 来源和 LGPL 声明。原 host 输入、按线程消息状态、异步低位消费和 Unicode 文字队列的 Keyboard owner 不变。已有 Unicode unit test 随 adapter 移动，消息集成回归改调用 typed 状态方法；PE32 menu 中四项重复未实现路由删除。稳定边界见[键盘架构](../architecture/keyboard.md)。

候选双架构领域回归 PE32 **177**、Win64 **198** 项通过，包含四项独立基线。完整 release **537 passed / 7 ignored**，debug Win32/AMD64/FPU **384 passed**，显式 native **7 passed**，工具 **20 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。

冻结 `r2-keyboard-wire/wire`，SHA256 `40d21a10e33677a7410e09255b83cc2e42960fde87797f8d3eeeedd5ca40dea5`，**5,920,120 bytes**，较 before **+2,928 bytes**，相对 before 的 16KiB 增量预算内。**320 个 Rust 文件**（src/tests **312**）与活动源码、快照逐项核对一致，target/frozen executable 哈希一致，证据 `r2-keyboard-source-verification.json`。

正式性能范围为基础 PE32 raw/static IR 与 AMD64 integer/memory/SSE/FP/bit/cache-alias，加新键盘网关、既有热 gateway 和 GDI/processor/GUID 代表路径；各至少三对，超门槛独立调查。真实游戏前缀复用已有 runtime。编译、正式计时、native 游戏及 runtime diagnostics 严格串行，性能与游戏结果另行记录。

## 性能与接受决定

**28 条初筛路径**全部三对结果校验，CPU 中位数最大回退 **+2.652%**（PE32 bit/raw），RSS 中位数最多 **+96KiB**。AMD64 integer/memory/SSE 为 **−0.222%/−1.844%/−4.699%**，FP/bit/code-alias/data-alias 为 **+0.475%/−0.504%/+1.720%/−2.214%**。Win64 新状态/布局网关 **+0.954%/−2.034%**；PE32 状态 IR/raw 初筛 **−15.165%/−5.475%**，其中 IR 的有利离散另作扩样，不将初筛 −15% 表述为稳定收益。

PE32 integer/IR、SSE/IR 因单轮 **+10.268%/+9.652%** 扩为 900M；GUID/raw 因单轮 **+5.262%** 扩为 150M；新 keyboard/IR 因上述改善幅度离散扩为 150M。每项均加入同哈希 control，三种顺序各一对：

| 路径 | work | after/before CPU | control/before CPU | after/control CPU |
| --- | ---: | ---: | ---: | ---: |
| PE32 integer/IR | 900M | +3.102% | −0.173% | +3.280% |
| PE32 SSE/IR | 900M | −0.906% | −0.088% | −0.819% |
| PE32 keyboard/IR | 150M | −7.217% | −1.732% | −5.582% |
| PE32 GUID/raw | 150M | −0.026% | −0.441% | +0.417% |

integer 三对分别 **−8.689%/+4.856%/−4.154%**，CPU 的跨轮中位数为 after **3.910674 秒**、before **3.793023 秒**、control **3.786468 秒**；首轮同哈希 before/control 为 3.2254/3.7405 秒，保留该明显漂移。记录 +3.102% 的中位数成本及全部逐对结果，没有三对均超过 5% 的拒绝路径。

独立离线调查核对九次长样本 report：IR **900,000,003**、fallback **1**、CALL_NATIVE **0**、scalar runs **149,999,714**、段内指令 **899,997,142** 及全部最终 GPR/EIP/EFLAGS 一致。冻结之间 **16 个 src/x86 Rust 文件**哈希未变。按 Mach-O addr/size 限定反汇编范围，ir32/mix32/raw32/0f32 四节的指令数与助记符序列保持；simd32 为 **1664→1736**，助记符序列变化。这里只比较助记符，未核对全部操作数，整数成本的具体机制尚未确证。证据 `r2-keyboard-integer-investigation/summary.json`，没有另起 diagnostics 与正式计时重叠。

新 keyboard/IR 的三对长样本 **−5.531%/−8.711%/−6.581%**，改善范围较初筛收敛。GUID/raw 最后一组 before/after 约为 9.3974/9.3061 秒，control 7.8543 秒；全部保留，不丢弃慢轮。不同 work 独立汇总，最终 **32 个 workload protocol = 28 初筛 + 4 扩样**，全部结果校验。汇总 `r2-keyboard-stable-initial-summary.json` 与 `r2-keyboard-stable-summary.json`。

Alice 200M 前缀三对保持预算出口、隐藏 client **314×211** 与 Draw/Present **0/0**。CPU 中位数 **−3.102%**、wall **−3.402%**，after CPU **2.583591 秒**、wall **2.621487459 秒**、RSS **417,366,016 bytes**。直接复用原 runtime，未复制素材；该前缀未绘制，未推断游戏 FPS。证据 `r2-keyboard-stable-alice/compare.json`。

在保留整数局部成本和离散轮次的前提下接受此冻结，本批 **16 exports** 纳入已验收范围，累计 **400 exports**。随后准备的 raw-input/IME 基线与 fixture 草稿不属于该冻结、通过计数或本批验收。
