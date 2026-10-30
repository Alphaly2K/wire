# 进程身份与 LastError

GetCurrentProcessId、GetCurrentThreadId、GetCurrentProcess、GetCurrentThread、IsDebuggerPresent、GetLastError 和 SetLastError 的 provider 与 ABI 来自 `win32/catalog/context.rs`。新 Wasm 入口在两种架构使用当前客体 TEB/PEB；状态归属见[线程生命周期](thread-control.md)。

| 行为 | PE32 | Win64 |
| --- | --- | --- |
| provider | kernel32 | kernel32/kernelbase |
| 默认根进程 PID / 初始 TID | 1 / 1 | 1 / 1 |
| 后续 TID | 单调递增 DWORD | 单调递增 DWORD |
| pseudo handle | 32 位 −1/−2 | 完整 64 位 −1/−2 |
| TEB | FS 基址 | GS 基址 |
| LastError | TEB+0x34 的 DWORD | TEB+0x68 的 DWORD |
| PEB 引用 | TEB+0x30 | TEB+0x60 |

GetCurrentProcessId 和 GetCurrentThreadId 读取当前 TEB 的身份字段；IsDebuggerPresent 读取 PEB+2 并归一化为 BOOL。每个线程有独立 LastError，SetLastError 截断参数高 32 位。调度器保存和恢复段基址，因此线程切换不复制另一线程的错误状态。

核心装载可以接收独立的进程身份，初始/后续线程 TEB、TLS 成员、加载锁、进程查询与 Toolhelp 使用同一身份。每个进程保留互不重叠的单调 TID 区间，句柄仍由对应进程所有；无效 PID 或不匹配的句柄 ABI 在发布映像前拒绝。默认 Web 装载保持上述根进程身份；子进程创建和跨进程句柄调度的接入状态见 GOAL。

调用帧在访问服务输出之前保存返回地址及栈完成位置。参数、返回槽或 TEB/PEB 访问失败时保留 CPU 和 work；成功服务计一 work，不计客体退休，不读取平台时钟。

OutputDebugStringA/W 在两种 ABI 通过 kernel32/kernelbase 提供。当前没有外部调试器或系统调试输出接收端；调用仍向客体 VEH 投递调试事件，未接收的输出由内部处理结束。A 使用原始字节，W 使用 UTF-16 并按当前 CP932 转换 ANSI fallback；NULL A 为一个空字符串事件，NULL W 直接完成。字符串需在 65536 个字节或 UTF-16 单元内终止。异常参数、宽/窄分发与恢复契约见[异常投递](exceptions.md)。

RtlCaptureContext 在两种 ABI 接受 kernel32/kernelbase/ntdll，使用逻辑 API 调用者的 GPR、控制状态和返回 PC/SP，保留 RAX/EAX 与 LastError。i386 输出 ContextFlags=0x10007，AMD64 输出 0x10000f；仅更新这些标志覆盖的字段，未请求的存储保持原字节。AMD64 同时捕获 XMM/MXCSR 与 x87，要求 CONTEXT 按 16 字节对齐。数据和保存帧可以别名，写入前保存完整返回状态；访问或对齐失败不提交 CPU/work。挂起线程的选择性获取和设置见[线程上下文](thread-control.md)。

GetComputerNameA/W、GetComputerNameExA/W 和 GetUserNameA/W 使用同一虚拟 profile。计算机名接受 kernel32/kernelbase；用户名来自 advapi32，PE32 还保留 kernel32 的历史别名。NetBIOS 名为 `WIRE`，DNS hostname/FQDN 为 `wire`，未加入域时 DNS domain 为空，用户名为 `wire`；不读取宿主账户或主机配置。

容量经指针读写 DWORD；成功时计算机名长度不包含 NUL，用户名长度包含 NUL。不足容量时写入包含 NUL 的所需长度，并分别返回错误 111（普通计算机名）、234（扩展计算机名）或 122（用户名）；输出缓冲区保持原值。长度和错误槽先统一预检，成功输出也先检查字符串与长度槽，故障不留下部分结果。扩展格式接受 0–7，其余返回零/87，且不读取输出指针；格式参数按 DWORD 截断。
