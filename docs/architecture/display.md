# 虚拟桌面、显示查询与 monitor callback

`win32/catalog/display.rs` 是 16 项显示 API 的 ABI、provider、动态查询范围和 typed ID 的唯一声明。绑定时解析名称，标量调用使用 `DisplayApi`；Win64 `EnumDisplayMonitors` 使用 `ControlImport::MonitorEnumeration`，PE32 使用具名 callback facade。

`win32/user/display.rs` 提供不可变 1024×768、32-bit、60-Hz desktop profile，以及 DISPLAY_DEVICE、DEVMODE、MONITORINFO 的显式 little-endian 编码和 metrics 纯核。它不访问 guest 内存或执行 CPU。`win32/x86/display.rs` 和 `x64/runtime/display.rs` 保留架构验证与输出提交；monitor callback 的临时 heap、guest executor、frame 和释放顺序由原进程路径管理。

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

独立旧行为、callback、完整正确性与性能证据见[显示批次报告](../reports/refactoring-display-2026-10-18.md)。
