# Win64 设备发现与通知

`DEVICE_CONTRACTS` 统一 24 项 provider、ABI、参数类型与 DeviceApi：HID 14 项仅由 hid.dll 提供，SetupAPI 7 项仅由 setupapi.dll 提供，窗口设备通知 3 项仅由 user32.dll 提供；Win64 静态和动态范围一致。PE32 的 HID/SetupAPI 保持 absent/未知 ABI，三项 user32 通知保持已有 ABI 和未实现策略。

私有 `x64::runtime::devices::Devices` 拥有设备集合、通知订阅及各自的单调句柄游标。handler 只接收 Memory、ID、参数、当前线程 LastError 地址和收件窗口是否存在；Runtime64 协调窗口查询和销毁时清理，不再持有设备业务分支。沿用原高位句柄范围、4096 项容量与累计耗尽规则。

设备数据库仍为空，不生成物理设备、接口路径或 HID report。保留筛选 flags、字符串、GUID、结构大小、句柄与输出指针检查顺序，以及 HIDP status、BOOL/LastError 和 VOID 的差异。通知订阅是真实拥有的对象，销毁窗口时移除；当前没有生成设备到达事件的后端。

基线 `e32efe3` 的 24 项 arity、DWORD 位置和 DLL provider 逐项核对一致；handler 经 typed ID、owner、窄借用替换和格式归一后与旧实现一致。使用既有 device discovery、HID absence 与通知生命周期用例验证，不增加重复测试。该批没有修改执行器或内存/cache 热路径，按低风险结构迁移处理，不进行性能计时；验证记录见 `wire-out/refactoring-2026-10-14/r2-devices-*`。

本批 release Win64 2 项、通知单元 1 项、catalog 一致性 1 项、fmt、release lib/bin strict Clippy 和 release build 全部通过，累计 **816 exports**。验证 JSON 记录 314 个 src Rust 文件与 executable 的 SHA；按用户要求在验证完成后清理 Cargo 构建缓存，保留验证记录。
