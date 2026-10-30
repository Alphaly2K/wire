# 窗口查询、字段与坐标

新 Wasm 入口已接通 `win32/catalog/window_queries.rs` 在各架构声明的全部查询。两种 ABI 支持窗口/可见/编码/最小化状态、父级、线程/进程归属、矩形、坐标、Long 字段、GetWindowPlacement、GetDesktopWindow、IsZoomed、IsWindowEnabled 和 MapWindowPoints。PE32 另有 FindWindowA/W；AMD64 的这两项尚未接通。PE32 LongPtr 导出保持 absent，C 头文件宏使用普通 Long。

IsWindow、IsZoomed 和 IsWindowEnabled 对无效 HWND 返回零并保留 LastError，其余窗口查询报告错误 1400；PE32 GetWindowPlacement 按旧协议直接返回零和错误 120，不读取 HWND 或输出。线程归属来自同一窗口记录；PID 为进程身份 1。RECT/POINT 输出统一完整预检，失败保留输出前缀和 CPU；坐标按当前 client 原点作 wrapping 转换，两种 ABI 的 child 通过共享父链累加各级客户区原点。GetClientRect 返回以零为原点的 client 尺寸，GetWindowRect 将 child 本地矩形转为屏幕矩形。

FindWindow 复用共享查找规则，按窗口身份顺序返回首个匹配的顶层窗口，排除 WS_CHILD 和 message-only parent。类可以为 atom 或名称，标题为可选筛选条件；CP932/UTF-16 输入按旧策略读取，名称和标题使用 Unicode lowercase 匹配。未找到返回零并保留 LastError。MapWindowPoints 最多处理 4096 个 POINT，零 HWND 表示屏幕原点；先读完整批次，按 wrapping 加法转换，再完整写入。返回值将两个偏移的低 16 位打包，不执行 RTL 镜像；零数量不访问点指针，仍校验窗口。新 VMM 的输入/输出故障保留整批数据，旧适配保留原错误协议。

两种 ABI 的 GetDesktopWindow 按旧归属规则创建一次进程级逻辑窗口和持久 Desktop 类，固定 HWND 为 `0x4000fffc`，几何来自当前 1024×768 显示配置。其线程归属为零，普通查询可访问，PE32 FindWindow 也可查找；它不出现在供宿主呈现的应用窗口快照中，其他线程不能销毁。静态启动拒绝清理同一 USER owner；AMD64 的 Desktop 类保留完整 DefWindowProcW 地址；没有实际宿主桌面。

AMD64 GetWindowPlacement 复用旧 44 字节布局：flags 为零、普通 showCmd 为 1，min/max POINT 全一，normal RECT 来自当前窗口。先检查 HWND，再检查输出非空和 length=44，再预检全部输出；随后对最小化/最大化样式返回 50。成功保持 LastError，输出故障与错误槽故障可修复后重试，不提交部分结构。normal 以外的 placement 状态仍待迁移。

Long 使用 4 字节，Win64 LongPtr 使用 8 字节，extra 存储允许有界的重叠访问。style/exstyle 使用 DWORD；WNDPROC、instance、parent、menu 和 user data 仅由 PE32 Long 或 Win64 LongPtr 访问。设置 WNDPROC 时先验证非零目标及可读/执行权限或已绑定逻辑服务，再检查 HWND；空过程返回 87。字段索引/extra 范围错误返回 1413，直接修改 parent 字段返回 50，两种 ABI 均可使用受检 SetParent。

窗口过程修改立即影响后续生命周期和同步发送通知。成功查询及修改保留 LastError；保存的 API 返回不从可能别名的输出重新读取。标题相关调用者和窗口输入所有权的迁移范围见[窗口文本与输入所有权](window-state.md)。

GetWindow 使用共享窗口记录的逆创建序查询 FIRST/LAST/NEXT/PREV、直接 CHILD 和顶层 OWNER。child siblings 限于同 parent，桌面查询排除逻辑桌面与 message-only 窗口；逻辑桌面的 CHILD 返回普通顶层窗口。非法 HWND 返回零/1400，未知命令零/87，GW_ENABLEDPOPUP 因无 enabled-popup manager 返回零/50。当前没有独立可变 z-order。双 ABI 与层次隔离由 `node crates/wire-wasm/tests/managed-runtime.mjs` 验证。
