# 窗口文本与输入所有权基线（2026-10-18）

状态：六项独立测试全部通过，运行时尚未切换该家族。测试在前批 memory 补证与采样结束后随完整 release 检查执行；同期修订只固定无关的 AMD64 访存 helper 内联边界，没有修改本家族行为。

后续范围十六项：Set/GetWindowTextA/W、GetWindowTextLengthA/W，以及 Get/SetFocus、Get/SetActiveWindow、Get/SetForegroundWindow、Get/SetCapture、ReleaseCapture、GetLastActivePopup。PE32 文本和输入所有权 setter 继续通过原 guest WNDPROC 协调，Win64 的原 scalar 行为与 absent 范围分别保留，不在纯重构中补充功能。

PE32 四项固定匹配 A/W WNDPROC 的 CP932/UTF-16 长度与截断、NULL/零/负容量、HWND→线程→访存检查顺序、失效文本输入不改变原值、NULL 所有权清除、重复 setter 不发送通知、重入 callback 观察已提交状态，以及 callback 故障时保留先提交的焦点。Win64 两项固定五项 absent、静态 provider 接受范围、NULL setter 的 1400 与旧 owner、统一 title 的 A/W 长度、DWORD 容量截断及访存故障。日志 `wire-out/refactoring-2026-10-14/r2-user-queries-memory-boundary-release-tests.txt`。

Win64 原查询测试的真实窗口 helper 供本家族复用，临时原创 PE 文件名增加进程内序号，避免多个测试并行创建文件时覆盖；游戏资源不参与这些测试。该测试文件变更发生在 `r2-window-queries-validation-source-verification.json` 完成之后，不属于已冻结的 375 个 Rust 文件内容；后续不使用 live 源码重新证明旧冻结。
