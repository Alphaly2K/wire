# 消息队列、过程分发和计时器基线（2026-10-18）

状态：22 项契约、适配及完整正确性检查通过，尚待性能验收。日志 `r2-messages-baseline-tests.txt`。窗口控制正式矩阵和游戏验证结束后才编译独立基线，未与计时重叠；适配草稿在其补证计时期间编写，直到补证结束才触发 Cargo。

后续契约范围 22 项：SetTimer/KillTimer，PostMessageA/W、PostThreadMessageA/W、PostQuitMessage，PeekMessageA/W、GetMessageA/W、DispatchMessageA/W、TranslateMessage、WaitMessage、MsgWaitForMultipleObjects/Ex，以及 Get/SetMessageExtraInfo、GetMessageTime、GetMessagePos、GetQueueStatus。五项 PE32 metadata/queue-status 继续保留 ABI-only，六项 Win64 timer/wait/queue-status 继续 absent。

PE32 基线通过可观察的 MsgWait/Peek/Get 行为固定：失败 MSG 输出之前已清除 queue-change bits，但消息仍保留；WaitMessage 不移除消息；invalid range 与 HWND_MESSAGE 同时出现时原错误为 1400；message/object wait 参数先于指针校验；同步专用 ID 仍可 post，PostQuitMessage 原 trace carrier 返回 1。

Win64 基线固定参数 DWORD 截断和 WPARAM/LPARAM 全 64 位保留、输出失败时 metadata/CPU/API 次数不提交、成功取消息才清除 extra-info；invalid range 原错误为 87、同步专用 post 错误 1159，以及 PostQuitMessage 返回 0、exit-code DWORD 截断。既有消息、键盘、paint、阻塞线程、跨线程发送及 TIMERPROC 测试继续作为领域回归。

这些测试和未注册契约草稿不属于窗口控制此前核对的 384 Rust 文件冻结；后续不使用 live 源码重新证明旧冻结。

基线提交 `f818b33`；[调用边界](../architecture/messages.md)记录 typed 控制、ABI-only/absent 和状态归属。删除旧手工 ABI、Win64 arity/DWORD/provider 名单和普通名称执行分支，GetMessage 的阻塞和 DispatchMessage 的 callback/resume 走具名控制结果。首次构建前固定 executable 增量预算为相对 `r2-window-control-wire/wire` +24KiB。先等待前批冻结补证结束，再运行本批回归和完整检查。

原创可选 `gateway-messages` 每轮向当前线程 PostMessageW，再 GetMessageW 实际移除该消息，累加交付的 WPARAM=7；输出位于预留的 guest 栈空间，不写代码页。Win64 retired=13n+10/work=15n+10，PE32 retired=14n+9，API 均 2n，结果均 7n。旧运行时 `r2-window-control-wire/wire` 上 Win64、PE32 IR/raw 三条路径首次全部验证通过，工具 23 项通过；证据 `r2-messages-fixture-baseline{64,32}` 与 `r2-messages-fixture-tool-tests.txt`。性能工具在前批计时结束后才修改。

四项迁移后基线、完整 release 607 / 7 ignored、debug 453、显式 native 7 与 fmt、strict all-target/all-feature Clippy、build 全部通过，日志 `r2-messages-{candidate,validation}-*`，首次迁移编译及完整检查均无错误。388 个 Rust 文件（src/tests 380）live/frozen/manifest 与文件集合一致，target/frozen executable SHA 相同。冻结 `r2-messages-wire/wire`：SHA256 `e4dc65862bc32f2aecb7da78efc58322a5c994891e5218048ed2ecc376e989db`，5,970,440 bytes，相对前批 +5,728 bytes，满足预设 +24KiB 预算；见 `r2-messages-validation-source-verification.json`。

正式矩阵保留双架构基础执行器、当前 post/get、真实 WNDPROC、文本与窗口字段，以及 thread/GUID/processor/GDI 代表网关，共 35 条路径。按计划对每条运行至少三对并核对结果、CPU/wall/RSS；出现离散或越过门槛时再定向扩样。该范围覆盖本批改动涉及的队列和控制路由，不把所有历史网关不断叠加为每批必跑项目。Alice 继续同 200M 启动前缀，编译、计时和原生游戏串行。
