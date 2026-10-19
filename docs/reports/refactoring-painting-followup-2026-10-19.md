# 五项 paint 契约的独立基线与宿主控制

状态：已补基线，运行时迁移暂缓；没有接入新的 paint catalog。正式运行时仍为已验收 `b433c63`，累计 675 项。

下一候选只包含 InvalidateRect、ValidateRect、GetUpdateRect、BeginPaint、EndPaint 五项。RedrawWindow 的真实 API 网关已经由 MenuApi::Redraw 负责，保留 error 120 与菜单未实现诊断；公开 `user32::dispatch` facade 则进入矩形绘制实现。独立菜单用例已固定这一区别，不能把 facade 行为接到真实网关。先前六项候选的缺陷和拒绝仍按原报告保留。

在旧实现上新增三项独立行为测试，固定五项的 HWND 验证先于 guest pointer 读取、wrong-provider 的未知 ABI/零参数清栈与诊断，以及 BeginPaint/GetUpdateRect 输出覆盖返回地址后再读取返回 frame 的顺序。GetUpdateRect 保持原有 erase 参数行为及更新矩形；既有用例继续覆盖回调 fault 后的 region 消费、输出未写入与 DC 生命周期。初版用例直接设置 state.last_error，未同步 guest TEB；改用真实 SetLastError 调用后，PE32 五项测试及 Win64 一项 absent 测试全部通过。实现尚未修改，测试提交 `3e8dcf9`，证据 `r2-paint5-independent-baseline-v2.txt`，初始失败日志保留。

开始迁移前，用同一已验收二进制、同一 SHA、同一固定 executable/cwd/argv、utility、headless/null-log 协议做 150M 消息 IR 三对控制。两轮合计 12 次执行全部结果校验。首轮 CPU 中位数差 −3.686%，各对 −35.339%/−4.791%/−0.699%；末对趋稳后再核验，第二轮中位数差 −8.540%，各对 +18.234%/−8.540%/−16.064%，单次 CPU 6.497–16.004 秒。正式计时未与本任务编译、采样、native 探针或 Rust 修改重叠。

同一程序仍有明显离散，暂时无法据此验收新的运行时布局。没有选择性排除首轮或慢轮，也没有据控制结果认定代码回退；保持原运行时，继续独立 R7 工具工作。全部控制数据与输入 hash 在 `r2-paint5-host-control`、`r2-paint5-host-control-v2`，各自 `control-metrics.json` 保存变化。宿主可重复测量后再实施五项迁移，不重复引入已拒绝的内联/CGU/LTO 试验。
