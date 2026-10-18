# R2 窗口查询、字段与坐标基线（2026-10-18）

状态：查询路由已接入，六项独立旧行为回归及完整正确性检查通过；与前批名称/属性、trace 修订一起进行集成性能验收，尚未计入已验收累计数。

拟覆盖二十五项：IsWindow/IsWindowVisible/IsWindowUnicode、GetParent、GetWindowThreadProcessId、GetClientRect/GetWindowRect、Get/SetWindowLongA/W、Get/SetWindowLongPtrA/W、FindWindowA/W、GetDesktopWindow、GetWindowPlacement、IsIconic/IsZoomed/IsWindowEnabled、ClientToScreen/ScreenToClient/MapWindowPoints。PE32 四项 Ptr 保留未知 ABI 与零清栈，placement 保留明确 error120 与旧 trace；Win64 六项原缺失查询继续 absent，不添加功能。

基线日志 `wire-out/refactoring-2026-10-14/r2-window-queries-baseline-tests-v2.txt`：PE32 四项、Win64 两项。覆盖 provider 与动态 lookup、无效 HWND 的 LastError 差异、DWORD/指针和 4/8 字节 extra 存储、WNDPROC 验证顺序、完整 instance/用户数据、PID 差异、坐标环及错误映射、桌面/顶层过滤。PE32 RECT 逐 DWORD 故障保留前缀，Win64 整块检查失败保持输出；Win64 使用真正执行 WNDPROC 回调创建的逻辑窗口。

首轮五项通过，Win64 输出故障测试选择的栈边界仍处于 Process64 的较大映射中，未制造出预期故障；测试改用独立单页的末尾，运行时与断言语义保持，六项通过。原失败日志 `r2-window-queries-baseline-tests.txt` 保留。

本批契约和双架构 scalar handler 已接入，移除旧 ABI/arity/dwords/provider 与字符串查询分支。PE32 handler 收窄为 User32、受检内存和 LastError，首次桌面创建仍由 module/class coordinator 完成。Win64 callback、焦点和文本路径保留；[边界说明](../architecture/window-queries.md)记录架构差异与故障输出顺序。接入发生在前批最终 source verification 之后，未混入前批冻结 executable。

接入候选的 executable 增长预算为 +32KiB，在构建与测量前固定；比较基线将在前批属性/trace 验收完成后确定。窗口查询 fixture 将先在旧冻结程序上独立核对实际返回、退休数、API 次数与 Win64 work，再用于新旧比较。

接入后六项独立回归通过，完整 release 594 passed / 7 ignored，debug 440、显式 native 7、工具 23；fmt、strict all-target/all-feature Clippy、release build 全部通过。日志前缀 `r2-window-queries-validation-*`；本批 Rust 首次编译与上述检查均通过，无需恢复旧查询分发。375 个 Rust 文件（src/tests 367 个）的 live/frozen/manifest SHA 与文件集合一致，target/frozen executable SHA 一致，见 `r2-window-queries-validation-source-verification.json`。

集成冻结 `r2-user-queries-integrated-wire/wire`：SHA256 `1b32033648a5f513318697248e41fffd22776307b7008af473e9c51f9e78734c`，5,970,664 bytes，相对 Classes +5,392 bytes，相对属性/trace 最终草稿 +1,584 bytes，满足预先固定的各项体积预算。两个家族保留各自独立行为基线；共同 before 为已验收 Classes，集成状态将在新的固定路径协议下比较，不把已拒绝候选作为性能准入基线。

新增 `gateway-window` 已在旧 Classes 冻结程序上独立验证 Win64 与 PE32 IR/raw 三条路径。Win64 真正执行 WNDPROC 并创建/销毁窗口，PE32 预设桌面字段后循环读取；返回值、退休数、API 次数及 Win64 work 均通过。集成候选的 51 条路径、三对轮换矩阵正在执行，之后再做游戏前缀验收。
