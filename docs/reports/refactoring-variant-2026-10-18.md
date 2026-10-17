# R2 Variant（2026-10-18）

## 独立旧行为基线

起点 `0b00cf7`，before 为 `r2-bstr-wire/wire`，SHA256 `43c225f5103d14f867c07ff81d88300a652e9b256a7de099e685b133f8bb00dc`。范围四项 PE32 oleaut32 export，Win64 保持 absent。

基线提交 `62ff9f9`，四项新增回归在旧实现全部通过，固定 16 字节 scalar/reserved/payload copy、Clear 只清 tag、低 WORD 目标类型、忽略 flags、stdcall 与 LastError；容量 64 的 heap 连续失败验证 BSTR deep-copy rollback 不泄漏，自复制与 BYREF 生命周期、错误检查顺序和 Win64 import/GetProcAddress absent。证据 `r2-variant-baseline-tests.txt`。

新增可选 `gateway-variant` 原创 fixture，每轮复制 I4(tag 3、payload 42) 再 Clear，checksum 消费返回值、copy 后 tag/payload 和 clear 后 tag，无逐轮资源增长。旧 executable 的 PE32 raw/static IR 15K 预检均核对 checksum、退休数和 API 次数，证据 `r2-variant-fixture-baseline`。

## 实现与正确性

四项接入唯一 VariantApi，旧 common_api admission 和名称实现全部移除。adapter 只借明确资源，通过同一 Bstrings32 owner 分配、释放与失败回滚，保持原访存、HRESULT/LastError、日志和返回顺序。WMI 共享 allocation facade 和 literal `#9` ABI 保留，Win64 不增加能力；详见 [Variant 边界](../architecture/variants.md)。

完整 release **523 passed / 7 ignored**、debug Win32/AMD64/FPU **370 passed**、显式 native **7 passed**、工具 **20 passed**，fmt、strict all-target/all-feature Clippy 和 release build 全部通过。冻结 `r2-variant-wire/wire`，SHA256 `1c5fcc14fec044cf9e9c4a6d7c0c947e584f6fad81579bd439b909cdfad39e64`，**5,922,232 bytes**，较 before 减少 **78,944 bytes**。

冻结时 **304 个 Rust 文件**（src/tests **296**）逐项与活动源码和快照核对一致，target executable 与冻结哈希一致，证据 `r2-variant-source-verification.json`。下一批 GUID allocation 测试及 fixture 随后创建，不属于该冻结输入或本批通过计数。

## 性能

稳定启动协议下 **55 条初筛路径**各三对全部结果校验，CPU 中位数最大回退 **+2.137%**，RSS 最多增加 **320KiB**。新增 Variant scalar copy/clear 网关 CPU PE32 IR/raw **−0.843%/−2.248%**；该负载不包含 BSTR allocation 或复杂类型转换。

五条路径出现单轮超过 5% 的回退，按三倍 work、直接引用 before 原路径的同哈希 control、三种顺序各一对扩样。完整 `r2-variant-stable-summary.json` 保留 **60 个 workload protocol**，不同 work 独立记录：

| 路径 | work | after/before CPU | control/before CPU | after/control CPU |
| --- | ---: | ---: | ---: | ---: |
| Win64 calendar | 300M | −0.445% | −0.285% | −0.161% |
| Win64 thread control | 300M | +0.625% | −0.254% | +0.881% |
| PE32 integer/IR | 900M | −0.101% | +0.052% | −0.153% |
| PE32 FP/IR | 900M | +0.017% | +0.232% | −0.215% |
| PE32 Local/Global/IR | 150M | −0.036% | +0.135% | −0.170% |

五项扩样最大 CPU 中位数回退 **+0.625%**，三对均回退超过 5% 的路径为零。全部初筛和离散轮次保留；历史线程控制 SSE、线程属性 HeapSize、日历 Local/Global 和 BSTR SSE/raw 的成本继续按各自基线记录。

Alice 200M 三对均保持预算出口、隐藏 **314×211** 窗口及 Draw/Present **0**，CPU **+0.051%**、wall **−0.044%**，现有 runtime 直接复用。该前缀尚未实际绘制；编译、正式计时与 native 游戏串行，证据 `r2-variant-stable-alice`。

接受四项 export，契约累计 **352 项**。未扩展 Variant 语义或 Win64 能力，Mono 继续排除。
