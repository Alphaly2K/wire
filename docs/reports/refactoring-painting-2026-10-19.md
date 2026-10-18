# 绘制区域与 paint DC 基线（2026-10-19）

状态：三项独立旧行为基线通过，六项契约和 typed paint 适配待接入。消息队列长样本补证期间仅编辑运行时源码和文档，编译及性能工具修改保持串行。

范围为 InvalidateRect、ValidateRect、GetUpdateRect、BeginPaint、EndPaint、RedrawWindow。保留 PE32 矩形 update/erase、GDI clip/DC 资源与 guest 回调协调，六项 Win64 export 保持 absent，不扩展复杂 region 或非客户区绘制。

两项 PE32 基线固定 RedrawWindow flags/region 验证先于 HWND/矩形访存、PAINTSTRUCT 输出预检失败时保留 update region、WM_ERASEBKGND 失败前已清除 update/erase 但未写输出，以及 EndPaint 释放 DC 而保留后来独立产生的 update region，重复 EndPaint 仍报告错误 6。Win64 一项固定 user32/kernel32/kernelbase 的原 absent 范围。既有真实像素、背景擦除、clip、合成 paint 消息及默认过程回归继续保留。

这些测试和未注册契约不属于消息队列此前核对的 388 Rust 文件冻结。先等待消息性能与游戏执行结束，再编译旧行为基线；运行时迁移必须在基线通过之后开始。

消息初筛与游戏已串行结束；在首次构建候选前固定 executable 增量预算为相对 `r2-messages-wire/wire` +24KiB。新增可选 `gateway-painting`：实际建立 32×32 窗口，每轮 InvalidateRect→BeginPaint→EndPaint，PAINTSTRUCT 位于 guest 栈，累加 EndPaint 返回值并最终销毁窗口。独立旧程序验证后再用于比较。

旧消息冻结上原创 fixture 的 IR/raw 首次全部校验通过：n=100，retired=1,341、API=303、checksum=101；工具 23 项及三项独立 painting 测试通过。证据 `r2-painting-fixture-baseline`、`r2-painting-tool-tests.txt` 与 `r2-painting-baseline-tests.txt`。此时旧 paint handler 仍在运行，测试先于 typed 注册。
