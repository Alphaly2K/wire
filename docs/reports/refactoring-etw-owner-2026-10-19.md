# Win64 ETW provider 状态归属

状态：完整正确性、性能和启动前缀通过。直接运行时基线为累计已验收的 `d65c389` 环境冻结；独立旧行为测试与原创 fixture 先提交于 `480a14e`。本批不新增 catalog export，累计仍为 **706**。

Runtime64 的注册表与 next_etw 两个字段收拢到私有 `tracing::Providers`，ETW 处理函数由 `impl Runtime64` 改为 owner 方法，只借用 Memory 与参数。现有八个 legacy 入口的 provider/arity/DWORD、禁用 session 能力、状态码及提交顺序保持；[架构边界](../architecture/tracing.md)说明 source/output guard、容量、退休句柄和事件 header 的不同规则。原处理体经过明确的类型/方法/字段改名后逐字一致，证明 `r3-etw-source-proof.json`。

三项独立旧行为测试先在原实现通过，覆盖注册故障不消耗句柄、guard 的 source/output 顺序、output alias saved frame、完整句柄身份、只检查 disabled event 的 header、不跟随 payload/文本/callback、LastError/API count 及 64 个同时注册和累计 4096 个单调句柄的边界。迁移前 release 三项通过，迁移后包含既有网络/ETW 生命周期的 release/debug 定向各 11 项通过。

完整检查 `r3-etw-validation-*`：release **647 passed / 7 ignored**，debug Win32/AMD64/FPU **491** 加日志单元 **2**，显式 native GPU **7**，工具 **29**；fmt、strict all-target/all-feature Clippy、SIMD crate 静态检查及 release build 通过。新增 `gateway-etw` 的 OFT/IAT inventory 要求恰好八项原入口，单次和 12K 输入已在旧冻结上消费 checksum、work/retired 与 API count。

冻结 `r3-etw-wire/wire`：SHA256 `18a1c1e8c50a246396f0a449404d06df83164546124b8b506aa3257d2bff5911`，**6,063,160 bytes**，较环境冻结 **+176 bytes**，在本批 +24KiB 预算内。302 个 src Rust 文件与 target/frozen/manifest 核对一致，仅两项运行时源码改变；没有修改 CPU/IR/cache 或图形实现。

正式比较使用三标签轮换、相同旧冻结 control、stable launch 和 fixed executable slot，常规 AMD64/PE32 为 utility，V2 保持其独立默认 QoS。12 条协议、**108 次执行**全部校验；完整汇总 `r3-etw-performance-summary.json`。ETW 150M work 循环约 20.93M API 次数，CPU 中位数 **−0.149%**；其余计算/别名/网关 −2.477% 至 +2.110%。memory 的 after/before −7.983% 伴随同哈希 control/before −8.734%，after/control +0.823%，保留该漂移，不据此宣称访存收益。fixture RSS 中位数最多增加 **81,920 bytes**。

三对 Alice 200M work 在可访问 Metal/AppKit 的宿主环境中全部到相同预算、hidden 314×211、Draw/Present 0/0 出口。CPU/wall 中位数 **−0.168%/−0.519%**，RSS 中位数 **+1,081,344 bytes**；仍处于已有启动样本的离散范围，没有扩大任何缓存/资源额度。证据 `r3-etw-performance-alice`；复用原素材与输入 SHA，解释器临时软链接退出删除。该前缀未进入绘制，不推断游戏 FPS。
