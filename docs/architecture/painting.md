# 绘制区域与 paint DC

六项 PE32 export 为 InvalidateRect、ValidateRect、GetUpdateRect、BeginPaint、EndPaint、RedrawWindow。typed 契约迁移因持续浮点性能回退撤回，当前继续使用 `user32_paint.rs` 和原 PE32 ABI 表；Win64 六项继续 absent。候选源码和验证证据见[批次报告](../reports/refactoring-painting-2026-10-19.md)。

后续重试应只迁移前五项：RedrawWindow 的真实 API 网关已经归 MenuApi::Redraw，保留 error 120；公开 `user32::dispatch` facade 才调用本页描述的矩形绘制路径。新增独立故障/返回 frame 用例已在旧实现通过；两轮同二进制控制仍明显离散，五项运行时迁移尚未实施，见[后续基线与控制记录](../reports/refactoring-painting-followup-2026-10-19.md)。

`user32_paint.rs` 协调窗口更新矩形、erase 标记、graphics 所有的 DC/clip，以及 WM_ERASEBKGND guest 回调。窗口继续拥有 update/erase，graphics 继续拥有 DC 和像素。默认窗口过程的 WM_PAINT 使用原字符串入口，RedrawWindow 的立即更新委托既有 typed 窗口控制。公开 PE32 入口适配 Returned、阻塞或非局部转移，保留原有 CPU/API 提交规则。

首个迁移候选改变了软件浮点的实际内联，AMD64 FP 三对持续回退。模块命名、内联提示、固定家族 iterator 和编译配置试验均未通过，所有运行时试验已撤回；保留独立故障回归和原创 paint 循环，后续具备稳定编译边界时再迁移。

验证顺序属于旧行为：RedrawWindow 先拒绝复杂 region 和不支持的 flags，再校验 HWND；BeginPaint 先检查 PAINTSTRUCT 的 64 字节可写性，然后创建 DC、设置 clip、清除 update/erase 并调用擦除过程。回调失败或转移后，已经发生的状态变更继续保留。PAINTSTRUCT 在正常回调返回后写入。EndPaint 读取原结构并释放对应 DC；该过程保留独立新产生的 update region，重复释放仍返回错误 6。

现有行为只支持矩形更新区域。ValidateRect 仅在请求覆盖完整旧矩形时清除区域；GetUpdateRect 保持原有可选输出与 erase 参数行为。复杂 region、非客户区绘制和额外 Win64 功能另行实现，避免改变此次迁移的可观察行为。
