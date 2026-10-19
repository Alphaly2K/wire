# API 日志职责与格式化候选

状态：代码接入，性能待验收。直接基线为 GDI 候选 `174306b`，累计性能仍对照正式消息冻结；本次不新增 export。对话框、菜单、GDI 的 80 项候选仍等待累计准入。

当前消息/IR 150M 三对持续回退后，重新采样消息、直接分发、GDI 三份冻结。日志 inclusive 时间分别为 36.799%/36.787%/37.447%，通用格式化为 31.886%/32.422%/30.910%，分发 self 比例没有增加。采样未确定回退的具体来源，但明确存在值得优化的日志成本。完整证据见 `wire-out/refactoring-2026-10-14/r2-gdi-message-profile-v2-2026-10-19`，旧失败和计时轮次继续保留。

先在旧实现上提交两个独立测试，固定完整文件逐字节输出、十六进制边界、十进制宽度和 u64 最大调用号、实现标志、非 ASCII note、同一 slot 的 provider/name 修改、调用频率与穿插 note 后的 64 条历史淘汰。原实现 release 两项通过，提交 `1a20914`，日志 `r2-api-log-independent-baseline.txt`。

将 ApiLogger 提取至 `runtime::api_log`，原 `api::ApiLogger` 重导出保持；公开频率表、调用计数与 recent 仍可被原调用者访问。只将热路径中的固定整数和字段格式改为 String 追加及有界栈缓冲，完整 trace 逐字节写入原 BufWriter；冷路径频率报告排序与格式保持。复用原 64 条 String，所有调用仍更新频率和完整记录，没有删采样、关日志或改变 guest API 语义。数字转换使用受检 UTF-8，无 unsafe。

最初提取遗漏 ApiState 中 File 类型的 import，编译失败记录 `r2-api-log-focused.txt`；补回 import 后 release/debug 两项通过，完整验证与性能对照进行中。

完整 release **625 passed / 7 ignored**，debug Win32/AMD64/FPU **469** 加日志单元 **2**，显式 native GPU **7**，工具 **23** 全通过；fmt、strict all-target/all-feature Clippy、独立 SIMD crate 静态检查与 release build 通过。冻结 `r2-api-log-wire/wire`，SHA256 `c0f7d4c12691d2059c735bc2d14e8b1643741d3a975df21936a79b2cf02f937d`，6,018,712 bytes，较直接 GDI 候选增加 32 bytes，满足 +24KiB 预算。

消息 150M raw/IR 三标签三对、18 次执行全部通过相同 retired/EAX/API count 校验。相对正式消息冻结，IR/raw CPU 中位数 **−13.210%/−12.110%**；相对直接 GDI 冻结 **−15.118%/−12.904%**。固定 argv/cwd/executable、utility、保留日志格式化并丢弃文件写入，manifest/全部轮次见 `r2-api-log-messages`，两种基线汇总分别为 `r2-api-log-message-metrics.json`、`r2-api-log-message-local-metrics.json`。这是原创消息循环的进程 CPU 收益；真实启动前缀和其它负载仍待核验。
