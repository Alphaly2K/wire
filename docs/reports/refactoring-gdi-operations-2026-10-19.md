# GDI 对象、像素与字体契约候选

状态：31 项结构迁移已接入，完整验证进行中；尚未增加已验收计数。直接基线为 PE32 分发模式冻结 `929f495f386ef76931e75b5cc6dbfd7dc855083e7c47e8298fbc552c2a182558`。对话框、菜单和分发候选的性能成本继续独立保留。

迁移前提交三项 PE32 独立基线和一项 Win64 缺席基线，在旧运行时上与已有 GDI/DC 检查共 13 项通过。覆盖 BITMAP 输出覆盖返回地址、跨页失败的原字节与 frame、颜色/BOOL 错误和 error120 trace、错 provider 零清栈、DIB handle 错误优先于 flags/input 以及可读 BITMAPINFO 诊断。测试输入和期望值独立于新 catalog。旧 executable SHA 与冻结仍一致，日志 `r2-gdi-operation-independent-baseline.txt`。

新增唯一 catalog 和纯图形 GdiOperation 枚举；31 项均 PE32 typed、Win64 absent。删除 30 条旧手写 ABI，逐项清栈数相同；GetObjectType 原用 Wine 10.0 `gdi32.spec:415` 的 stdcall(long)，新签名继续一槽。GDI 适配层改为只借 Graphics 与错误槽，删除旧 DLL/名称检测列表；核心、DIB 和 font 三个模块也直接匹配操作。

位图、DIB、像素和字体处理顺序保持，font 优先于 bitmap，错误诊断与输出/返回槽提交位置保持。三份核心源文件核对为原文本加明确的名称→枚举、字符串条件→有限枚举条件替换和 rustfmt，计算、访存、资源和错误处理体没有其它改动。graphics 不导入 Win32 catalog；没有扩展 Win64 GDI、native graphics、Mono 或 IL2CPP 能力。

完整 release **623 passed / 7 ignored**、debug Win32/AMD64/FPU **469**、显式 native GPU **7**、fmt、strict all-target/all-feature Clippy 与 release build 全部通过。冻结 `r2-gdi-operation-wire/wire`，SHA256 `a95c0105e765e8ce0c644cfa717ff5737f10c3adcfa3fd016db536f066a8b90a`，6,018,680 bytes，较直接基线减少 6,240 bytes，满足本批 +24KiB 预算。source manifest 与 patch 同目录保存，测试与构建日志使用 `r2-gdi-operation-` 前缀。

GDI 150M raw/IR 三标签三对共 18 次执行全部校验。CPU 中位数相对直接父冻结 IR +3.853%、raw −1.107%；相对消息冻结 +5.117%/−5.622%。局部 IR 成本继续保留，没有以 raw 改善抵消它。

随后完成累计宽矩阵：合计 30 条协议、186 次执行，全部结果校验，`r2-gdi-operation-broad-metrics.json` 保留每一组。AMD64 memory/SSE/FP/bit/data-alias CPU 中位数 −11.068%/−2.978%/−5.642%/−2.524%/−0.486%，PE32 FP IR/raw −0.867%/−8.497%。

九条初筛超过 3% 调查线：PE32 integer/IR +4.364%、SSE/IR +6.315%、bit/IR +5.918%、GDI/IR +5.117%、messages/IR +4.044%、menu/raw +6.130%、GUID/raw +3.173%，以及 AMD64 integer +7.062%、window-control +5.896%。其中 bit/IR 逐对 +7.081%/+5.064%/+5.918%，先以 900M work、same-hash control 和 scalar/direct/本批冻结区分宿主漂移与引入成本的阶段；其它超阈值路径随后扩样。

900M bit/IR 五标签三对随后全部结束，15 次校验通过；相对 before 的 CPU 中位数：control −0.580%、scalar −1.561%、previous/direct +2.222%、after −0.664%。短样本的持续回退没有在该轮扩样复现，各阶段与同哈希 control 均独立保留；其余八条超线路径继续扩样。

其余八项扩样全部结束，72 次执行均完成结果校验。CPU 中位数及逐对离散保存在 `r2-gdi-operation-expanded-metrics.json`：

| 路径 | after/before | 同哈希 control/before | after/control |
| --- | ---: | ---: | ---: |
| PE32 integer/IR 900M | +4.842% | +7.675% | −2.631% |
| PE32 SSE/IR 900M | +1.192% | −0.668% | +1.872% |
| AMD64 integer 2.1B | −0.997% | −0.408% | −0.591% |
| PE32 GDI/IR 450M | +1.343% | −1.627% | +3.019% |
| PE32 messages/IR 150M | +7.371% | −2.458% | +10.077% |
| PE32 menu/raw 150M | +1.185% | +6.750% | −5.213% |
| PE32 GUID/raw 150M | +8.960% | +9.652% | −0.632% |
| AMD64 window-control 300M | −1.672% | −3.960% | +2.383% |

messages/IR 逐对 +5.488%/+14.136%/+7.371%，触及三对持续回退的拒绝线，当前冻结不准入。GUID 同哈希 control 出现相近成本；窗口控制逐对 −17.768%/+2.883%/+30.982%，离散较大，保留全部轮次。没有用其它负载的改善抵消消息回退。

随后对已验收消息、直接分发、当前 GDI 三份冻结分别做 5 秒/1ms 的真实采样；相同消息 exe/IR、headless、utility、完整格式化但 API 文件指向 `/dev/null`，采样和正式计时分开。首次受限沙箱内采样失败，报告保留在 `r2-gdi-message-profile-2026-10-19`；允许进程采样后成功证据在 `r2-gdi-message-profile-v2-2026-10-19`。三者日志 inclusive 占比 36.799%/36.787%/37.447%，通用格式化 31.886%/32.422%/30.910%，消息 handler 22.514%/22.518%/23.188%，热点分布接近；尚未确定持续回退的具体来源。以该证据开展保持逐字节日志格式的专用整数格式化优化，另存候选与对照。

尚未完成性能准入，正式计数保持 595。架构入口见 [GDI 对象边界](../architecture/gdi-operations.md)。
