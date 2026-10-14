# 新增 Win32 API：GUID 试点

`StringFromGUID2`、`CLSIDFromString`、`IIDFromString` 已迁入新边界。普通 scalar API 的操作步骤如下；其它家族目前仍使用旧分发器，不能将 ABI 已知当作完整实现。

1. 在 `src/win32/catalog/` 的所属领域声明 export、DLL/架构范围、typed ID、参数类型、返回约定和限制。GUID 的一份声明派生 PE32 stdcall 清栈和 Win64 DWORD 截断，执行时不查询字符串表。
2. 把不依赖 guest 内存和进程状态的算法放入领域纯核。GUID 的 Windows little-endian 布局、canonical parser/formatter 位于 `win32/com/guid.rs`；纯核不读 CPU，不写 LastError。
3. 在 `win32/x86` / `win32/x64` 实现对应内存访问与错误映射。返回 frame 仍由原 `api` / `x64::runtime` facade 提交。varargs、FLOAT/XMM、callback、sret 等需要专用适配器，不能塞入当前 scalar 签名模型。
4. 冷绑定选择 typed ID；动态 lookup 复用同一 resolver。删除已迁移 export 在旧支持、arity、DWORD、route 表中的手写项。保留 legacy 家族和现有 HotImport。
5. 先固定旧行为的独立 fixture，再切换路由。验证错 DLL、参数高位/符号、输出故障、guard、返回槽别名和重绑定。期望值不从契约自动生成。影响 gateway 时冻结 executable，串行执行至少三对 AB/BA。

## GUID 的架构边界

| 行为 | PE32 | Win64 |
| --- | --- | --- |
| DLL | ole32 | ole32/combase |
| parser 输入 | 现有 bounded `read_wstr(...,39)` | raw UTF-16，保留终止/32768 unit 上限及 fault |
| parser null input | 写零 GUID，S_OK | 写零 GUID，S_OK |
| parser output fault | HRESULT `0x80004003` 并更新 LastError | 访存错误；CPU/API count 不提交；CLSID 的 null output 为 `0x80070057` |
| 无效文本 | CLSID/IID 都 `0x800401f3` | CLSID `0x800401f3`；IID 长度错误 `0x80070057`，canonical 格式错误 `0x800401f4` |
| 未注册 ProgID | 保留原有输出 | CLSID 按原路径清零输出 |
| formatter capacity | signed 32 位，小于 39 返回 0 | 相同；Win64 高 32 位截断 |
| formatter input/output fault | input 为 HRESULT；output 返回 0 | 访存错误，CPU 不提交 |
| output alias return slot | handler 后读取返回地址 | handler 前保存返回 frame |
| wrong DLL | 原 unknown ABI/零清栈诊断 | 拒绝 import，CPU/API count 不提交 |

这些差异属于本次冻结行为，未来若要修正 Windows 符合性，应另做语义提交。`CoCreateGuid`、`StringFromCLSID/IID` 的 allocator/entropy 契约仍走 legacy，只共享格式化纯核。

PE32 `ApiTable.entries` 公开可修改，为兼容旧调用方，内部 immutable binding 在条目变化时重建；未变化调用只读取已绑定描述符。stub 地址、ordinal 别名和 CRT data export 不变。Win64 `Gateways::insert` 同时更新原始导出视图和冷绑定计划。

Win64 的 raw 字符串 codec 已提取到 `win32/x64/text.rs`，旧 Runtime64 静态 helper 委托它。GUID 适配器只借用架构内存，完全不依赖总运行状态。UTF-16 codec 和共享 parser 分开，以保持 fault 与 HRESULT 策略。
