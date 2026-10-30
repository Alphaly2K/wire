# 宿主窗口与输入边界

新 Web 入口通过 `windows(processId?)` 枚举已完成创建的顶层窗口，返回独立记录：`handle` 为完整 BigInt HWND，`title` 为字符串，`width/height` 为 client 像素尺寸，`visible/topmost` 为布尔值。省略 PID 查询当前入口所属进程；显式 PID 从同一进程组选择 owner，未知、零或非 DWORD 身份拒绝。装载前返回空数组，创建未完成或已销毁的窗口不发布。浏览器展示层通过以下接口取得窗口状态、GDI 像素及输入队列。

`windowPixels(window, processId?)` 接受 BigInt HWND，返回独立的 `{width, height, stride, rgba}`，其中 `rgba` 是按行从上到下、每像素四字节的 RGBA8 副本，alpha 固定 255，stride 为 width×4。尚未创建 backing、已销毁或未知窗口返回 null；修改返回数组不影响客体像素。`windowOrigin(window, processId?)` 返回两个 signed DWORD 的 client screen 原点，未知或已退休窗口返回 null。装载前允许查询，进程组运行中及 VM 销毁后拒绝。查询不执行客体指令或读取时钟。

底层 `wire_window_pixels(owner, hwnd)` 返回字节长度，零表示无 backing，失败为 `u32::MAX`；公共暂存区的 16 字节头包含四个 little-endian u32：width、height、stride、format=1，随后为 RGBA8。宿主应在下一次使用暂存区前复制输出。

`queueWindowEvents(events, processId?)` 批量供给最多 256 项输入，`queueWindowEvent(event, processId?)` 是单项形式。PID 与 HWND 共同确定接收 owner，不按低位 HWND 跨进程匹配。字段如下：

| 字段 | 契约 |
| --- | --- |
| window | unsigned 64-bit BigInt HWND；过期或未知完整身份被忽略，不截断后重新匹配 |
| message | DWORD；接受激活/焦点 6..8、WM_CLOSE、WM_ACTIVATEAPP、键盘 0x100..0x109、鼠标 0x200..0x20e 和 WM_APP 0x8000..0xffff |
| wparam/lparam | unsigned 64-bit BigInt，默认零；入队时按客体指针宽度规范化 |
| time | 可选 DWORD 毫秒；省略时同一批次共享一次单调时钟样本 |
| point | 可选 `[x,y]`，每项为 signed DWORD screen 像素 |

接口要求 VM 暂停执行且已装载；执行中拒绝，销毁后拒绝。先验证完整批次的格式、消息类别和容量，再更新键盘/游标与入队；无效项不会留下前缀消息。每进程队列最多 4096 项，容量按整个输入批次保守预约，包括可能被忽略或合并的项，以及 RawInput 额外产生的 WM_INPUT；数据包容量和完整句柄范围同样在发布前检查。输入不执行客体指令，不改变 CPU/work；成功批次通知调度器重新检查等待线程。

没有显式 point 时，普通鼠标消息从 lParam 的两个 signed WORD client 坐标加窗口 client origin 得到 screen 坐标；滚轮 lParam 本身使用 screen 坐标。其他消息保存当前逻辑游标。每项保存当次快照，后续移动不会修改旧 MSG.pt；GetMessagePos 按两个 WORD 打包返回。同线程 focus/capture 路由已接入，capture 转发保留 screen 快照并换算接收窗口的 client lParam；滚轮坐标保持 screen。焦点和激活事件在入队时更新对应 owner，见[窗口状态](window-state.md)。ClipCursor 对屏幕位置、接收窗口坐标及 RawInput 使用同一裁剪结果；child 和跨线程输入所有权仍待接入。

共享 Keyboard owner 接收物理 VK、扫描码和状态位；宿主应另提供其实际布局解析的 UTF-16 WM_CHAR/WM_SYSCHAR，并紧随对应 key-down 送入。配对文本缓存到 TranslateMessage 时才生成字符消息，保留代理对；Unicode 窗口使用 UTF-16，ANSI 窗口使用 CP932。没有宿主文本时只保留控制字符回退，不猜测 US 字符。RawInput 与可选 mouse-to-pointer 转换已接入，原始设备、消费及 NOLEGACY 契约见[输入 profile](input-profile.md)。IME composition、dead-key 解析及宿主键盘布局解析尚未接入。

硬件事件更新异步状态；只有 GetMessage 或 PM_REMOVE 成功取出才更新当前线程的队列状态。PM_NOREMOVE、输出故障和 PostMessage 不模拟硬件消费。窗口销毁清理其缓存文本，线程退出清理线程状态。GetKeyState/GetAsyncKeyState/GetKeyboardState 已接入客体服务，虚拟布局查询见[键盘契约](keyboard.md)。

底层使用公共 `wire_input` 暂存区。`wire_windows` 返回字节长度，失败为 `u32::MAX`；每个记录为 24 字节头（u64 handle、u32 width、u32 height、u32 flags、u32 UTF-8 字节数）后接标题，flags 的位 0/1 为 visible/topmost。`wire_queue_window_events` 消费整个暂存区，每项 48 字节：u64 HWND、u32 message、u32 flags、u64 wParam、u64 lParam、u32 time、i32 x、i32 y、u32 reserved；flags 位 0 表示显式 point，其余位及 reserved 必须为零。成功返回零，失败返回一并保留队列。Web 层返回持有副本的窗口记录，不暴露暂存区生命周期。

`wire_window_origin(owner, hwnd)` 使用同一暂存区返回八字节的 `[i32 x, i32 y]`；零表示没有窗口，`u32::MAX` 表示执行活动冲突。它与像素/窗口枚举一样要求在下一次暂存区操作前复制结果。

## 浏览器展示

`web/presenter.mjs` 的 `BrowserPresenter(machine, container)` 借用 VM 和一个 DOM 容器，配套样式位于 `web/presenter.css`。`sync()` 在暂停边界同步所有进程的顶层窗口、标题、可见性、topmost 和软件 RGBA8 画面；`run(budget)` 先提交积累的输入，再执行同一 VM 的预算并同步输出，预算和退出原因沿用运行时。调用者负责后续执行/等待和 VM 的关闭。

AMD64 图形配置可使用 `{getCanvas: presenter.getCanvas, shaderCompiler}`。该回调只访问宿主记录，按 PID/HWND 返回独立 WebGPU canvas；同一窗口首次使用 GPU 后保留图形画面到窗口退休，当前没有 GDI 与 DXGI 画面的合成。软件 2D 与 GPU 使用各自的 canvas，避免浏览器 context 类型冲突。`close()` 移除展示窗口及未提交输入，不关闭借用的 VM、GPU owner 或 shader 编译器。

DOM 键盘输入携带物理 VK/set-1 扫描码、重复/扩展/上下文/释放位，并以实际 `KeyboardEvent.key` 提供配对 UTF-16 文本。鼠标坐标按 canvas 显示比例换算为 client 像素，滚轮按已查询的 client 原点生成 screen 坐标并保留小数增量；pointer capture、失焦时释放按键/按钮、激活和 WM_CLOSE 使用相同输入队列。事件只积累宿主数据，不在 DOM 回调中执行客体或重入核心。每进程最多积累 4096 项，邻接鼠标移动合并；键盘与其文本不跨 256 项提交边界拆分，提交失败保留尚未接受的输入。IME、dead key、完整系统键盘布局和原生桌面 compositor 仍不在当前展示层范围。
