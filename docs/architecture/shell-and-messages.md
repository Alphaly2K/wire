# Shell 目录、命令行与错误消息

两项 MessageFormatApi 和六项 ShellApi 汇入唯一契约，累计 **875 exports**。FormatMessage 在 PE32 仅 kernel32 typed，Win64 kernel32/kernelbase typed。Shell 在 PE32 为 shell32 的五项目录 shim typed、CommandLineToArgvW ABI-only；Win64 只保留 argv/known-folder typed，另外四项 absent。

Win64 argv/known-folder 的原静态导入同时接受 kernel32/kernelbase/shell32。三个 provider 的 fresh GetProcAddress 仍返回 0/error 127；只有已经注册同 provider 的 gateway 才能被动态发现。该历史差异由 lookup plan 和既有 gateway fallback 表达，未在迁移时扩大动态查询范围。

PE32 shell adapter 只借 GuestMem、GuestHeap、process-heap handle 和 LastError。旧目录 shim 继续返回 `C:\`、忽略原本忽略的参数；known-folder 先将输出清零，再分配 8 字节、写入路径并提交指针。错误映射、trim/目录 note、堆资源和返回槽顺序保持。公开 `wire::nls::dispatch` 保留原精确 DLL/五项 shell 函数范围，转调同一 adapter；main gateway 直接使用 bound ID。

Win64 shell adapter 只借受检 Memory、GuestEnvironment 与 confined filesystem，返回准备好的 argv 字符串或 known-folder 数据。纯命令行 parser 迁入 kernel 算法模块，处理 executable、空参数、引号和反斜杠的原算法保持。运行时协调原 heap 分配、指针表重定位和 copyout：argv 仍归 LocalFree，known-folder 仍归 CoTaskMemFree；目录创建、路径约束和输出故障顺序保持。

PE32 FormatMessage 整体迁入窄借用 adapter，保留原小型错误文本集合、CP932/UTF-16 计数、进程堆分配、错误/输出顺序与 trim note。Win64 继续保留自己的 flags、language、insert 限制和错误文本集合；adapter 负责准备/受检写出，运行时负责 heap 分配和 local-block 登记。模块装载器不再负责消息格式化。两种架构原有差异分别保留。

旧行为基线 `d034c5b`，新增静态/动态 provider 和 PE32 分配/清栈检查先在旧实现通过。parser、PE32 消息主体、Win64 argv/known-folder 准备和提交、Win64 消息准备经 ID/窄借用/空白归一与原源码一致。正确性及源码/executable SHA 证据在 `wire-out/refactoring-2026-10-14/r2-shell-message-verification.json`；不复制游戏或二进制，不跑性能矩阵。

release 验收：Win64 modules 12、shell provider 1、argv/known-folder/local lifetime 各 1，PE32 kernel 13、启动参数 roundtrip 与 catalog 单元各 1，共 31 项；fmt、release lib/bin strict Clippy/build 通过。
