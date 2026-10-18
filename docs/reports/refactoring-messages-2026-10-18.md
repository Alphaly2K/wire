# 消息队列、过程分发和计时器基线（2026-10-18）

状态：四项独立旧行为基线通过，运行时消息 handler 尚未迁移。日志 `r2-messages-baseline-tests.txt`。窗口控制正式矩阵和游戏验证结束后才编译，未与计时重叠。

后续契约范围 22 项：SetTimer/KillTimer，PostMessageA/W、PostThreadMessageA/W、PostQuitMessage，PeekMessageA/W、GetMessageA/W、DispatchMessageA/W、TranslateMessage、WaitMessage、MsgWaitForMultipleObjects/Ex，以及 Get/SetMessageExtraInfo、GetMessageTime、GetMessagePos、GetQueueStatus。五项 PE32 metadata/queue-status 继续保留 ABI-only，六项 Win64 timer/wait/queue-status 继续 absent。

PE32 基线通过可观察的 MsgWait/Peek/Get 行为固定：失败 MSG 输出之前已清除 queue-change bits，但消息仍保留；WaitMessage 不移除消息；invalid range 与 HWND_MESSAGE 同时出现时原错误为 1400；message/object wait 参数先于指针校验；同步专用 ID 仍可 post，PostQuitMessage 原 trace carrier 返回 1。

Win64 基线固定参数 DWORD 截断和 WPARAM/LPARAM 全 64 位保留、输出失败时 metadata/CPU/API 次数不提交、成功取消息才清除 extra-info；invalid range 原错误为 87、同步专用 post 错误 1159，以及 PostQuitMessage 返回 0、exit-code DWORD 截断。既有消息、键盘、paint、阻塞线程、跨线程发送及 TIMERPROC 测试继续作为领域回归。

这些测试和未注册契约草稿不属于窗口控制此前核对的 384 Rust 文件冻结；后续不使用 live 源码重新证明旧冻结。
