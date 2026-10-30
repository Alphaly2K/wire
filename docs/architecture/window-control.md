# 窗口生命周期与过程控制

新 Wasm 入口在 PE32/Win64 支持 CreateWindowExA/W、DestroyWindow、DefWindowProcA/W、CallWindowProcA/W、ShowWindow、MoveWindow/SetWindowPos、UpdateWindow 和同线程 SendMessageA/W、SendMessageTimeoutA/W，以及 EnableWindow、SetParent、DefDlgProcA/W。导出和 ABI 来自 `win32/catalog/window_control.rs`，各架构声明的入口均已绑定。窗口过程在同一 Wasm JIT、代码缓存、VMM 和预算中执行，使用每线程回调续点，不嵌套宿主执行循环。

窗口记录复用便携的 `win32/user/windows64.rs`，独立于旧执行器；过程和所有 HWND、instance、wParam/lParam 使用客体指针宽度。每进程最多 1024 个活动窗口和 4096 个单调窗口身份，注销/销毁不复用 HWND。两种 ABI 支持 child、owner 和 message-only parent，验证 parent 存在，其他线程的非桌面 parent 返回 50；child 的 hMenu 按 control ID 保存，PE32 顶层非零菜单必须有效，否则返回 1401；AMD64 顶层非零菜单返回 50。跨线程发送和跨 A/W 文本消息转换仍返回 50。类菜单名尚不自动实例化菜单，挂接/销毁与客户区更新见[菜单契约](menus.md)。两种 ABI 按类注册编码构造 CREATESTRUCT。跨 A/W 调用的标题经 CP932/UTF-16 转换后存放于本次回调栈区；PE32 使用类的持久名称指针，AMD64 将转换后的类名保存在同一回调栈区，容量按所需字符串检查。

创建先捕获完整 API 调用，再检查回调 frame、记录输出、类、过程和参数，成功后才发布窗口。CW_USEDEFAULT 使用 100/100/800/600，尺寸限于 1..8192，拒绝坐标溢出。CREATESTRUCT 位于本次调用的客体栈存储中，其生命周期覆盖创建回调；该存储和回调 frame 相互分离。Win64 依次调用 WM_GETMINMAXINFO、WM_NCCREATE、WM_NCCALCSIZE、WM_CREATE，初始 MINMAXINFO 使用 1280×720 的逻辑显示尺寸；PE32 保留 WM_NCCREATE、WM_CREATE 顺序，client 几何使用共享非客户区算法。

每次生命周期通知重新读取当前窗口过程，允许在创建中 subclass。WM_NCCREATE 返回零后发送 WM_NCDESTROY；WM_CREATE 返回 -1 后发送 WM_DESTROY、WM_NCDESTROY，再删除窗口。回调中自行销毁窗口使原创建返回零。下一 frame 或 NCCALCSIZE 读取失败保留已完成回调结果，修复后不重放；最终普通返回无需重新写入已无用途的 frame。

DestroyWindow 只允许 owner 线程，发送 WM_DESTROY/WM_NCDESTROY 后删除窗口，递归销毁同一对象返回零。ShowWindow 先提交可见位，再发送 WM_SHOWWINDOW；PE32 可见性未改变时不通知，Win64 还发送 WM_SIZE 并仅接受 0/1/4/5/8。返回先前可见性。CallWindowProc 可直接转发四个窗口参数；SendMessage 重新进入当前过程，保留完整 LRESULT。

SendMessageTimeoutA/W 复用同一 WNDPROC 调用链，只支持同线程窗口。同队列调用忽略 uTimeout，不采样时钟；接受 SMTO_BLOCK、ABORTIFHUNG、NOTIMEOUTIFNOTHUNG、ERRORONEXIT 的组合（掩码 0x2b），其他 flags 返回零/87。成功状态为非零，即使窗口过程的 LRESULT 为零；可选 lpdwResult 接收完整 DWORD_PTR。输出在进入回调前预检，在回调完成后发布；过程可让出、嵌套或装载 DLL，输出故障后保留冻结结果并允许重试，不重放过程。输出与原返回槽重叠也使用已保存的返回位置。

