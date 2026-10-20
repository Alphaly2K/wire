# 绘制区域与 paint DC

InvalidateRect、ValidateRect、GetUpdateRect、BeginPaint、EndPaint 五项 PE32 export 的 DLL、参数和 typed ID 集中在 `win32/catalog/painting.rs`。当前实现已接入，验证与性能状态见[后续报告](../reports/refactoring-painting-followup-2026-10-19.md)。Win64 五项继续 absent。早期六项候选的撤回记录仍见[历史报告](../reports/refactoring-painting-2026-10-19.md)。

RedrawWindow 的真实 API 网关归 MenuApi::Redraw，保留 error 120 与菜单未实现诊断；公开 `user32::dispatch` facade 进入本页描述的矩形绘制路径。两者是现有入口差异，不将 RedrawWindow 纳入五项 paint 契约。独立菜单回归同时验证两个入口。

`win32/user/pe32/paint.rs` 只借内存、Graphics 和 LastError，准备更新矩形、erase 标记及 DC/clip，并产生值、立即更新或 BeginPaint 请求；它不依赖 CPU、ApiState 或 guest 执行器。窗口拥有 update/erase，graphics 拥有 DC 和像素。`x86/process/painting.rs` 执行 WM_ERASEBKGND，正常返回后完成 PAINTSTRUCT 写入；立即更新继续委托 typed 窗口控制。绑定后的网关、默认窗口过程 WM_PAINT 与公开名称 facade 共用该边界。PE32 适配器遇到 Returned 后才提交普通 API 返回；阻塞或非局部转移保持原 CPU/API 提交规则。

早期迁移候选改变了软件浮点的实际内联，AMD64 FP 三对持续回退；该候选及其模块命名、内联和编译配置试验均已撤回。后续精确浮点编译边界独立验收，本次五项迁移不重新引入这些试验。宿主后台渲染对性能测量的影响另在后续报告记录。

验证顺序属于契约：五项 paint 均先校验 HWND，再访问 guest RECT/PAINTSTRUCT；RedrawWindow facade 先拒绝复杂 region 和不支持的 flags，再校验 HWND。BeginPaint 先检查 PAINTSTRUCT 的 64 字节可写性，然后创建 DC、设置 clip、清除 update/erase 并调用擦除过程。回调失败或转移后，已经发生的状态变更继续保留。PAINTSTRUCT 在正常回调返回后写入。EndPaint 读取原结构并释放对应 DC；该过程保留独立新产生的 update region，重复释放仍返回错误 6。

普通网关在 handler 输出之后读取返回地址，因此 PAINTSTRUCT 或 RECT 覆盖返回槽时必须使用覆盖后的值。成功路径保持原 LastError；错误 DLL 保持未知 ABI、零参数清栈及原未实现诊断，不新增 kernel32/kernelbase 别名。

现有行为只支持矩形更新区域。ValidateRect 仅在请求覆盖完整旧矩形时清除区域；GetUpdateRect 保持原有可选输出与 erase 参数行为。复杂 region、非客户区绘制和额外 Win64 功能另行实现，避免改变此次迁移的可观察行为。

请求边界基线 `94eadcb`。准备算法、失效矩形、回调与后置输出核对保持，Graphics 和 callback frame 未改；release 完整 Win32 257、IR 31、fmt/release lib/bin strict Clippy/build 通过。普通职责提取不重复性能或游戏矩阵，证据 `wire-out/refactoring-2026-10-14/r4-paint32-{source-checks,verification}.json`。
