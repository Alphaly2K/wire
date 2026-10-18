# 矩形与窗口边框边界

状态：13 项契约已接入候选，独立旧行为与迁移回归通过，完整验收见[本批报告](../reports/refactoring-geometry-2026-10-18.md)。

十项 RECT 运算与三项 AdjustWindowRect 变体集中声明。PE32 全部 user32 typed；Win64 保留原六项 copy/offset/point/frame typed，以及七项 absent。六项 Win64 静态 kernel32/kernelbase/user32、动态仅 user32 的范围单独记录。

PtInRect 的 POINT 是一个 8-byte 值：PE32 占两个连续 stack words，Win64 是一个 packed 64-bit 参数。契约使用 Uint64 表达该 carrier，因此 stdcall 清栈仍为 12 bytes，Win64 arity 为 2，两个 signed LONG 坐标均保留。DWORD/BOOL 与 signed LONG 的 Win64 高位截断继续由绑定参数计划完成。

`win32/user/geometry.rs` 只处理 `[i32; 4]` 值，包括 signed/empty、wrapping offset/inflate、交并集、96-DPI frame；不访问进程、窗口或 guest 内存。原 Hangover/Wine frame 算法和版权保留，`user32_geometry::frame` 暂作旧窗口调用者的兼容 re-export，client-origin 的窗口遍历仍归旧 owner。

`win32/x86/geometry.rs` 只借 Args、GuestMem 和 LastError，保留 PE32 trace、unsupported-DPI 的 120、先读后验证 writability 的调整顺序，以及 handler 后读取返回槽。`win32/x64/geometry.rs` 只借 Memory，集中 RECT codec；旧 USER 窗口查询继续复用 codec。Win64 错误由入口提交，unsupported-DPI 50、saved return frame 与输出故障顺序保持。两端均完整 copy-in 后 copy-out，允许输入输出重叠，跨页写入故障保持可访问前缀。

旧 PE32 手写 ABI、geometry adjust 名称分发、common rectangle 分支与函数、Win64 arity/DWORD/名称分发及重复 RECT codec 删除。未新增窗口、DPI 或 Win64 rectangle 能力。
