# 新增 Win32 API：GUID 试点

已迁移家族及各架构策略见[当前状态](../status.md)，本文保留 GUID 试点说明并补充后续控制路由规则。普通 scalar API 的操作步骤如下；尚未迁移的家族继续走明确的 legacy 路由，ABI 已知与实现能力分别记录。

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

PE32 `ApiTable.entries` 公开可修改，为兼容旧调用方，内部 immutable binding 在条目变化时重建；未变化调用只读取已绑定描述符，包括 Typed 与 LegacyUnimplemented 执行策略。ABI-only 调用保留原参数读取、零返回/LastError=120、诊断与返回 frame 提交；变更同一 stub 的执行策略也必须重新绑定。stub 地址、ordinal 别名和 CRT data export 不变。Win64 `Gateways::insert` 同时更新原始导出视图和冷绑定计划。

Win64 的 raw 字符串 codec 已提取到 `win32/x64/text.rs`，旧 Runtime64 静态 helper 委托它。GUID 适配器只借用架构内存，完全不依赖总运行状态。UTF-16 codec 和共享 parser 分开，以保持 fault 与 HRESULT 策略。

identity 展示了 ABI 与执行策略分离：相同签名在 PE32 上保留 `LegacyUnimplemented`，在 Win64 上拥有 typed route。PE32 历史 kernel32 GetUserName ABI 登记也保留，不能用 Win64 的 DLL 范围覆盖它。每个新增家族需要先核对这样的差异，再删除旧规则；catalog 的唯一性检查防止 duplicate export/owner 与参数索引越界。

version 增加 `Absent`，表示该架构没有原有绑定；空 DLL 集合与这一策略必须对应。64 位 scalar 在 PE32 使用两个 DWORD slot，在 Win64 使用一个参数位置。BYTE 的 Win64 原始 trace carrier 保留，由适配器消费低位。详细兼容行为见 [version](version.md)。

callback API 使用专用架构控制路由。NLS 的 `EnumSystemLocalesEx` 在冷绑定时选择 ControlImport，由原进程框架完成回调 frame 与资源生命周期；普通 scalar handler 的参数和返回类型没有被扩成万能 Context。参见 [NLS](nls.md)。

模块家族把静态导入 DLL 与动态查询 DLL 分开声明：默认一致，有历史差异时显式设置 `lookup_dlls`，且 lookup provider 必须属于 import provider 集合。Win64 fresh-provider 的 PSAPI 查询可见性与已有 gateway registry fallback 分别保持，新增声明不能替换后者。K32 与无前缀名称冷绑定为相同行为 ID。`LoadLibrary`、`FreeLibrary`、`GetProcAddress` 使用模块控制路由，adapter 返回 Value、Returned 或 Load 请求，进程框架保存 frame、API count、trace、unblock 和加载提交顺序。新 API 若产生控制请求，应先固定这些顺序，再选择其专用 outcome；参见 [模块契约](modules.md)。

文件家族先收敛契约与 bound ID，adapter 继续留在原调用边界。`SetFilePointerEx` 的 Int64 参数在 PE32 占两个 DWORD slot，Win64 占一个 carrier；输出检查与定位提交先后按架构保留。普通文件、pipe、标准 endpoint 的完成与 unblock 次序各自保持。扩展契约无需同时抽取结果框架或移动状态 owner；这些工作分别验收。CWD 容量查询、小 buffer、file-part 输出 fault 与 return-slot alias 都有独立行为基线；详见 [文件契约](files.md)。

## 同步与调度控制

阻塞 API 冷绑定为专用 `ControlImport`，普通 `catalog_call -> u64` 不接管它们。adapter 用领域 outcome 表示完成、Try 失败、阻塞或唤醒请求；进程处理 CPU、scheduler、trace 和计数。特别保留 Try 失败是否解除既有 blocked 状态、阻塞是否消费返回帧，以及最后一次递归释放才唤醒等待者等区别。真实 guest 线程与小 work 切片应覆盖重试路径；参见 [SRW/InitOnce](inline-sync.md)和[critical section](critical-sections.md)。

状态 owner 按实际资源形成：SRW 的 map 私有封装，成功写 guest token 后才提交；地址等待的序号、队列和注册属于线程 owner。具有实际内存布局差异的 PE32/Win64 codec 分别保留，不用全结构预检替换既有逐字段写入。owner 只借所需内存和错误槽，避免再次借用整个 Runtime64/ApiState；CPU 执行仍留在进程边界。

PE32 的 `Absent` 与 `LegacyUnimplemented` 对清栈有实际区别。WaitOnAddress 家族原本 ABI 未知，因此即使名字进入 catalog，X86 plan 仍为空 DLL 集合，保留 legacy unknown ABI 的零参数清栈；不得从 Win64 参数表自动补出 PE32 ABI。已有 Wine ABI 的 SRW/InitOnce 则保持 ABI-only。分别固定旧行为再迁移，见[地址等待](address-waits.md)。
