# 路径辅助与通用句柄

`win32/catalog/paths.rs` 声明 13 项路径辅助，`handles.rs` 声明六项通用句柄 API，均已接入。契约提供 export/provider、scalar 参数、返回约定、架构执行策略和冷绑定 ID。文件的 53 项契约仍由 FileApi 拥有；路径辅助与句柄作为独立家族，不复制文件声明。46 条性能路径、section/IR 的局部成本与完整验收见 [记录](../reports/refactoring-paths-handles-2026-10-15.md)。

| 家族 | PE32 | Win64 |
| --- | --- | --- |
| PathApi | SHLWAPI 的 Exists/IsDirectory/RemoveFileSpec/RenameExtension/AddExtension/UnquoteSpaces A/W；imagehlp 的 MakeSureDirectoryPathExists；13 Typed | 全部 Absent |
| HandleApi | kernel32；GetStdHandle/CloseHandle Typed，其余四项 ABI-only | kernel32/kernelbase；六项 Typed |

路径的 codec、受限 GuestFs 和 Hangover/Wine 衍生算法保留于 file_api。内部互斥的 Core/Helper ID 区分文件和路径操作，运行时不再以名称推测 A/W 或 Add/Rename。公开 file_api::dispatch 保留冷 facade，仍只接受原有小写 provider，并继续排除 GetFileType；普通 gateway 直接使用绑定 ID。

路径保留 CP932 trail-byte 处理、UTF-16 单位、根/prefix 规则、extension 的 MAX_PATH、NULL 提前返回和 checked 输出的访问顺序。成功和 NULL short-circuit 保持原 LastError；访存、路径与长度错误仍由原 handler 映射。UnquoteSpaces 的返回约定为 Void，保留原零 EAX trace carrier。原 LGPL 来源与版权头保持。

Win64 句柄参数按 LLP64 区分：selector、access、BOOL/options、mask/flags 取 DWORD，HANDLE 和输出地址保留全部 64 位。PE32 未实现的四项保留原清栈、零返回、LastError=120、输出与资源不变，不因 ABI 已知扩展能力。

句柄资源仍由各原 owner 管理。typed ID 在原分支位置选择标准 endpoint、文件、pipe、section、token、snapshot、thread、event 等行为，保留查询顺序、保护位、错误码、提前完成与 unblock 提交。没有统一 handle arena 或新的通用完成框架。

PE32 CloseHandle 先关闭资源、更新 named object，再写 EAX、读取返回地址；返回槽故障时资源与 EAX 已提交。Win64 先保存返回 frame，再执行 handler；输出即使覆盖返回槽，仍返回保存的地址。独立基线位于 `tests/win32/paths.rs`、`tests/win32/handles.rs` 和 `tests/x64/handles.rs`，并覆盖 fault、provider、高 DWORD、输出别名、长度校验先于写入等反向行为。
