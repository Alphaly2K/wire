# R2 GUID 资源辅助（2026-10-18）

## 独立旧行为基线

起点 `8486ed6`，before 为 `r2-variant-wire/wire`，SHA256 `1c5fcc14fec044cf9e9c4a6d7c0c947e584f6fad81579bd439b909cdfad39e64`。范围 CoCreateGuid、StringFromCLSID、StringFromIID，补齐原有三个 GUID parser/formatter 的契约家族。

基线提交 `925b834`，四项新增回归在旧运行时全部通过：容量 80 的 PE32 heap、失败输出保留或清零、访问错误顺序、78 字节进程 heap 分配与释放、自身输入/输出别名、两端返回槽别名、高位指针、故障时 CPU/API count/内存不提交以及 Win64 provider 范围。既有 entropy/UUID 布局、GUID roundtrip、TaskMemory 释放与跨页 fault 测试继续保留，证据 `r2-guid-allocation-baseline-tests.txt`。

新增可选 `gateway-guid-create`，反复调用 CoCreateGuid(NULL)，累计两架构各自 HRESULT，避免随机输入与资源增长。旧 executable 的 Win64 与 PE32 raw/static IR 12K 预检均核对 checksum、退休数、API 次数及 Win64 work，证据 `r2-guid-allocation-fixture-baseline64/32`。该负载覆盖错误返回开销，没有测量 entropy 或 owning string allocation 吞吐。

## 实现与正确性

三个 export 汇入原 GUID 契约，删除重复 ABI、arity、provider、legacy bit 路由与动态 lookup 分支，执行按已绑定 ID。PE32 adapter 只借参数、内存、heap 与进程 heap ID，保留先读 GUID、输出清零、分配、写入及失败回滚顺序；Win64 继续由原 allocator 与私有 TaskMemory 协调分配及发布。HRESULT、LastError、entropy 错误与返回帧差异保持，详见 [GUID 边界](../architecture/adding-api.md)。

首次领域构建发现模块查询仍引用旧 GUID export helper，移除该重复分支及残余 import 后，完整领域回归和严格检查通过。失败日志保留为 `r2-guid-allocation-domain-tests.txt`，修正后的领域日志为 `r2-guid-allocation-domain-tests-fixed.txt`。

完整 release **527 passed / 7 ignored**、debug Win32/AMD64/FPU **374 passed**、显式 native **7 passed**、工具 **20 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。冻结 `r2-guid-allocation-wire/wire`，SHA256 `ec45ed673a43da225fb3f78541e4941e7d5c9050067d6bd19f1f4fc6ddc1e0d8`，**5,917,144 bytes**，较 before 减少 **5,088 bytes**。

冻结时 **306 个 Rust 文件**（src/tests **298**）逐项与活动源码和快照核对一致，target executable 与冻结哈希一致，证据 `r2-guid-allocation-source-verification.json`。随后准备的下一批基础 helper 草稿不属于本次冻结或通过计数。

## 性能与接受决定

稳定启动协议下 **63 条初筛路径**各三对全部结果校验，包含原 GUID 网关、新错误返回网关，以及 AMD64 FP/bit 与缓存别名。初筛最大 CPU 中位数回退 **+4.294%**，RSS 最多增加 **112KiB**。新网关 CPU Win64 **−0.951%**、PE32 IR/raw **+1.452%/−1.379%**；原 GUID formatter 网关 Win64 **−0.179%**、PE32 IR/raw **+0.161%/+3.751%**。

八条路径超过中位数门槛或出现单轮超过 5% 的回退，按三倍 work、直接引用 before 原路径的同哈希 control、三种顺序各一对扩样。完整 `r2-guid-allocation-stable-summary.json` 保留 **71 个 workload protocol**，不同 work 独立记录：

| 路径 | work | after/before CPU | control/before CPU | after/control CPU |
| --- | ---: | ---: | ---: | ---: |
| Win64 WinRT | 300M | +0.585% | −0.585% | +1.176% |
| PE32 integer/IR | 900M | +1.160% | −0.303% | +1.468% |
| PE32 integer/raw | 900M | −0.775% | −1.019% | +0.247% |
| PE32 CoCreateGuid/IR | 150M | −0.279% | −0.197% | −0.081% |
| PE32 GUID formatter/raw | 150M | +1.135% | −0.010% | +1.145% |
| PE32 thread control/raw | 150M | +2.065% | +0.107% | +1.955% |
| PE32 multimedia/IR | 150M | +3.181% | +0.081% | +3.098% |
| PE32 VirtualQuery/IR | 150M | +3.584% | −0.273% | +3.868% |

保留 multimedia/IR 和 VirtualQuery/IR 的局部成本；三对分别 **−0.067%/+4.087%/+0.602%**、**+3.045%/+4.521%/+1.710%**。没有三对均超过 5% 的拒绝路径。正式计时及 Alice 结束后独立 diagnostics：multimedia 两版 retired/API **120008/10909**，IR/fallback/CALL_NATIVE **109099/10910/10909**；VirtualQuery 两版 retired/API **120016/10910**，IR/fallback/CALL_NATIVE **120016/1/10910**，compact 计数也分别一致。

16 个 PE32 CPU/IR/memory 源文件及两个 gateway adapter/period owner 的源码哈希保持；五个 PE32 执行器节的指令数和助记符序列一致，未核对全部操作数。api::dispatch 到下一 text symbol 的跨度 **31,616 → 32,136 bytes**，multimedia/virtual-memory adapter 跨度仍为 **364/776 bytes**，符号地址改变。局部成本机制尚未确证。证据 `r2-guid-allocation-diagnostics32-checked`；首个 diagnostics 脚本错误要求 int3 fixture 返回 exit 0，修正为原 exit 1 后全部结果校验，原失败日志保留在 `r2-guid-allocation-diagnostics32`。

Alice 200M 三对均保持预算出口、隐藏 **314×211** 窗口及 Draw/Present **0**，CPU **−0.401%**、wall **−0.530%**，原 runtime 直接复用。该前缀尚未实际绘制，未推断游戏 FPS；编译、正式计时、native 游戏与 diagnostics 串行，证据 `r2-guid-allocation-stable-alice`。

接受三个新增 export，GUID 家族合计六项，契约累计 **355 项**。两项局部成本及此前各批历史成本按独立基线保留，Mono 继续排除。