ERRORONEXIT 在窗口于处理期间销毁后返回零和错误 1400，保持结果输出；错误槽故障可原位修复。没有该标志时仍返回发送成功并发布过程结果。非法 HWND 返回 1400，跨线程、HWND_BROADCAST 和跨 A/W 文本消息返回 50，资格失败不访问结果指针。跨线程等待/超时、hung-thread 检测、广播及字符串 marshalling 仍缺失。[公开接口契约](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessagetimeoutw)。

两种 ABI 子窗口的 rect/client 使用父客户区坐标，查询及宿主鼠标输入通过共享父链计算屏幕原点；移动父窗口后子窗口坐标立即反映新位置。SetParent 检查无效父级和循环，保留样式；child 改挂或分离时保持屏幕位置，不重复叠加边框偏移，不能表示的本地矩形返回 87。EnableWindow 返回先前 disabled 状态，仅在启用状态变化时发送 WM_ENABLE；首帧预检后提交样式，过程内故障保持已接受的改变。

默认过程当前接入 NCCREATE、NCCALCSIZE、WM_WINDOWPOSCHANGED 的 MOVE/SIZE 通知、WM_SETTEXT/GETTEXT/GETTEXTLENGTH 和 WM_CLOSE 的销毁转移；其余消息保留标量零返回。两种 ABI 的 DefDlgProcA/W 复用同一基本默认子集，模态 dialog manager 尚未接通。文本使用同一有界 CP932/UTF-16 编码与完整输出预检，容量为零不访问输出。默认 WM_CLOSE 完成销毁后返回零。UpdateWindow、默认 WM_PAINT 与 WM_ERASEBKGND 使用[绘制续点和实际 GDI 像素](painting.md)；其余系统消息仍待迁移。

MoveWindow/SetWindowPos 在两种 ABI 使用标准 WINDOWPOS 和 NCCALCSIZE_PARAMS 布局，暂存记录位于本次调用的客体栈上，保留至全部通知结束。流程为 WM_WINDOWPOSCHANGING、需要时 WM_NCCALCSIZE、提交几何与可见/topmost 状态、WM_WINDOWPOSCHANGED；默认后者再通知 WM_MOVE/WM_SIZE。每阶段重读过程和 guest 修改的记录；NOSENDCHANGING 跳过首通知，NOMOVE/NOSIZE 使用现有几何，normalized WINDOWPOS 保存实际值。CHANGING/NCCALCSIZE 后的记录或 frame 故障不重放完成回调。

接受 0x7ff 内的同步 flags；尺寸为 0..8192，拒绝坐标加法溢出。NOZORDER 允许忽略 insert-after；否则当前只支持 HWND_TOPMOST/HWND_NOTOPMOST，其他相对 z-order 和异步请求返回 50。错误 HWND 返回 1400，其他线程窗口返回 50。NCCALCSIZE 必须给出有效且有界的 client RECT；几何提交前预检下一 frame，已提交的状态不因后续过程异常而回滚。已有 backing 的尺寸变更先分配并保留重叠像素，成功后提交几何；失败保留原状态。没有 NOREDRAW 时失效整个 client。完整重绘策略、激活通知和相对 z-order 仍待迁移。

每个接受的 API 计一次 work，WNDPROC 指令及其服务使用同一累计预算，各阶段准备/返回不额外计费。窗口过程可抛出异常、让出、等待或装载 DLL；装载返回仍按发起深度识别。目标展开可跨 WNDPROC 续点，放弃创建或销毁操作时回收该窗口；线程退出删除其窗口对象，静态启动拒绝清空 USER 状态。父窗口最终退休时同时清理 WS_CHILD 后代及其排队消息、输入引用、属性、DC 与像素，child ID 不销毁数值相同的菜单。完整后代销毁通知、线程退出通知和 owner 弹窗销毁联动仍待迁移。DispatchMessage 共用这条 WNDPROC 边界并保留自身 ABI，见[消息队列](messages.md)。线程退出另释放该线程的兼容 DC；宿主窗口展示与图形提交见[虚拟桌面](display.md)和对应图形契约。
