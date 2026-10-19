# 五项 paint 契约迁移与后台负载复核

状态：五项 paint 已接入唯一契约与 typed 分发，完整正确性通过；随后在 [Unity 渲染结束后的累计复核](refactoring-post-unity-performance-2026-10-19.md)中通过性能验收，当前累计 706 项。以下保留最初后台负载波动、暂停计时和功能验证的原始过程，不追认已撤回候选。

下一候选只包含 InvalidateRect、ValidateRect、GetUpdateRect、BeginPaint、EndPaint 五项。RedrawWindow 的真实 API 网关已经由 MenuApi::Redraw 负责，保留 error 120 与菜单未实现诊断；公开 `user32::dispatch` facade 则进入矩形绘制实现。独立菜单用例已固定这一区别，不能把 facade 行为接到真实网关。先前六项候选的缺陷和拒绝仍按原报告保留。

在旧实现上新增三项独立行为测试，固定五项的 HWND 验证先于 guest pointer 读取、wrong-provider 的未知 ABI/零参数清栈与诊断，以及 BeginPaint/GetUpdateRect 输出覆盖返回地址后再读取返回 frame 的顺序。GetUpdateRect 保持原有 erase 参数行为及更新矩形；既有用例继续覆盖回调 fault 后的 region 消费、输出未写入与 DC 生命周期。初版用例直接设置 state.last_error，未同步 guest TEB；改用真实 SetLastError 调用后，PE32 五项测试及 Win64 一项 absent 测试全部通过。实现尚未修改，测试提交 `3e8dcf9`，证据 `r2-paint5-independent-baseline-v2.txt`，初始失败日志保留。

开始迁移前，用同一已验收二进制、同一 SHA、同一固定 executable/cwd/argv、utility、headless/null-log 协议做 150M 消息 IR 三对控制。两轮合计 12 次执行全部结果校验。首轮 CPU 中位数差 −3.686%，各对 −35.339%/−4.791%/−0.699%；末对趋稳后再核验，第二轮中位数差 −8.540%，各对 +18.234%/−8.540%/−16.064%，单次 CPU 6.497–16.004 秒。正式计时未与本任务编译、采样、native 探针或 Rust 修改重叠。

同一程序仍有明显离散，暂时无法据此验收新的运行时布局。没有选择性排除首轮或慢轮，也没有据控制结果认定代码回退；保持原运行时，继续独立 R7 工具工作。全部控制数据与输入 hash 在 `r2-paint5-host-control`、`r2-paint5-host-control-v2`，各自 `control-metrics.json` 保存变化。宿主可重复测量后再实施五项迁移，不重复引入已拒绝的内联/CGU/LTO 试验。

用户随后指出后台 Unity 正在渲染。本轮只读进程快照观察到 Unity 105.1%、WindowServer 91.9% CPU，构建期间另一次快照为 36.0%/93.9%。它是计时干扰的实际线索，前两轮控制仍保留为历史观测，不据此追认旧候选。继续推进正确性；后续性能记录背景负载、同哈希 control 和全部轮次，不终止用户程序。构建和计时串行，计时期间保持 Rust 源码静止。

五项声明集中于 `win32/catalog/painting.rs`，删除旧 PE32 ABI 手表的五项重复条目。`api::dispatch` 消费冷绑定 PaintingApi；领域实现迁至 `user32/paint.rs`，默认 WM_PAINT 直接调用 typed Begin/End。RedrawWindow 的 MenuApi 网关保持，公开 USER facade 用独立 legacy 操作进入同一矩形实现。窗口与 graphics 的所有权、检查/回调/输出/返回 frame 提交顺序未调整；Win64 仍 absent。没有重新引入先前的内联或构建配置试验。

在首次冻结前固定相对 `r2-api-log-wire/wire` 的 executable 增量预算 +24KiB，基线 6,018,712 bytes。计划记录 `r2-paint5-plan.json`。专项检查全部通过：PE32 五项、Win64 一项、菜单四项，共十项；日志 `r2-paint5-focused-{pe32,x64,menu}.txt`。

完整 release **628 passed / 7 ignored**，debug Win32/AMD64/FPU **472 passed**，另验日志单元 **2 passed**，合计 debug 474。显式提供原 native graphics、MoltenVK、DXBC/GS 输入后 **7 项 native** 通过；fmt、strict all-target/all-feature Clippy、SIMD crate 检查、release build 与 **26 项工具测试**通过。日志统一 `r2-paint5-validation-*`。

冻结 `r2-paint5-wire/wire`：SHA256 `255c74fb63128b90071d00553c6d087c3523499aa727cde2c0808f30d33b49b8`，6,018,760 bytes，仅较基线增加 **48 bytes**。290 个 src Rust 文件的集合、live/frozen/manifest hash 与 target/frozen binary 全部一致；源码只涉及五项绑定、模块和调用入口，未修改 CPU、IR、cache 或 SIMD。核对 `r2-paint5-source-proof.json`。

正式长样本采用固定 executable/cwd/argv、headless、完整 trace 格式化写 null device、utility 和三标签交替顺序。paint 使用 30M work，messages 使用 150M，各自 IR/raw 三轮完整执行。

| 路径 | after/before CPU 中位数 | 同哈希 control/before |
| --- | ---: | ---: |
| paint IR | −19.178% | −10.232% |
| paint raw | +3.808% | +11.567% |
| messages IR | −4.046% | −5.318% |
| messages raw | −7.703% | −14.203% |

上述是受干扰的观测值，不用于性能准入。同一旧二进制的 messages/raw CPU 从约 17.731s 降至 10.138s；paint control 逐对为 −19.363%/−10.232%/+17.491%。paint 前后快照中 Unity 为 111.5%/85.1%、WindowServer 为 91.7%/94.8%，无 Cargo/rustc。本轮没有关闭用户应用。

确认宿主漂移后停止后续长计时，保留 **37 次完整校验执行及 1 次主动中断**。中断仅向本任务 driver 的 Wire guest 发送 SIGINT；该 GDI/after 样本 exit −2、validated=false，驱动保存输出并退出 1。这次中断不列为实现故障。完整观察和未执行范围记录于 `r2-paint5-performance-observation.json`，host 快照和 console 全部保留；不反复扩样来追逐稳定结果。

随后以 12K work 补齐 **20 条功能协议、120 次前后执行**：PE32 paint/messages/GDI/integer/SSE/FP 的 IR/raw、AMD64 integer/memory/SSE/gateway、V2 FP/bit/code-alias/data-alias，全部 work/retired/checksum/API 次数通过。证据 `r2-paint5-functional-{pe32,amd64,v2}`，小样本耗时不作性能结论。

Alice 使用同一原始 runtime 做三对、共六次 200M 启动前缀，全部到既有预算出口，hidden 314×211、Draw/Present 0/0，无新增 API/访存错误。游戏输入 hash 保持，复用原资源，解释器临时软链接每次退出删除。证据 `r2-paint5-functional-alice`。各次 RIP/retired/API 次数随线程与计时变化；RSS before 约 307–378MiB、after 379–397MiB，差异及原始数据保留待复核，本次不据此宣称性能或内存准入。功能结束后再次核对运行时源码、target/frozen binary 一致。
