# 进程控制与文件 endpoint

`ProcessControlApi` 统一声明 CreateProcessW、ExitProcess 与 TerminateProcess。PE32 CreateProcessW 保留 kernel32 ABI-only 策略，ExitProcess 仍执行原 DLL detach 和进程停止。TerminateProcess 接受 pseudo process handle 或 Process 对象，不检查原来未检查的 access mask；成功先设置 halted/exit code，再经过公共返回路径，返回槽故障也保留退出提交；无效对象设置错误 6。它不执行 DLL detach。Win64 前两项接受 kernel32/kernelbase，由冷绑定控制 ID 进入原 CPU/trace/API-count 路径，TerminateProcess 保持 absent。

同批把 SetHandleCount 和 GetCurrentPackageId 接入各自 HandleApi / ProcessQueryApi。PE32 仅接受 kernel32：容量提示原样返回输入、不创建句柄；package 查询返回 APPMODEL_ERROR_NO_PACKAGE，不写输出或 LastError。原 trace 保持。Win64 两项均 absent，动态查询仍返回错误 127。

Win64 `win32::x64::process_control::probe_child` 只借 guest Memory 和受限 GuestFs，负责原 STARTUPINFO/PROCESS_INFORMATION 预检及应用路径解析。无效 cb 返回具名 InvalidStartup，组合根保留原直接完成路径；文件缺失返回路径与错误码，组合根记录原诊断并正常返回。存在文件时仍显式拒绝尚未实现的 child backend。路径搜索、结构大小、输出顺序和调度 unblock 行为保持。

同批 FileApi 新增 CreatePipe、GetConsoleMode、WriteConsoleW、NtQueryDirectoryFile。前三项在 PE32 仍 ABI-only；NtQueryDirectoryFile 在 PE32 仍未知 ABI，在 Win64 仅接受 ntdll。BOOLEAN 由 adapter 取低 byte，trace 保留完整 carrier，长度/信息类才截断为 DWORD。

匿名管道创建归入已有 Pipes owner，目录 batch 查询及 cursor 提交归入 Files。标准流仍按 redirected pipe 处理，两个 console 调用保留错误 6 和原输出不变行为。Win64 根 ABI 表移除这六项重复声明，其余 legacy 图形方法继续独立迁移。

旧行为基线 `c39a740`。release 整合 Win64 268、PE32 247、library 60 passed / 7 ignored；fmt、release lib/bin strict Clippy 和 build 通过。管道、目录、进程退出、应用解析及错误提交主体归一核对一致。证据位于 `wire-out/refactoring-2026-10-14/r2-endpoints-{source-checks,verification}.json` 及相应 validation 日志；只保留 executable SHA/size，没有复制二进制。普通契约与 owner 迁移不运行性能或游戏矩阵。

后续独立基线 `12ed607` 固定三个 PE32 kernel endpoint 的 provider、输出不变、清栈及退出故障顺序；生产迁移后完整 Win32 251、Win64 modules 12、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过。原终止和容量处理体归一一致。累计 891 exports，证据为 `wire-out/refactoring-2026-10-14/r2-kernel-endpoints32-{source-checks,verification}.json`，未跑性能矩阵。
