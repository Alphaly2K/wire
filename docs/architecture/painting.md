# 绘制区域与 paint DC

`win32/catalog/painting.rs` 声明六项 PE32 export：InvalidateRect、ValidateRect、GetUpdateRect、BeginPaint、EndPaint、RedrawWindow。参数宽度、stdcall 清栈、领域 ID 和架构策略由该契约提供；Win64 六项继续 absent。当前实施和验收状态见[批次报告](../reports/refactoring-painting-2026-10-19.md)。

`user32/paint.rs` 协调窗口更新矩形、erase 标记、graphics 所有的 DC/clip，以及 WM_ERASEBKGND guest 回调。窗口继续拥有 update/erase，graphics 继续拥有 DC 和像素。默认窗口过程的 WM_PAINT 与 RedrawWindow 的立即更新使用 typed 内部调用；公开 PE32 入口适配 Returned、阻塞或非局部转移，保留原有 CPU/API 提交规则。

验证顺序属于旧行为：RedrawWindow 先拒绝复杂 region 和不支持的 flags，再校验 HWND；BeginPaint 先检查 PAINTSTRUCT 的 64 字节可写性，然后创建 DC、设置 clip、清除 update/erase 并调用擦除过程。回调失败或转移后，已经发生的状态变更继续保留。PAINTSTRUCT 在正常回调返回后写入。EndPaint 读取原结构并释放对应 DC；该过程保留独立新产生的 update region，重复释放仍返回错误 6。

现有行为只支持矩形更新区域。ValidateRect 仅在请求覆盖完整旧矩形时清除区域；GetUpdateRect 保持原有可选输出与 erase 参数行为。复杂 region、非客户区绘制和额外 Win64 功能另行实现，避免改变此次迁移的可观察行为。
