# 消息队列与分发

新 Wasm 入口在 PE32/Win64 接入 `win32/catalog/messages.rs` 的 22 项消息契约，包含 Post/Get/Peek/Dispatch/Translate、消息 metadata、WaitMessage、MsgWaitForMultipleObjects/Ex、GetQueueStatus 和 SetTimer/KillTimer。旧目录中 Win64 的六项 absent 服务只在新运行时通过 user32.dll 扩展资格；共享旧运行时的执行策略保持不变。

队列保存完整 HWND、wParam/lParam、DWORD 消息与毫秒时间，以及产生时的 screen POINT。PE32 MSG 为 28 字节，Win64 为 48 字节。Get/Peek 完成整个输出后才发布时间/位置 metadata、清除 extra、移除消息和消费硬件键盘状态；跨页或 guard 故障保留消息和先前 metadata，修复后可重试。API 参数、返回地址和清栈量在输出前捕获，支持输出与调用帧别名。

每线程提取自己的消息。HWND 为零表示全部，指针宽度的 -1 仅取线程消息，其余为已存在的窗口；WM_QUIT 绕过窗口与范围过滤，GetMessage 返回零，普通消息返回一。无效 HWND 返回错误 1400，倒置范围返回 87；GetMessage 的错误结果为 DWORD -1。PeekMessage 接受 PM_REMOVE 和 PM_NOYIELD，其余标志返回 50。A/W 提取使用相同布局，TranslateMessage 按实际窗口编码产生 UTF-16 或 CP932 字节消息；跨 A/W 字符消息转换尚未接入。

GetMessage 无匹配消息时先检查输出，再保存原调用并让出线程。WaitMessage 只接受该线程未检查过的新输入；Get/Peek 和成功的 WaitMessage 更新已检查序号，PM_NOREMOVE 留下的旧消息不会反复唤醒。阻塞重试保留最初参数和返回帧，未完成时不增加 work；其他线程投递、计时器到期或宿主批次使调度器重新检查。没有活动消息计时器或可见脏窗口时，Get/Peek/Wait 不采样时钟。

GetQueueStatus 的高 WORD 是当前排队类别，低 WORD 是自上次检查后新增的类别；查询仅确认所请求类别，不吞掉其他类别的新输入。接受的 flags 为 0x1dff，未知位返回 87。键盘消息属于 QS_KEY，WM_MOUSEMOVE 属于 QS_MOUSEMOVE，其余已接入鼠标消息属于 QS_MOUSEBUTTON，WM_TIMER 属于 QS_TIMER，WM_PAINT 属于 QS_PAINT，WM_INPUT 属于 QS_RAWINPUT，其余消息属于 QS_POSTMESSAGE/QS_ALLPOSTMESSAGE。未接入的 sent、hotkey、touch/pointer 来源不生成相应类别。Get/Peek 成功或确认空队列后检查全部类别，输出故障不检查；WaitMessage 成功时检查全部类别。

MsgWaitForMultipleObjects/Ex 接受 0..63 个具有 SYNCHRONIZE 权限的线程或同步对象，与消息条件共用[对象等待](waits.md)的目标引用、首次 deadline、原子获取和退出清理。wait-any 先按低索引检查对象，消息结果为 WAIT_OBJECT_0 加对象数量；wait-all 同时满足全部对象和所请求的消息条件后才获取资源，返回 WAIT_OBJECT_0，abandoned mutex 沿用对象等待结果。Ex 接受 MWMO_WAITALL、MWMO_INPUTAVAILABLE 和 MWMO_ALERTABLE；INPUTAVAILABLE 也接受已检查的排队消息，否则只接受新输入。未知 flags/mask 返回 WAIT_FAILED/87；ALERTABLE 在参数与目标验证后优先投递用户 APC，完成返回 WAIT_IO_COMPLETION。完成或超时后只确认所请求的消息类别，阻塞时保留条件。零 timeout 可以轮询，INFINITE 不建立 deadline；普通事件/线程且所请求类别没有消息计时器或 paint 来源时无需时间样本。

