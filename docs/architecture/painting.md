# 绘制区域与 paint DC

新 Wasm 入口在 PE32/Win64 接入 InvalidateRect、ValidateRect、GetUpdateRect、BeginPaint、EndPaint 和 UpdateWindow，provider 为 user32.dll。共享目录的旧 Win64 absent 策略保持；新运行时显式扩展已迁移入口。RedrawWindow、复杂 region 与非客户区绘制未绑定。

窗口保存 client 坐标中的矩形更新区域和 erase 请求。InvalidateRect 将输入与 client bounds 相交，再合并包围矩形；erase 在区域间累积。ValidateRect 的 NULL 输入或完全覆盖旧包围矩形的输入清除区域，部分覆盖暂不做矩形减法。所有绘制 API 先验证 HWND 和当前线程所有权，再访问客体 RECT/PAINTSTRUCT；无效窗口返回 1400，跨线程调用返回 50。

BeginPaint 先检查完整输出和必要的回调 frame，再取得带更新区域 clip 的实际 DC，消耗当次区域并按需调用 WM_ERASEBKGND。回调在同一 JIT、线程和预算中执行；期间新产生的失效区域独立保留。正常返回后输出 PAINTSTRUCT，擦除返回非零时 fErase 为零。后续输出故障保留已完成的擦除和 DC，修复后继续输出；不会重放擦除过程。PAINTSTRUCT 在 PE32 为 64 字节，Win64 为 72 字节，HDC 保留指针宽度。

GetUpdateRect 不消耗区域；可选 erase 使用临时 clipped DC 调用同一擦除过程，成功后仅在区域版本未改变时清除旧 erase 请求。回调中新产生的 erase 请求会保留。输出故障保留已完成过程和临时 DC；成功输出后释放 DC。输出 RECT 是发起查询时的快照。

EndPaint 读取完整 PAINTSTRUCT，验证 DC 的窗口、线程及 paint 类型后释放；不会清除其间新产生的区域。重复释放返回错误 6。ReleaseDC 不接受 paint DC。窗口销毁和线程退出清理相应 DC；AMD64 展开越过尚未完成的 paint 续点时也归还该 DC。

UpdateWindow 在存在区域时同步发送 WM_PAINT，没有区域时直接成功。默认 WM_PAINT 使用同一消耗/擦除/释放流程；默认 WM_ERASEBKGND 按类背景 brush 填写实际像素并服从 clip。可见窗口创建、显示和允许重绘的位置变更会失效 client；尺寸变更保留 backing 的相交像素。画面快照通过 [windowPixels 与 BrowserPresenter](input-events.md) 在暂停边界送入浏览器画布，根进程与子进程按 PID/HWND 区分。

可见脏窗口在消息查询和请求 QS_PAINT 的等待中合成一条 WM_PAINT。验证、消费区域和销毁会清除旧的合成消息；显式 PostMessage 的 WM_PAINT 独立保留。队列满时保留更新区域供后续查询，详见[消息契约](messages.md)。

调用帧在输出前捕获。RECT/PAINTSTRUCT 覆盖返回槽时仍使用原返回地址与清栈量，成功路径保持 LastError。
