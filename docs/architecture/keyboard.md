# 键盘状态与布局边界

状态：已接入并验收，实现提交 `386f38a`。独立基线、完整正确性、性能成本与游戏前缀证据见[本批报告](../reports/refactoring-keyboard-2026-10-18.md)。

| 家族 | export 数 | PE32 | Win64 |
| --- | ---: | --- | --- |
| GetKeyState / GetAsyncKeyState / GetKeyboardState | 3 | user32 typed | user32 typed；kernel32/kernelbase 保留静态导入路由，动态查询仍拒绝 |
| layout / type / MapVirtualKey / key name / ToUnicode | 13 | user32 ABI-only，保留 LastError 120 和原 stdcall 清栈 | user32 typed，保留虚拟 US 布局 |

契约声明唯一调用 ID、provider、参数宽度与返回约定。三个 Win64 状态 API 的静态导入和动态查询范围不同，通过 lookup provider 显式记录。PE32 layout API 继续使用既有 ABI-only 策略；键盘能力不由 Wine spec 中存在签名来推导。

异步状态、已读取消息的队列状态、toggle、文字和线程隔离仍由同一个 Keyboard owner 管理。PE32 状态适配器只借明确参数、内存、owner 和当前 tid；异步查询继续消费原有低位标志。GetKeyboardState 只读取当前线程的快照，成功保留 LastError，访存失败沿原错误路径返回。两端返回槽别名分别保留 handler 后读取与预先保存 frame 的顺序。

布局查询使用既有 US HKL `0x04090409`，与 guest 的日语 NLS profile 分开。线程查询继续检查存活线程；HKL 在 Win64 保留完整指针宽度。纯 scan/VK、character 和 key-name 规则集中到 `win32/user/keyboard_layout.rs`，不依赖 guest 内存或进程总状态，保留 Hangover/Wine 来源和 LGPL 声明。

ToUnicode 使用调用者的 256-byte 状态，signed 容量检查先于 flags、layout 和指针访问；读取完整输入后写入输出，保留输入/输出别名。flag 范围、keyup 分支、NUL、截断及失败输出顺序保持。US profile 没有 IME、dead-key、AltGr 或组合状态；API 查询与 host 已解析的 WM_CHAR 文字仍各自按原路径处理。
