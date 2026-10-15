# 时钟 API 契约迁移

## 独立基线

基线为 `390665b` 的路径/句柄运行时，复用 `r2-paths-handles-v2-wire/wire`，SHA256 `fe24dec579585793f92866a893b500fbd53447a53297e72236eb40fe50cfd074`。本批六项为 GetTickCount、GetTickCount64、timeGetTime、QueryPerformanceCounter、QueryPerformanceFrequency、GetSystemTimeAsFileTime，不调整 sleep、调度或日历 API。

新增五项独立回归已在旧 handler 上通过：PE32 的 EDX:EAX、DWORD 保留 EDX、10MHz frequency、stdcall 与 LastError；输出只读 fault、覆盖返回槽的提交顺序；Win64 hot route 的完整指针和 frame/output fault；timeGetTime 的静态 provider 与 kernel 动态查询差异；delay import 解析后走 general route，输出覆盖返回槽仍使用提前保存的 frame。首次测试 fixture 漏注册 kernel provider，保留失败日志，补齐原 harness 所需前置条件后五项全部通过，没有修改运行时。

原创 gateway-clock 只调用 QueryPerformanceFrequency，累加 BOOL 返回并读出固定的 10,000,000 输出，两架构均检查退休数和 API count。PE32 raw/IR、Win64 的 12K 预检通过，默认 case 集合保持。源码和日志位于 `tests/{win32,x64}/clocks.rs`、`tools/perf/fixtures/clock_gateway.py` 与 `wire-out/refactoring-2026-10-14/r2-clocks-baseline-*`。

## 接入与正确性

六项 provider、scalar 参数、返回约定与架构策略集中到 `ClockApi`；ApiId variant 追加到末尾。删除两端重复的手写 ABI/arity 和字符串 handler。Win64 冷绑定从 ClockApi 选择原 Counter/Frequency/Tick32/Tick64 热路由，执行 body 保持；SystemFileTime 和 delay import 使用只借用参数、内存与原进程 Instant 的架构 adapter。

PE32 的 Qword 结果按原顺序写 EDX、公共 EAX 与返回 frame，DWORD 保持 EDX。输出故障与返回槽别名按架构保留；wall-time 的 cast-before/divide-before 差异也保留。timeGetTime 的 Win64 kernel 静态导入与 fresh-provider 动态 lookup 差异用独立 lookup_dlls 声明，原 gateway registry fallback 保持。详细边界见 [架构说明](../architecture/clocks.md)。

完整 release **437 passed / 7 ignored**、debug Win32/AMD64/FPU **285 passed**、显式 native GPU/Metal **7 passed**、工具 **16 passed**；fmt、strict all-target/all-feature Clippy 与 release build 全部通过。新 GetLastError/进程身份测试草稿尚未注册，不属于本批测试计数。

after 冻结于 `r2-clocks-wire/wire`，SHA256 `573529ac7f879977f11f808f9fb24eb65402eed097ef678ac200cade6b404d83`，5,937,480 bytes，比 before 增加 94,944 bytes。所有计时在冻结二进制之间进行；Cargo、采样和 runtime diagnostics 分开。

## 旧协议暂缓与调查

49 条性能路径全部通过 guest 结果校验。时钟负载的 Win64/PE32 IR/raw CPU 中位数为 −0.289%/+1.664%/−1.991%；但 PE32 integer/IR 的 700M 初筛出现 −1.759%/+40.586%/+7.160% 三个 paired 变化，中位数比例为 +37.096%。同二进制 control 中位数接近 before，不能将该成本归为共同宿主漂移。

正式计时结束后独立读取三组宿主硬件计数：before/after 指令中位数约 79.831B/79.842B，周期约 9.159B/24.055B。随后完全相同 700M 协议的三组正式轮换再次复现，中位数 +45.764%，三组均超过 5%。本版按拒绝门槛撤回接受决定，不执行 Alice smoke，也不将其余 48 条通过替代该失败。

