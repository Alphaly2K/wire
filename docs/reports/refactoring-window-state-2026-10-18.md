# 窗口文本与输入所有权基线（2026-10-18）

状态：2026-10-18 完成验收，十六项计入累计，合计 556 exports。下文保留基线、接入和完整性能证据。

后续范围十六项：Set/GetWindowTextA/W、GetWindowTextLengthA/W，以及 Get/SetFocus、Get/SetActiveWindow、Get/SetForegroundWindow、Get/SetCapture、ReleaseCapture、GetLastActivePopup。PE32 文本和输入所有权 setter 继续通过原 guest WNDPROC 协调，Win64 的原 scalar 行为与 absent 范围分别保留，不在纯重构中补充功能。

PE32 四项固定匹配 A/W WNDPROC 的 CP932/UTF-16 长度与截断、NULL/零/负容量、HWND→线程→访存检查顺序、失效文本输入不改变原值、NULL 所有权清除、重复 setter 不发送通知、重入 callback 观察已提交状态，以及 callback 故障时保留先提交的焦点。Win64 两项固定五项 absent、静态 provider 接受范围、NULL setter 的 1400 与旧 owner、统一 title 的 A/W 长度、DWORD 容量截断及访存故障。日志 `wire-out/refactoring-2026-10-14/r2-user-queries-memory-boundary-release-tests.txt`。

Win64 原查询测试的真实窗口 helper 供本家族复用，临时原创 PE 文件名增加进程内序号，避免多个测试并行创建文件时覆盖；游戏资源不参与这些测试。该测试文件变更发生在 `r2-window-queries-validation-source-verification.json` 完成之后，不属于已冻结的 375 个 Rust 文件内容；后续不使用 live 源码重新证明旧冻结。

唯一契约、双架构 typed adapter 与 PE32 显式 callback coordinator 已接入，六项独立回归再次全部通过。移除旧普通文本/焦点名称分支和重复 ABI，dialog wrappers 保留；User32 状态提交先于通知，逐条回调后重新解析下一窗口的 WNDPROC，Win64 原 scalar 行为独立保留。[边界说明](../architecture/window-state.md)记录原两架构差异。本批 executable 增量预算预先固定为相对已验收 before +16KiB；前一冻结的完整矩阵、memory 与游戏复测结束后开始本批编译，后续补证仍使用小型冻结程序串行运行。

原创 `gateway-window-state` 在旧 runtime 冻结上验证 Win64 和 PE32 IR/raw 三条路径。两端都实际创建/销毁窗口并消费长度；PE32 使用应用自行返回 3 的 WNDPROC，查询每次执行两条 guest 指令，Win64 读取三字符标题。Win64 retired=5n+63/work=6n+66/API=n+3，PE32 retired=7n+36/API=n+3，返回均为 3n+1。首次 PE32 统计多计终止 INT3 一条，修正后独立旧运行时验证通过，原日志保留；没有按新实现推导期望。工具 23 项通过。证据 `r2-window-state-fixture-baseline64`、`r2-window-state-fixture-baseline32{,-corrected}` 和 `r2-window-state-fixture-tool-tests.txt`。

实现提交 `fe6b552`。完整 release 600 / 7 ignored、debug 446、显式 native 7，以及 fmt、strict all-target/all-feature Clippy、build 均通过；日志 `r2-window-state-validation-*`。380 个 Rust 文件（src/tests 372 个）live/frozen/manifest 与文件集合一致，target/frozen executable SHA 相同。冻结 `r2-window-state-wire/wire`：SHA256 `0c4b58c61096fcb39dd122ac02e87919a14afec2d594b1f61f9b3c6679c43bd1`，5,977,432 bytes，相对前批调用边界修订 +6,736 bytes，满足预先固定 +16KiB 预算。

正式初筛 54 条路径、324 次执行全部校验；新长度查询网关 Win64 CPU +1.614%，PE32 IR/raw −25.612%/−22.253%，RSS 中位数最多 +304KiB。初筛矩形、integer/IR、SSE/IR、properties/raw 和 rectangle/raw 存在离散或成本，追加三倍 work、三标签轮换的五条协议与 45 次执行。共 59 个协议、369 次校验执行，初筛和扩样不跨不同 work 混算。证据 `r2-window-state-fixed-summary.{json,txt}`、`r2-window-state-expanded-summary.{json,txt}`。

扩样 PE32 integer/IR、SSE/IR、properties/raw、rectangle/raw CPU 分别 +0.219%/−0.104%/+0.971%/+0.702%，相对相同旧二进制 control 分别 +0.021%/+0.101%/+0.362%/+0.765%。properties/raw 保留一对 +5.621%，其余两对 −0.070%/+0.971%；没有连续三对超过回退门槛。Win64 rectangle 的 after/before 中位数 −55.724%，但旧二进制 control/before 同样为 −55.810%，after/control 为 +0.193%；逐对 −53.997%/−56.243%/−0.248% 全部保留，不归因为矩形实现收益。

Alice 200M 三对、六次运行全部校验，保持隐藏 client 314×211、Draw/Present 0/0 和预算出口。CPU +0.083%，wall +10.116%，RSS +240KiB；wall 的启动离散成本保留，不推断 FPS。证据 `r2-window-state-fixed-alice/compare.json`。复用原 runtime，未复制或硬链接游戏素材；编译、计时、原生游戏均串行。下一批窗口控制测试与运行时草稿不属于本批已核对的 380 文件快照。
