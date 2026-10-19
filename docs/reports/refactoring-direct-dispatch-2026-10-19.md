# PE32 typed 分发模式收敛

状态：本批结构随 `b433c63` 日志修订累计验收，见 [33 条性能协议与游戏前缀](refactoring-api-log-2026-10-19.md)。以下保留各独立冻结的历史数据与准入结论；旧失败冻结未被追认。

独立冻结时状态：运行时结构候选，性能验证进行中。基线为菜单冻结 `ad5f87ad780763733aba2500814628149c4d0812e4b9e373fb380b99d0b2427d`；另以已验收消息冻结作累计成本对照。对话框和菜单仍分别保留原性能记录，未扩大已验收计数。

## 采样依据

在修改源码前，对原消息冻结和 scalar-isolation 冻结分别串行采样真实 PE32 messages/IR、GDI/IR、SSE/raw，每次 5 秒、1ms 间隔。复用原 150M/900M fixture，保留结果报告与完整 sample；API trace 仍格式化并计数，写 null device。采样期间没有编译或其它 Wire 运行。

证据目录 `wire-out/refactoring-2026-10-14/r2-dispatch-profile-2026-10-19/`。消息 runtime 样本 4212/3825，其中 API 路径 82.645%/82.797%；GDI 样本 4228/4229，API 路径 78.784%/77.985%。两者的格式化、字符串操作、时间查询和总分发器占据较多样本。总分发器自身分别约 6%–7%，前后的占比接近。SSE/raw 样本 4233/4212，raw fallback 80.345%/80.413%，主要在 step_x86、step_0f、SIMD descriptor 和 fetch；没有出现新增的集中热点。采样没有精确归因此前几个百分点的计时差异。

## 改动

PE32 总分发器从 `(dll, name)` 加 typed guard 链，改为直接匹配 `(bound_api_id, dll, name)`。50 个 typed arm 使用枚举模式，44 个 legacy 名称 arm 增加通配的 ID 字段。分支次序、ABI-only 优先级、两处领域 guard 和最终 fallback 均保留；因此尚未迁移入口的覆盖关系不变，包括既有 RedrawWindow 差异。

没有移动处理体或调用/返回框架。独立逐 token 核对剔除 arm header 和空白后，前后 24,934 个处理体 token 完全相同。LastError、trace、错误/回调/阻塞和非局部返回继续使用原提交顺序。没有调整编译配置、浮点语义、ISA、IR 或缓存。

完整 release 619 passed / 7 ignored、debug Win32/AMD64/FPU 465、显式 native GPU 7、严格 all-target/all-feature Clippy 与 release build 通过。候选 `r2-direct-pattern-wire/wire` SHA256 `929f495f386ef76931e75b5cc6dbfd7dc855083e7c47e8298fbc552c2a182558`，6,024,920 bytes，较菜单增加 10,992 bytes，满足本批 +24KiB 预算。源码清单和补丁与冻结 executable 同目录保存。

三标签 before=已验收消息、previous=菜单、after=本候选，按三对轮换完成 150M work 的 messages/GDI/menu/dialog IR 网关，36 次执行全部 checksum/retired/API count 校验。

| IR 网关 | CPU 中位数相对消息 | 相对菜单 | RSS 相对菜单 |
| --- | ---: | ---: | ---: |
| messages | −0.440% | +4.668% | −48KiB |
| GDI | +3.905% | −0.443% | 0 |
| menu | −4.129% | −1.454% | +64KiB |
| dialog | −13.595% | −1.325% | −16KiB |

消息 before CPU 三次为 7.612/10.990/10.161 秒，保留宿主漂移；相对菜单 +4.668% 的局部成本也继续调查，没有用累计中位数掩盖它。真实 dispatch 符号的汇编从 8159 行变为 7997 行，完整反汇编与 metrics 同样冻结；总 executable 仍增加，不能用函数行数预测总性能。

随后串行完成 300M work 的 AMD64 FP 与 PE32 SSE/raw 各三对，12 次执行全部校验。CPU 中位数相对消息冻结 −5.180%/−3.090%；SSE/raw before 三次 7.394/10.214/10.489 秒、after 9.458/10.405/9.898 秒，首对离散原样保留。未重新枚举全局 CGU/LTO/内联配置。

Alice 200M 三对均到同一预算出口，hidden HWND、314×211 和 Draw/Present 0/0 保持；CPU/wall 中位数 +3.511%/+4.666%，RSS +128KiB。证据 `r2-direct-pattern-alice`：复用已有 runtime，临时二进制软链接每次退出后删除。这一前缀没有进入绘制，仍有局部成本需要在后续集成验证中核对，当前结构候选尚未作完整性能准入。
