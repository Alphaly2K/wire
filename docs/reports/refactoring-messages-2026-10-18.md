# 消息队列、过程分发和计时器基线（2026-10-18）

状态：2026-10-19 接受，22 项契约、完整正确性、38 条性能协议/237 次执行及三对游戏前缀全部通过，累计 595 exports。日志 `r2-messages-baseline-tests.txt`。窗口控制正式矩阵和游戏验证结束后才编译独立基线，未与计时重叠；适配草稿在其补证计时期间编写，直到补证结束才触发 Cargo。

后续契约范围 22 项：SetTimer/KillTimer，PostMessageA/W、PostThreadMessageA/W、PostQuitMessage，PeekMessageA/W、GetMessageA/W、DispatchMessageA/W、TranslateMessage、WaitMessage、MsgWaitForMultipleObjects/Ex，以及 Get/SetMessageExtraInfo、GetMessageTime、GetMessagePos、GetQueueStatus。五项 PE32 metadata/queue-status 继续保留 ABI-only，六项 Win64 timer/wait/queue-status 继续 absent。

PE32 基线通过可观察的 MsgWait/Peek/Get 行为固定：失败 MSG 输出之前已清除 queue-change bits，但消息仍保留；WaitMessage 不移除消息；invalid range 与 HWND_MESSAGE 同时出现时原错误为 1400；message/object wait 参数先于指针校验；同步专用 ID 仍可 post，PostQuitMessage 原 trace carrier 返回 1。

Win64 基线固定参数 DWORD 截断和 WPARAM/LPARAM 全 64 位保留、输出失败时 metadata/CPU/API 次数不提交、成功取消息才清除 extra-info；invalid range 原错误为 87、同步专用 post 错误 1159，以及 PostQuitMessage 返回 0、exit-code DWORD 截断。既有消息、键盘、paint、阻塞线程、跨线程发送及 TIMERPROC 测试继续作为领域回归。

这些测试和未注册契约草稿不属于窗口控制此前核对的 384 Rust 文件冻结；后续不使用 live 源码重新证明旧冻结。

基线提交 `f818b33`；[调用边界](../architecture/messages.md)记录 typed 控制、ABI-only/absent 和状态归属。删除旧手工 ABI、Win64 arity/DWORD/provider 名单和普通名称执行分支，GetMessage 的阻塞和 DispatchMessage 的 callback/resume 走具名控制结果。首次构建前固定 executable 增量预算为相对 `r2-window-control-wire/wire` +24KiB。先等待前批冻结补证结束，再运行本批回归和完整检查。

原创可选 `gateway-messages` 每轮向当前线程 PostMessageW，再 GetMessageW 实际移除该消息，累加交付的 WPARAM=7；输出位于预留的 guest 栈空间，不写代码页。Win64 retired=13n+10/work=15n+10，PE32 retired=14n+9，API 均 2n，结果均 7n。旧运行时 `r2-window-control-wire/wire` 上 Win64、PE32 IR/raw 三条路径首次全部验证通过，工具 23 项通过；证据 `r2-messages-fixture-baseline{64,32}` 与 `r2-messages-fixture-tool-tests.txt`。性能工具在前批计时结束后才修改。

四项迁移后基线、完整 release 607 / 7 ignored、debug 453、显式 native 7 与 fmt、strict all-target/all-feature Clippy、build 全部通过，日志 `r2-messages-{candidate,validation}-*`，首次迁移编译及完整检查均无错误。388 个 Rust 文件（src/tests 380）live/frozen/manifest 与文件集合一致，target/frozen executable SHA 相同。冻结 `r2-messages-wire/wire`：SHA256 `e4dc65862bc32f2aecb7da78efc58322a5c994891e5218048ed2ecc376e989db`，5,970,440 bytes，相对前批 +5,728 bytes，满足预设 +24KiB 预算；见 `r2-messages-validation-source-verification.json`。

正式矩阵保留双架构基础执行器、当前 post/get、真实 WNDPROC、文本与窗口字段，以及 thread/GUID/processor/GDI 代表网关，共 35 条路径。按计划对每条运行至少三对并核对结果、CPU/wall/RSS；出现离散或越过门槛时再定向扩样。该范围覆盖本批改动涉及的队列和控制路由，不把所有历史网关不断叠加为每批必跑项目。Alice 继续同 200M 启动前缀，编译、计时和原生游戏串行。

初筛中 PE32 SSE/IR CPU 中位数 +10.556%，三对分别约 +21.5%/+5.3%/−2.3%，触发调查；PE32 FP/IR −29.385%、AMD64 FP −20.113% 也存在较大的路径差异。源码未改 ISA/FP 热循环，尚不将这些差异归因于语义优化。待本轮游戏结束，使用长样本和同哈希旧二进制 control 复核；本批仍未验收。

900M work 三标签补证的 PE32 SSE/IR 相对 before −0.246%、旧程序 control/before +4.965%、after/control −4.964%；最初短样本回退未持续。PE32 FP/IR 相对 before −25.286%、control/before −0.807%、after/control −24.678%。这些样本仍属于纯重构的布局/宿主性能观测，ISA 源码没有变化，未据此推断完整游戏收益。原始补证在 `r2-messages-expanded-pe32`；AMD64 FP 补证完成后再作最终接受决定。

AMD64 FP 的 900M 三标签补证相对 before −19.561%，control/before +0.834%、after/control −20.226%。SSE/IR 的 after/before 逐对 +37.837%/+5.967%/−10.949% 及 control 时间漂移全部保留；其中首轮 before 明显快于同哈希 control，长样本未形成三对持续回退。其余初筛路径最大 CPU 中位数回退 +2.335%，post/get 网关 Win64/PE32 IR/raw 分别 −1.417%/−10.391%/−1.827%。初筛 RSS 中位数最多 +272KiB，扩样最多 +64KiB，未增加缓存容量。汇总 `r2-messages-{fixed,expanded}-summary.json`，ISA 源文件差异审计 `r2-messages-runtime-source-differences.json`。

Alice 200M 三对均通过同一预算出口：hidden、client 314×211、Draw/Present 0/0，CPU −0.907%、wall −7.945%、RSS +96KiB；保留启动时间离散和实际 API/retired 数，不宣称路径逐指令一致。复用现有 runtime，证据 `r2-messages-fixed-alice/compare.json`。本批通过性能门槛，后续绘制基线与源码修改不属于此前已核对的消息冻结。
