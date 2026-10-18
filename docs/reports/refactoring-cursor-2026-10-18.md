# R2/R3 游标与指针能力（2026-10-18）

状态：六项旧行为独立基线通过，尚未迁移运行时。本批不计入已验收 437 exports。

10 项候选：ShowCursor、GetCursor、SetCursor、GetCursorPos、SetCursorPos、ClipCursor、GetClipCursor、EnableMouseInPointer、IsMouseInPointerEnabled、TrackMouseEvent。PE32 前五项 typed，其余五项保持 ABI-only/error 120；Win64 除 SetCursorPos absent 外九项 typed。TrackMouseEvent 静态仅 user32，其余 Win64 typed 静态 kernel32/kernelbase/user32；动态均仅 user32。

六项独立基线固定真实 stock arrow、无效句柄、signed position、BOOL 高位截断、返回槽覆盖顺序、虚拟裁剪及 invalid/NULL 裁剪行为、双架构跨页输出故障，已在未迁移运行时 `fd3da58` 上通过。Win64 既有 tracking 单测继续覆盖空查询/取消、无后端注册、完整 HWND、结构大小和输出故障。两端显示计数均按线程维护，保留 saturating signed count 和返回载体；PE32 线程退出清理，Win64 现有退出行为保持。首四项和扩充后六项日志分别保留为 `r2-cursor-baseline{32,64}{,-extended}-tests.txt`。

原创可选 `gateway-cursor` 循环 ShowCursor(1)，只增长计数值，不逐轮分配资源，checksum n(n+1)/2。旧 geometry 冻结双架构 12K 实跑通过，Win64 退休 5n+10/work 6n+10，PE32 退休 5n+7；API 均为 n。PE32 raw/IR 均校验，工具 20 项通过；默认 case 集合保持。

before 为 `r2-geometry-wire/wire`，SHA256 `37574f910b7f5096d0799950e457a1743f047dd096e32c0a11bf295940ea769a`，5,949,080 bytes。本批完整文件体积预算 +24KiB。证据根 `wire-out/refactoring-2026-10-14/`，旧行为日志 `r2-cursor-baseline{32,64}-tests.txt`，fixture `r2-cursor-fixture-baseline{32,64}`。
