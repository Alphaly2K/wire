# R2/R3 线程查询与设置（2026-10-17）

## 独立旧行为基线

起点 `84e6fe0`，旧 executable 为 `r2-address-waits-wire/wire`，SHA256 `13422035b6ce571a74b94697bc07237b628e7be8e9771e700f2f360cf6fb6a90`。范围 Get/SetThreadErrorMode、Resume/SuspendThread、GetThreadId、Get/SetThreadPriority、GetExitCodeThread、SetThreadAffinityMask，共九项 export；Create/ExitThread、APC、CONTEXT 和 description 分配不在本批。

新增六项独立迁移前回归全部通过。PE32 固定二态挂起/恢复、优先级空操作、无效 handle 查询 exit 仍成功写零、pseudo handle 差异、输出故障与未保存返回槽别名；GetThreadId 和两项 error mode 保持 Wine ABI 已知但未实现，错 provider 保持未知 ABI/零参数清栈。Win64 固定完整 handle/affinity carrier、优先级 DWORD 截断及零扩展返回、127 层挂起上限、invalid handle 先于参数/输出检查、mode 校验/输出 fault/state 提交顺序、guard、保存返回帧和错 DLL 拒绝。证据 `r2-thread-properties-baseline-tests.txt`。

新增可选 `gateway-thread-properties`，循环设置并读回优先级，独立核对返回总和、退休指令和 API 次数；Win64 另核对 work。两种架构及 PE32 raw/static IR 在旧 executable 上 9900 work 预检通过，默认 fixture 集合保持。证据 `r2-thread-properties-fixture-baseline64`、`r2-thread-properties-fixture-baseline32`。

## 实现与正确性

基线提交 `473dcd1`。九项 export 接入 ThreadPropertyApi，移除重复 PE32 ABI、Win64 arity/DWORD、动态查询特例和根名称分发；PE32 三项 ABI-only 策略保持，新增 typed guard 追加在既有 scalar 家族之后。PE32 adapter 只借内存、Sched 和 LastError，二态挂起切换和 exit 状态读取归 Sched；保留 common affinity 原日志文本，SetThreadIdealProcessor 的剩余逻辑保持原语义。

Win64 冷绑定普通 scalar ID，`Threads::property_call` 只借受检内存和当前错误槽，error mode 字段私有；进程保留返回帧预检、CPU、unblock、trace 和 API count 提交。专用创建、退出、APC、CONTEXT 和 description 路径留待后续批次，未塞进 scalar handler。架构细节见[线程查询与设置](../architecture/thread-properties.md)。

六项独立回归及完整 release **494 passed / 7 ignored**、debug **341 passed**、显式 native **7 passed**、工具 **20 passed** 均通过，fmt、strict all-target/all-feature Clippy 和 release build 通过。冻结 `r2-thread-properties-wire/wire`，SHA256 `8bf857f2dbfe1953bc9e841b4da127c6508395531e5bb7335e9d816d5f9fb39f`，**5,973,128 bytes**，减少 **24,496 bytes**；279 个 Rust 文件（src/tests 271 个）全部与 manifest 核对一致，证据 `r2-thread-properties-source-verification.json`。

## 性能与接受决定

稳定启动协议、43 条路径、各三对全部结果校验，初筛 RSS 最多增加 **112KiB**。Win64 线程优先级网关 CPU **−6.625%**，PE32 IR/raw **+3.351%/+1.089%**。另外三条初筛超过 3%：Win64 HeapSize **+3.816%**、PE32 HeapSize/raw **+3.873%**、FP/raw **+4.744%**。全部原始轮次保留于 `r2-thread-properties-stable-initial-summary.json`，包括 integer/IR 的较大负差异；本批未改执行器算法。

四条路径分别扩大到 300M、150M、600M、150M work，并直接使用同 before 路径、同哈希 control，按 after/before/control、before/control/after、control/after/before 三种顺序测试。不同 work 独立汇总，完整证据 `r2-thread-properties-stable-summary.json`，共 43 条初筛路径与四项扩样协议：

| 路径 | 扩样 after/before CPU | control/before CPU | after/control CPU |
| --- | ---: | ---: | ---: |
| Win64 HeapSize，300M | +4.063% | +0.290% | +3.763% |
| PE32 HeapSize/raw，150M | +1.938% | +0.243% | +1.691% |
| PE32 FP/raw，600M | −3.254% | −0.700% | −2.573% |
| PE32 线程网关/IR，150M | +1.148% | −0.318% | +1.471% |

Win64 HeapSize 三对 after/before 为 **+4.186%/+3.621%/+4.063%**，保留为本批局部成本。正式计时后独立 diagnostics，两版 IR **8771**、translations **28**、fallback/invalidations **0**；AMD64 CPU/heap 源码哈希保持。CheckedSegment/RegisterSegment 分别仍为 **2156/1968** 条生成指令，助记符序列 SHA 相同，该比较未核对全部操作数。catalog_call 从 **17,512** 增至 **17,688 bytes**，dispatch_bound 从 **51,132** 缩至 **47,428 bytes**，符号地址改变；局部成本的具体机制尚未确证。证据 `r2-thread-properties-heap-investigation.json`、`r2-thread-properties-ir-codegen.json`、`r2-thread-properties-symbol-layout.json` 及原始反汇编。

Alice 200M 三对全部通过，CPU **−0.544%**、wall **−0.043%**，保持预算出口、隐藏 314×211 窗口和 Draw/Present 0；证据 `r2-thread-properties-stable-alice`。该启动前缀未进入实际绘制。严格串行完成编译、计时、diagnostics 与 native 游戏，原 runtime 直接复用。

接受本批并明确保留 Win64 HeapSize 的有限局部成本；没有三对均回退超过 5% 的路径，完整正确性、主要 CPU/缓存路径和游戏启动出口保持。契约累计 **313 个 export**，Mono 继续排除。日历批次已有的 Local/Global 局部成本仍按其原基线记录，不由本批结果覆盖。