五个 PE32 执行节的静态指令数分别为 before/after：IR 2207/2203、mixed 502/502、SIMD 1666/1664、raw 2238/2238、0f 6212/6212。仅 raw opcode 序列相同，其余生成代码有变化。ISA/cache 源码未修改；完整反汇编、规范化差异与独立硬件计数保存在 `r2-clocks-{before,after}-__wire_*.asm`、`r2-clocks-ir32-normalized-diff.txt` 和 `r2-clocks-integer-hardware/`。初筛和补样全部保留于 `r2-clocks-{initial,integer-recheck}-metrics.json`。

随后尝试显式保留 PE32 clock adapter 的函数边界，构建哈希却与首版完全相同，不能把后续较快样本归因于该属性；属性已移除。相同哈希的原候选与副本在同一旧协议轮换中也出现约 2.3s/7.1s 的重复差异，现已转为独立测量协议调查。固定 argv/log 参数且保持 utility QoS 后，两副本中位数相差约 0.15%。具体机制和全部失败/补样记录见 [启动协议调查](refactoring-launch-protocol-2026-10-16.md)。旧协议数据完整保留，后续验收独立使用新协议，未合并两者中位数。

## 稳定启动协议重新验收

`--stable-launch` 下重新完成 **49 条** case/path 的三对结果校验。integer/IR 的 700M 四标签结果复用协议调查的正式入口测量，after/before 为 +1.616%，同哈希 clone/after +0.120%、control/before +0.076%。其它敏感路径采用 700M 三标签顺序轮换：AMD64 memory −3.467%、thread-ID −0.559%、version +0.009%、file-type −1.220%，PE32 SSE/IR −0.865%。

初筛有两项超过 3%：PE32 bit-test/IR 300M 为 +3.194%，三对分别 −1.342%/−1.510%/+16.229%；gateway-path-helper/IR 50M 为 +6.247%，三对 +7.866%/+4.784%/−2.565%。两项均扩为 **700M**，以 after/before/control、before/control/after、control/after/before 轮换，保留原短样本且按不同 work 分组。bit 长样本中位数 +0.586%，三组 −20.480%/+1.496%/+0.586%，control/before +1.592%；仍有明显单轮离散。路径辅助长样本 −1.201%，三组 +1.187%/−1.201%/+0.692%，control/before −0.680%。没有三组均超过 5%，也不从负值推断 ISA 算法改善。

用对应长样本验收两项后，其余路径最大 CPU 中位数回退为 PE32 fp-scalar/IR 的 **+2.776%**；RSS 中位数最大增加 **144KiB**。时钟网关的 AMD64、PE32 IR/raw 分别为 −2.260%/−5.148%/−1.892%。没有修改 ISA/cache、缓存容量或调度算法；可执行文件增加 94,944 bytes，后续批次继续单独报告体积变化。全量汇总为 `r2-clocks-stable-final-metrics.json`，含 49 条路径和两个不同 work 的扩样分组，所有 manifest、离散轮次、旧协议和同哈希调查均保留。

新协议工具 **19 项**通过。迁移提交前重新核对冻结 manifest 中 **225 个**已注册 Rust 文件，源码 SHA 全部一致；未注册的 ContextApi 草稿不计入 437/285 的测试结果。

Alice 200M 三对在现有 runtime 目录串行执行，固定 argv0，保持游戏 cwd，使用单独记录的 `fixed-argv-game-v1` 协议。六轮均以预算出口停止、窗口隐藏且 client 314×211、Draw/Present 0；CPU 中位数 **−0.403%**、wall **−0.531%**。这次启动前缀未推进至实际绘制。证据在 `r2-clocks-stable-alice/`，没有复制或硬链接游戏素材。根据新协议完整矩阵、扩样、正确性与启动结果接受本批；旧协议中的拒绝与后续调查保持可追溯。
