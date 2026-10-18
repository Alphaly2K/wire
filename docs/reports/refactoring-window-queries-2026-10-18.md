# R2 窗口查询、字段与坐标基线（2026-10-18）

状态：六项独立旧行为回归已通过；运行时草稿尚未接入。前一批 USER 名称/属性候选因 Win64 性能门槛拒绝，先解决该问题，不将本批计入已验收累计数。

拟覆盖二十五项：IsWindow/IsWindowVisible/IsWindowUnicode、GetParent、GetWindowThreadProcessId、GetClientRect/GetWindowRect、Get/SetWindowLongA/W、Get/SetWindowLongPtrA/W、FindWindowA/W、GetDesktopWindow、GetWindowPlacement、IsIconic/IsZoomed/IsWindowEnabled、ClientToScreen/ScreenToClient/MapWindowPoints。PE32 四项 Ptr 保留未知 ABI 与零清栈，placement 保留明确 error120 与旧 trace；Win64 六项原缺失查询继续 absent，不添加功能。

基线日志 `wire-out/refactoring-2026-10-14/r2-window-queries-baseline-tests-v2.txt`：PE32 四项、Win64 两项。覆盖 provider 与动态 lookup、无效 HWND 的 LastError 差异、DWORD/指针和 4/8 字节 extra 存储、WNDPROC 验证顺序、完整 instance/用户数据、PID 差异、坐标环及错误映射、桌面/顶层过滤。PE32 RECT 逐 DWORD 故障保留前缀，Win64 整块检查失败保持输出；Win64 使用真正执行 WNDPROC 回调创建的逻辑窗口。

首轮五项通过，Win64 输出故障测试选择的栈边界仍处于 Process64 的较大映射中，未制造出预期故障；测试改用独立单页的末尾，运行时与断言语义保持，六项通过。原失败日志 `r2-window-queries-baseline-tests.txt` 保留。
