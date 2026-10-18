# 对话框控件与窗口枚举基线（2026-10-19）

状态：四项独立旧行为基线通过，未注册的 22 项契约及适配草稿已准备，运行时未切换。绘制首版因持续性能回退停止计时后才编译基线，日志 `r2-dialog-baseline-tests.txt`。

范围为 Get/SetDlgItemTextA/W、SendDlgItemMessageA/W、GetDlgItem、GetDlgCtrlID、Get/SetDlgItemInt、EnumChildWindows、EnumWindows、EnumThreadWindows，以及原来明确返回错误 120 的九项 USER 服务。Win64 全部继续 absent。九项 unavailable 仍由 typed 领域处理，以保留原 `USER32 error 120` note 和 unimplemented trace；通用 ABI-only 返回路径的 note 不同，因此不直接套用。

独立基线固定 child/thread 检查先于输出访存、GetDlgItemInt 在 WM_GETTEXT 故障前清零 translated 输出、枚举 callback=NULL 的不同检查规则，以及 callback FALSE 在顶层/子窗口枚举中不同的返回 carrier。明确不可用的服务继续在任何 guest 输入访问前失败；既有文本/有符号十进制边界、枚举快照与回调控制测试继续保留。

这些后续草稿不属于绘制已核对的 391 个 Rust 文件冻结。先结束绘制计时和游戏，再编译旧行为基线，基线通过后才注册迁移。
