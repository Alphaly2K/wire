# API 日志职责与格式化候选

状态：累计验收通过，实现 `b433c63`。直接基线为 GDI 候选 `174306b`，累计性能对照正式消息冻结；日志本身不新增 export。对话框 22、菜单 27、GDI 31 项在本修订上累计准入，总数 **675**；各旧冻结的失败和局部成本保留。

当前消息/IR 150M 三对持续回退后，重新采样消息、直接分发、GDI 三份冻结。日志 inclusive 时间分别为 36.799%/36.787%/37.447%，通用格式化为 31.886%/32.422%/30.910%，分发 self 比例没有增加。采样未确定回退的具体来源，但明确存在值得优化的日志成本。完整证据见 `wire-out/refactoring-2026-10-14/r2-gdi-message-profile-v2-2026-10-19`，旧失败和计时轮次继续保留。

先在旧实现上提交两个独立测试，固定完整文件逐字节输出、十六进制边界、十进制宽度和 u64 最大调用号、实现标志、非 ASCII note、同一 slot 的 provider/name 修改、调用频率与穿插 note 后的 64 条历史淘汰。原实现 release 两项通过，提交 `1a20914`，日志 `r2-api-log-independent-baseline.txt`。

将 ApiLogger 提取至 `runtime::api_log`，原 `api::ApiLogger` 重导出保持；公开频率表、调用计数与 recent 仍可被原调用者访问。只将热路径中的固定整数和字段格式改为 String 追加及有界栈缓冲，完整 trace 逐字节写入原 BufWriter；冷路径频率报告排序与格式保持。复用原 64 条 String，所有调用仍更新频率和完整记录，没有删采样、关日志或改变 guest API 语义。数字转换使用受检 UTF-8，无 unsafe。

最初提取遗漏 ApiState 中 File 类型的 import，编译失败记录 `r2-api-log-focused.txt`；补回 import 后 release/debug 两项通过，完整验证与性能对照进行中。

完整 release **625 passed / 7 ignored**，debug Win32/AMD64/FPU **469** 加日志单元 **2**，显式 native GPU **7**，工具 **23** 全通过；fmt、strict all-target/all-feature Clippy、独立 SIMD crate 静态检查与 release build 通过。冻结 `r2-api-log-wire/wire`，SHA256 `c0f7d4c12691d2059c735bc2d14e8b1643741d3a975df21936a79b2cf02f937d`，6,018,712 bytes，较直接 GDI 候选增加 32 bytes，满足 +24KiB 预算。

消息 150M raw/IR 三标签三对、18 次执行全部通过相同 retired/EAX/API count 校验。相对正式消息冻结，IR/raw CPU 中位数 **−13.210%/−12.110%**；相对直接 GDI 冻结 **−15.118%/−12.904%**。固定 argv/cwd/executable、utility、保留日志格式化并丢弃文件写入，manifest/全部轮次见 `r2-api-log-messages`，两种基线汇总分别为 `r2-api-log-message-metrics.json`、`r2-api-log-message-local-metrics.json`。这是原创消息循环的进程 CPU 收益；真实启动前缀和其它负载仍待核验。

累计宽矩阵随后完成，32 条不同 work/路径协议、198 次执行全部校验；完整汇总 `r2-api-log-broad-metrics.json`。PE32 八项计算/IR/raw 为 −3.972% 至 +2.111%，AMD64 七项计算/别名为 −4.408% 至 +0.531%，AMD64 三项网关为 −0.358% 至 +0.712%。GDI 150M IR/raw **−16.987%/−13.229%**；PE32 50M 对话框、窗口控制、消息、菜单与 GUID 的所有 IR/raw 中位数改善 9.048% 至 32.790%。峰值 RSS 中位数最多增加 49,152 bytes，没有增加 cache 容量或资源限额。

逐对正向离散保留：SSE/raw +5.081%/+2.258%/−2.278%，AMD64 bit +1.125%/+5.723%/−9.246%，GUID/raw −15.378%/−12.707%/+19.595%。GUID 最后一对偏离较大，另用 150M work 与同哈希 control 扩样；不把该离散轮次删除。Alice 启动前缀待完成后才决定累计准入。

GUID/raw 150M 三标签三对全部校验，after/before 中位数 **−14.007%**，逐对 −14.584%/−14.007%/−11.729%；同哈希 control/before −1.917%，after/control −12.327%。证据 `r2-api-log-guid-expanded` 与对应 metrics；合计 **33 条性能协议、207 次 fixture 执行**，不同 work 独立汇总。

Alice 使用现有 `wire-out/native-mono-probe-state/runtime`，只临时软链接解释器，退出即移除，未复制或硬链接游戏。三对六次全部按 200M work 停止，hidden、client 314×211、Draw/Present 0/0。CPU/wall 中位数 **+1.272%/+1.650%**。RSS 中位数 +6,193,152 bytes（约 5.91MiB）；before 各轮 334,528,512/416,858,112/408,895,488，after 404,766,720/415,596,544/415,088,640，保留原生启动中的较宽离散；未增加缓存、guest 资源或常驻缓冲。该前缀未进入绘制，不据此推断游戏 FPS 或加载完成时间。

计时结束后单独补抓 5 秒/1ms 消息采样，证据 `r2-api-log-profile-2026-10-19`。日志 inclusive 从 GDI 冻结的 37.447% 变为 26.919%，通用格式化从 30.910% 变为 4.604%；有界 UTF-8 检查等仍有成本，留待以后有证据时优化。402 个 Rust 源文件和实际 release executable 与冻结逐项核对一致，证明文件 `r2-api-log-wire/verification-b433c63.json`；文档改动没有重写原冻结。最终以本修订累计接入对话框、精确标量编译隔离、菜单、直接分发和 GDI；原 `a95c0105…` 冻结仍维持拒绝结论。