PostThreadMessage 接受任何存活线程，退出后错误 1444。Post 的广播 HWND 返回 50，无效窗口返回 1400。创建、字符串、MINMAXINFO、WINDOWPOS、COPYDATA 和 NCCALCSIZE 等已列出的同步专用消息拒绝异步投递并返回 1159。每进程最多 4096 条排队消息，guest 投递满时返回 1816；宿主批次在改变队列前检查容量。窗口销毁移除其排队消息、计时器及缓存文本，线程退出另清理线程消息、计时器和 metadata。消息身份序号单调增加，不随移除复用。

SetTimer 接受当前线程的窗口，错误 HWND 返回 1400，其他线程的窗口返回 5；NULL HWND 创建线程计时器。窗口的非零 ID 保留完整指针宽度；零 ID 或未注册的线程 ID 分配非零单调身份，跳过已有窗口 ID，已有线程 ID 可替换。周期限制为 10..0x7fffffff 毫秒，首次 deadline 溢出返回 87。每进程最多 1024 项，容量/身份耗尽返回 8，失败不消耗身份，已有项仍可替换。KillTimer 只取消后续生成，保留已经排队的 WM_TIMER；缺失项返回零。

计时器在 Get/Peek/Wait、请求 QS_TIMER 的 queue status/混合等待中按需刷新。每个 owner/HWND/ID 最多保留一条 WM_TIMER，跳过错过的周期并保留已排队消息的原 payload；队列满时保留待到期状态供后续刷新。最后一个可表示 deadline 到期后停止后续生成。普通 JIT 边不检查计时器，全部线程等待时返回 Waiting，由宿主再次运行推进时间。

DispatchMessage 读取完整 MSG；普通线程消息返回零，窗口消息通过同一 JIT 的 WNDPROC 续点执行。WM_TIMER 的非零 lParam 调用 TIMERPROC，参数为 HWND、WM_TIMER、完整 ID 和分发时的新 DWORD tick，回调为 void，DispatchMessage 返回零。两者保存 DispatchMessage 自身的一参数返回帧，PE32 正确清理四字节参数；重入、异常、让出、装载和退出共用窗口回调边界。每次完成或接受的 API 只增加一次 work，过程指令使用正常预算。

Post/PostThread/Quit、SetTimer、key-down 的 TranslateMessage 和 TIMERPROC 分发使用同一 10 MHz 单调源，除以 10000 得到 DWORD 毫秒。活动消息计时器或可见脏窗口使请求相应类别的查询/等待读取时间，用于生成 MSG 的时间快照；取消计时器无需样本，不请求 QS_TIMER/QS_PAINT 的 queue status 不读取这些来源。GetMessageTime/GetMessagePos 返回当前线程最近成功提取或窥视的快照。SetMessageExtraInfo 交换线程值；下次成功提取消息时 extra 归零，宿主入口当前不提供额外数据字段。

可见脏窗口在 Get/Peek/Wait 或请求 QS_PAINT 的 queue status/混合等待中合成 WM_PAINT。每窗口最多保留一条合成项；已有显式 WM_PAINT 时不重复合成。区域验证、BeginPaint 消耗和销毁会撤销合成项，显式投递项仍保留。被取走但尚未验证的区域可在后续查询重新生成；容量不足时区域继续保留。隐藏窗口的旧合成项在刷新时清理，显示、resize 和重入失效区域见[绘制契约](painting.md)。仅请求 QS_KEY 的混合等待不会生成或接受 paint。

宿主事件与 Unicode 文本配对见[输入边界](input-events.md)。同步跨线程发送、touch/pointer 与完整输入所有权仍待接入。

硬件 WM_INPUT 与普通宿主消息共用队列、QS_RAWINPUT 和等待唤醒。GetMessage/PM_REMOVE 仅在完整 MSG 输出成功后标记 raw 包已移出队列；缓冲读取及默认过程的清理规则见[RawInput](input-profile.md)。
