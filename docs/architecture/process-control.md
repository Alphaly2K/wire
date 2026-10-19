# 进程控制与文件 endpoint

`ProcessControlApi` 统一声明 CreateProcessW 与 ExitProcess。PE32 前者保留 kernel32 ABI-only 策略，后者仍执行原 DLL detach 和进程停止；Win64 两项接受 kernel32/kernelbase，由冷绑定控制 ID 进入原 CPU/trace/API-count 路径。

Win64 `win32::x64::process_control::probe_child` 只借 guest Memory 和受限 GuestFs，负责原 STARTUPINFO/PROCESS_INFORMATION 预检及应用路径解析。无效 cb 返回具名 InvalidStartup，组合根保留原直接完成路径；文件缺失返回路径与错误码，组合根记录原诊断并正常返回。存在文件时仍显式拒绝尚未实现的 child backend。路径搜索、结构大小、输出顺序和调度 unblock 行为保持。

同批 FileApi 新增 CreatePipe、GetConsoleMode、WriteConsoleW、NtQueryDirectoryFile。前三项在 PE32 仍 ABI-only；NtQueryDirectoryFile 在 PE32 仍未知 ABI，在 Win64 仅接受 ntdll。BOOLEAN 由 adapter 取低 byte，trace 保留完整 carrier，长度/信息类才截断为 DWORD。

匿名管道创建归入已有 Pipes owner，目录 batch 查询及 cursor 提交归入 Files。标准流仍按 redirected pipe 处理，两个 console 调用保留错误 6 和原输出不变行为。Win64 根 ABI 表移除这六项重复声明，其余 legacy 图形方法继续独立迁移。

旧行为基线 `c39a740`。release 整合 Win64 268、PE32 247、library 60 passed / 7 ignored；fmt、release lib/bin strict Clippy 和 build 通过。管道、目录、进程退出、应用解析及错误提交主体归一核对一致。证据位于 `wire-out/refactoring-2026-10-14/r2-endpoints-{source-checks,verification}.json` 及相应 validation 日志；只保留 executable SHA/size，没有复制二进制。普通契约与 owner 迁移不运行性能或游戏矩阵。
