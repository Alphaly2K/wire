# Unity 后台渲染结束后的累计性能复核

状态：累计验收通过。对照此前累计验收的 `b433c63` 日志冻结与当前 `d65c389` 环境接口冻结，覆盖中间五项 paint、两项控件初始化、十项资源、五项启动信息、九项环境接口以及 CLI 拆分。新增 31 项完成性能复核，契约累计 **706 项**。

用户确认 Unity 渲染结束后，宿主只读进程快照显示 Unity Editor 约 1.3% CPU，WindowServer 约 45.1%；上轮 Unity 持续 85%–111% 的测量环境已经变化。旧干扰轮次完整保留，本次新建 `wire-out/refactoring-2026-10-14/r2-post-unity-*`，不合并旧样本。

before executable SHA256 为 `c0f7d4c12691d2059c735bc2d14e8b1643741d3a975df21936a79b2cf02f937d`，6,018,712 bytes；after 为 `eccc6142a980c984d96be14f9b1fe983762527249447ae05a5b6e8e9d83d5e44`，6,062,984 bytes。当前 release 文件与 after 完全一致，302 个 src Rust 文件沿用 `r2-environment-source-proof.json` 的源码冻结。期间没有编辑 Rust、编译、运行采样或并发启动游戏。

全部原创循环使用三标签、三轮顺序轮换；control 与 before 是相同 SHA 的同一冻结文件。PE32 与常规 AMD64 使用 utility QoS、stable launch、fixed executable slot；PE32 继续执行完整 API 格式化及计数，最终写入空设备。V2 保持其已有默认 QoS，与其余协议分开汇总。每条协议独立核对输入、work/retired、checksum、API count、CPU/wall/RSS，不计算跨负载总分。

已完成的消息 150M IR/raw CPU 中位数为 +0.878%/+0.193%；同哈希 control/before 为 +1.237%/+0.122%。首次 IR pair +4.058% 保留，随后两对 +0.703%/+0.434%，after/control 中位数 −0.354%。本次同哈希漂移显著小于旧干扰轮次。

| PE32 50M 接口循环 | IR CPU | raw CPU |
| --- | ---: | ---: |
| paint | −2.809% | −4.385% |
| common controls | +0.517% | +0.798% |
| resources | −6.242% | −4.557% |
| process startup | +1.099% | +0.542% |
| environment | −51.455% | −45.525% |

环境循环包含既有 ABI-only setter；冷绑定后的明确不可用策略避开旧的多层名字兜底分发，仍返回原 error 120，输出、日志与帧行为不变。该循环的改善不代表游戏整体加速比例。

PE32 150M 四项计算、raw/IR 共八条协议 CPU 中位数 −4.098% 至 +0.934%；AMD64 150M 整数、访存、SSE、线程 ID 网关为 −0.355%/−0.015%/+2.574%/−0.542%。V2 150M 浮点、位操作、data-alias、code-alias 为 −1.239%/−1.208%/−1.539%/−1.084%。完整轮次与成对差值分别保存在 `r2-post-unity-{messages,families,compute32,amd64,v2}` 及同名前缀 summary JSON。

既有 GDI 100M IR/raw CPU 中位数 +0.062%/−0.285%，GUID 100M IR/raw −0.363%/−0.040%。共 **32 条协议、288 次执行**全部通过；完整汇总 `r2-post-unity-cumulative-summary.json`。各协议最大 CPU 中位数回退为 AMD64 SSE 的 +2.574%，after/control +3.440%，保留这项局部成本；三对分别 +0.549%/+2.574%/+2.038%，没有持续超过 5% 的回退。GDI/IR 同哈希中位数漂移 −4.236%，其三对 after/before +1.084%/−0.924%/+3.494% 全部保留，没有用环境循环收益抵消其它负载。

fixture 峰值 RSS 中位数最大增长 **114,688 bytes**。累计 executable 增加 **44,272 bytes**（约 0.736%），来自六批独立结构变更；各批分别维持其 +24KiB 文件预算，没有改变缓存容量、资源限额或常驻缓冲。源码与冻结在计时结束后再次核对，证明 `r2-post-unity-admission.json`。

Alice 首次沙箱运行因 Metal 设备不可见而在创建 Vulkan instance 时失败，证据 `r2-post-unity-alice`；该失败没有计入性能样本。随后在允许原生 GPU/AppKit 的宿主环境中，新目录 `r2-post-unity-alice-host` 串行完成三对六次 200M work。全部到相同预算出口，hidden 314×211、Draw/Present 0/0；CPU/wall 中位数 **−1.543%/−0.328%**，RSS 中位数 **−262,144 bytes**。保持原游戏、UnityPlayer、guest Mono、native graphics 与 MoltenVK 的输入 SHA，只临时软链接解释器，退出即删除。该启动前缀未进入绘制，游戏整体帧率和加载完成耗时仍需相应场景另测。

完整正确性沿用同源码冻结的 release **644 passed / 7 ignored**、debug **490 passed**、显式 native **7 passed**、工具 **28 passed**，fmt/严格 Clippy/SIMD 静态检查/build 均已通过；本次没有源码变更，因此没有重复整套编译。2026-10-19 接受当前累计冻结，原来的后台干扰、主动中断与被撤回候选的结论继续保留。
