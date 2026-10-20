# 虚拟桌面、显示查询与 monitor callback

`win32/catalog/display.rs` 是 16 项显示 API 的 ABI、provider、动态查询范围和 typed ID 的唯一声明。绑定时解析名称，标量调用使用 `DisplayApi`；Win64 `EnumDisplayMonitors` 使用 `ControlImport::MonitorEnumeration`，PE32 使用 Process 的具名 callback 协调器。

`win32/user/display.rs` 提供不可变 1024×768、32-bit、60-Hz desktop profile，以及 DISPLAY_DEVICE、DEVMODE、MONITORINFO 的显式 little-endian 编码和 metrics 纯核。它不访问 guest 内存或执行 CPU。`win32/x86/display.rs` 和 `x64/runtime/display.rs` 保留架构验证与输出提交；monitor callback 的临时 heap、guest executor、frame 和释放顺序由原进程路径管理。

Win64 标量查询由 User 执行，只借 Memory 和只读 Threads；Runtime64 只组合资源。HWND 检查、DEVMODE/设备输出、flags 与错误槽访问保持原顺序，EnumDisplayMonitors 的 callback coordinator 保持独立。

| 行为 | PE32 | Win64 |
| --- | --- | --- |
| 静态 provider | user32 | kernel32/kernelbase/user32 |
| 动态查询 | user32 | user32；metrics 默认查询缺口保留，已有 gateway fallback 保留 |
| MonitorFromPoint | POINT 占两个 stack words，共 12-byte stdcall 清栈 | POINT 打包在一个 64-bit carrier，flags 截断至 DWORD |
| MonitorFrom* | 原宽松单 monitor 返回，不读 RECT/HWND | 验证 flags、intersection、完整 HWND |
| display devices | 仅 adapter | adapter 和 monitor child |
| display enumeration copy fault | error 87、零返回，正常提交调用 | checked fault，保留 CPU/API 计数与原输出前缀 |
| 三项 CCD API | ABI-only、error 120、零返回 | direct status 50，不读输出指针、不改 LastError |

`EnumDisplayMonitors` 保持 clipping、callback 返回布尔值、lParam 宽度、资源释放和暂停/退出等控制结果。纯编码共享不改变两种内存实现的受检读写顺序。原 PE32 `display.rs`、重复 ABI/arity/DWORD/provider 表与字符串分发已移除。

PE32 `win32/x86/display.rs::prepare_monitors` 仅借内存、heap、process heap 与错误槽，准备 scalar 结果或带临时 block 的 callback 请求。`x86/process/display.rs` 执行 callback，并在取得 Result 后按原顺序释放 block，再传播正常、fault、transfer 或 exit；没有用提前返回跳过释放。基线 `683f025`，迁移前独立 fault fixture 验证同一 block 可重用，原准备主体和标量 adapter 核对保持；证据 `wire-out/refactoring-2026-10-14/r4-sent-monitor32-{source-checks,verification}.json`。

独立旧行为、callback、完整正确性与性能证据见[显示批次报告](../reports/refactoring-display-2026-10-18.md)。
