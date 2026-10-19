# 对话框控件与窗口枚举基线（2026-10-19）

状态：本批结构随 `b433c63` 日志修订累计验收，见 [33 条性能协议与游戏前缀](refactoring-api-log-2026-10-19.md)。以下保留各独立冻结的历史数据与准入结论；旧失败冻结未被追认。

独立冻结时状态：四项独立旧行为基线及完整正确性通过，22 项契约及 typed 适配已接入，性能仍待复核。绘制首版因持续性能回退停止计时后才编译基线，日志 `r2-dialog-baseline-tests.txt`。

范围为 Get/SetDlgItemTextA/W、SendDlgItemMessageA/W、GetDlgItem、GetDlgCtrlID、Get/SetDlgItemInt、EnumChildWindows、EnumWindows、EnumThreadWindows，以及原来明确返回错误 120 的九项 USER 服务。Win64 全部继续 absent。九项 unavailable 仍由 typed 领域处理，以保留原 `USER32 error 120` note 和 unimplemented trace；通用 ABI-only 返回路径的 note 不同，因此不直接套用。

独立基线固定 child/thread 检查先于输出访存、GetDlgItemInt 在 WM_GETTEXT 故障前清零 translated 输出、枚举 callback=NULL 的不同检查规则，以及 callback FALSE 在顶层/子窗口枚举中不同的返回 carrier。明确不可用的服务继续在任何 guest 输入访问前失败；既有文本/有符号十进制边界、枚举快照与回调控制测试继续保留。

这些后续草稿不属于绘制已核对的 391 个 Rust 文件冻结。先结束绘制计时和游戏，再编译旧行为基线，基线通过后才注册迁移。

原创可选 `gateway-dialog` 创建父/子窗口，每轮 GetDlgItem 后将实际返回值与子窗口 handle 做 XOR 并累积 OR，最终分别销毁两个窗口。n=work//7，API=n+5，retired=7n+65，checksum=0。首次旧程序运行因把终止 int3 计入退休数导致期望多 1，失败日志 `r2-dialog-fixture-baseline` 保留，修正该独立计数后再验证 IR/raw。

修正后在旧消息冻结上 n=100 的 IR/raw 首次全部校验通过：retired=765、API=105、checksum=0；工具 23 项通过。证据 `r2-dialog-fixture-baseline-corrected`、`r2-dialog-tool-tests.txt`。领域边界见[对话框说明](../architecture/dialog.md)，typed 草稿仍未注册。

绘制与固定家族 iterator 已独立撤回，运行时重新构建精确恢复已验收消息冻结 SHA；该回滚提交为 `8ea8b97`。随后才接入本批：删除 22 项手写 ABI、原 common 字符串分发和单独枚举实现，ApiId::Dialog 直接进入 typed handler，公开名称入口保留冷兼容委托。九项 unavailable 的 error 120/note/implemented=false 原样提交。构建前将 executable 增量上限固定为相对消息冻结 +24KiB；先跑完整 release 正确性、冻结并筛查已知敏感 FP，源码和正式计时继续串行。

原结构候选完整 release 614 passed / 7 ignored，393 个 Rust 文件核对一致，冻结 `r2-dialog-wire` SHA256 `2448f94606a75c537536dd72c5290d1aed88811cd7db5e91f03786848763b6ad`，5,974,936 bytes，增加 4,496 bytes。三对 300M FP CPU 为 6.1136/6.0237/6.0292 秒，before 为 4.7838/4.8837/4.7524 秒，持续回退，原结构候选拒绝。计时结束后才开始[纯浮点编译隔离](refactoring-scalar-codegen-2026-10-19.md)，保留 `r2-dialog-fp-screen` 的所有失败样本。本批继续与该结构边界共同验证，累计验收尚未增加。

隔离候选完整 release 614 / debug 460 / native 7 与严格检查通过，395 个 Rust 文件核对一致。28 条初筛全部结果校验，七条超过调查门槛；后续宿主同哈希耗时漂移超过两倍，停止剩余计时，详见隔离报告。菜单四项基线在计时退出后单独通过，未改变候选运行时 SHA。Alice 新旧各一次 200M 功能 smoke 均保持 hidden 314×211、Draw/Present 0/0，证据 `r2-dialog-functional-alice`；这两个功能样本的耗时不参与性能验收，正式三对游戏对照仍待执行。
