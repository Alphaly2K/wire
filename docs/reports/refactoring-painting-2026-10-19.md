# 绘制区域与 paint DC 基线（2026-10-19）

状态：三项独立旧行为基线和完整正确性通过，六项契约和 typed paint 适配已接入，尚待性能验收。消息队列长样本补证期间仅编辑运行时源码和文档，结束后才开始 Cargo，编译及性能工具修改保持串行。

范围为 InvalidateRect、ValidateRect、GetUpdateRect、BeginPaint、EndPaint、RedrawWindow。保留 PE32 矩形 update/erase、GDI clip/DC 资源与 guest 回调协调，六项 Win64 export 保持 absent，不扩展复杂 region 或非客户区绘制。

两项 PE32 基线固定 RedrawWindow flags/region 验证先于 HWND/矩形访存、PAINTSTRUCT 输出预检失败时保留 update region、WM_ERASEBKGND 失败前已清除 update/erase 但未写输出，以及 EndPaint 释放 DC 而保留后来独立产生的 update region，重复 EndPaint 仍报告错误 6。Win64 一项固定 user32/kernel32/kernelbase 的原 absent 范围。既有真实像素、背景擦除、clip、合成 paint 消息及默认过程回归继续保留。

这些测试和未注册契约不属于消息队列此前核对的 388 Rust 文件冻结。先等待消息性能与游戏执行结束，再编译旧行为基线；运行时迁移必须在基线通过之后开始。

消息初筛与游戏已串行结束；在首次构建候选前固定 executable 增量预算为相对 `r2-messages-wire/wire` +24KiB。新增可选 `gateway-painting`：实际建立 32×32 窗口，每轮 InvalidateRect→BeginPaint→EndPaint，PAINTSTRUCT 位于 guest 栈，累加 EndPaint 返回值并最终销毁窗口。独立旧程序验证后再用于比较。

旧消息冻结上原创 fixture 的 IR/raw 首次全部校验通过：n=100，retired=1,341、API=303、checksum=101；工具 23 项及三项独立 painting 测试通过。证据 `r2-painting-fixture-baseline`、`r2-painting-tool-tests.txt` 与 `r2-painting-baseline-tests.txt`。此时旧 paint handler 仍在运行，测试先于 typed 注册。

基线提交 `0626057`。本批删除六项旧手工 ABI 和 `user32_paint` 名称入口，默认 WM_PAINT 的 Begin/End 也使用 typed ID；PE32 的返回/转移适配仍在原调用边界提交。矩形区域、PAINTSTRUCT、DC 和擦除回调原分支保留，边界见[绘制说明](../architecture/painting.md)。

完整 release 610 / 7 ignored、debug 456、显式 native 7，fmt、strict all-target/all-feature Clippy 与 build 全部通过，首次候选构建没有错误。391 个 Rust 文件（src/tests 383）live/frozen/manifest 和文件集合一致，target/frozen SHA 相同。冻结 `r2-painting-wire/wire` SHA256 `7342f483c9a5f7e6b20789eae2674d413e3aeaf55ddf96754038ab7736951a13`，5,973,144 bytes，较消息冻结 +2,704 bytes，在 +24KiB 预算内。日志 `r2-painting-validation-*`，来源核对 `r2-painting-validation-source-verification.json`。

正式初筛 28 条路径：双架构基础执行器与 FP/bit/cache-alias；Win64 控制/消息/GUID；PE32 实际 paint 循环、窗口过程控制、消息、GDI/GUID 的 IR/raw。每条三对，超过门槛再定向扩样；Alice 仍使用同一 200M 预算和现有 runtime，全部计时与编译、诊断串行。
