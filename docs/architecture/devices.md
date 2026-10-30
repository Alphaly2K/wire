# Win64 设备发现与通知

Wasm 入口在 AMD64 接入 `DEVICE_CONTRACTS` 的全部 24 项：HID 14 项仅由 hid.dll 提供，SetupAPI 7 项仅由 setupapi.dll 提供，窗口设备通知 3 项仅由 user32.dll 提供；静态和动态范围一致。PE32 尚未接入该组，绑定时拒绝。

`win32/device_state.rs` 在新旧入口间共享设备集合、通知订阅、单调句柄及全部处理函数。适配器只提供受检内存、当前线程错误槽和窗口存在性。沿用原高位句柄范围、4096 项容量与累计耗尽规则；新 USER owner 在窗口销毁、线程退出和进程清理时移除相应订阅。

设备数据库仍为空，不生成物理设备、接口路径或 HID report。保留筛选 flags、字符串、GUID、结构大小、句柄与输出指针检查顺序，以及 HIDP status、BOOL/LastError 和 VOID 的差异。通知订阅是真实拥有的对象，销毁窗口时移除；当前没有生成设备到达事件的后端。
