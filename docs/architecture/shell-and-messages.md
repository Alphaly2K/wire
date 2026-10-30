# Shell 目录、命令行与错误消息

新 Wasm 入口在 PE32 与 AMD64 均提供 kernel32/kernelbase 的 FormatMessageA/W。统一捕获七个参数和原返回帧，DWORD 保留低 32 位，源、输出和 Arguments 指针保留架构宽度。两项服务无需平台时间，完成计一 work，不增加客体退休数。

| 模式 | 契约 |
| --- | --- |
| FROM_SYSTEM | 返回既有英文消息集合，追加 CRLF；ID 包含 0、2、3、5、6、8、32、50、87、122、126、127、183、203，未知 ID 返回错误 317 |
| FROM_STRING | 复制调用者提供的 NUL 结尾字符串，保持原行尾；读取上限为 32768 个字节或 UTF-16 单元，包含终止符 |
| IGNORE_INSERTS | 不访问 Arguments，原样保留源中的插入占位；ARGUMENT_ARRAY 可与之组合 |
| 调用者缓冲区 | A 按 CP932 字节计数，W 按 UTF-16 单元计数；返回值不含 NUL，容量须包含 NUL；容量不足返回错误 122 并保留输出 |
| ALLOCATE_BUFFER | 预检指针宽度的输出后分配固定 Local 块，大小为 `max(nSize, count + 1) × 单元宽度`；LocalSize 返回字节数，由 LocalFree 释放 |

系统消息支持语言 0、0x400 和 0x409，均使用同一英文集合。消息文本从旧内存适配中提取到便携的 `win32/message_text.rs`；新入口直接使用共享词汇、CP932 转换与进程堆，不调用旧运行时。调用者缓冲区模式也可用于裸 CPU；分配模式要求已经装载映像并建立进程堆。

未设置 IGNORE_INSERTS 时，当前只接受无百分号且 Arguments 为 NULL 的文本。插入参数、转义控制、自动折行、模块消息表和其它语言均返回错误 50；不消费 va_list 或参数数组。无消息来源、FROM_STRING 与其它来源混用或未知标志返回错误 87。合法但未实现的 FROM_HMODULE 来源返回错误 50，包含其与 FROM_SYSTEM 的组合。

消息源在写入前完整读取，允许源与目标或输出指针重叠。直接输出和输出指针统一预检完整写入范围；分配路径在初始化整个新块并写入文本后发布指针。输出故障保留原缓冲区、原指针、CPU 和计数，不消耗空闲堆块；堆块上的 guard 失败可在同一调用重试。配额不足返回错误 8，原输出指针保持不变；成功保留 LastError。输出覆盖返回槽时仍使用保存的返回目标。块随[进程堆](heaps.md)回收，并沿同一 VMM 路径参与代码失效。

CommandLineToArgvW 在 PE32/AMD64 均由 shell32 提供，不接受 kernel32/kernelbase 别名。输入按原始 UTF-16 读取，上限为 32768 个单元（含 NUL），保留未配对代理项；空输入返回当前映像完整路径，前导空白建立空的首参数。首参数与其余参数分别遵循 Windows 引号和反斜杠规则。

返回指针指向单个固定 Local 块，前部为客体指针宽度的 argv 表及 NULL 终止指针，后部为所有 NUL 结尾 UTF-16 字符串。参数个数按 DWORD 输出，LocalSize 返回整个块字节数，由 LocalFree 一次释放。源与计数输出可以重叠，返回槽别名使用保存帧；计数输出、输入和待复用堆页的故障不提交计数、返回、work 或分配身份，修复后可重试。成功保留 LastError，计一 work，不采样时钟。

SHGetKnownFolderPath 在 PE32/AMD64 均由 shell32 提供，复用旧入口的 GUID 到虚拟 profile 目录映射，支持 LocalAppData、LocalAppDataLow、RoamingAppData、Profile 和 Documents。成功返回 NUL 结尾的 UTF-16 字符串，由 CoTaskMemFree 释放；分配属于当前进程的 COM task owner，与 Local/Global 块独立。GUID 与结果指针可重叠，结果也可覆盖已保存的返回槽。

只接受空 token，以及 KF_FLAG_CREATE（0x8000）和 KF_FLAG_DONT_VERIFY（0x4000）。CREATE 通过已有有界 VFS 创建目录，DONT_VERIFY 允许查询尚不存在的目录而不创建；默认要求目录确实存在。未知 GUID、token 或其余 flags 返回错误 50 的 HRESULT，目录不存在返回错误 3 的 HRESULT；语义失败清空结果指针，LastError 保持。flags 按 DWORD 处理，token 和结果指针保留完整架构宽度。

完整结果槽先于目录变更和分配预检，GUID 在输出前完整复制。访存故障保留原输出、CPU 和服务 work，不消耗可复用 task 块，修复后可重试。映射的默认值和 GUID 识别由新旧入口共同使用；新入口的目录检查和创建只访问虚拟文件系统。

PE32/AMD64 均接入 shell32 的 SHGetFolderPathA/W 和 SHGetSpecialFolderPathA/W，支持 CSIDL_PERSONAL、APPDATA、LOCAL_APPDATA、WINDOWS、SYSTEM 和 PROFILE 六个 profile 映射。只接受 CREATE/DONT_VERIFY 标志；FolderPath 要求 NULL token，类型接受 CURRENT/DEFAULT。未知标志或类型返回错误 87，其他 CSIDL/token 返回 50。SpecialFolder 的 create 参数与 CSIDL_CREATE 合并。默认验证 VFS 目录，CREATE 原子创建父链，DONT_VERIFY 允许目录尚不存在且不创建。

A 输出按 profile CP932，W 按 UTF-16，成功只写实际路径及 NUL，长度不超过 MAX_PATH；NULL 输出只执行验证/创建。完整输出先于目录变更预检，故障不创建目录。语义失败只清输出的首个 NUL：FolderPath 返回对应 HRESULT 并保持 LastError；SpecialFolder 返回零并写 LastError，错误槽与 NUL 统一预检。成功保持 LastError，不分配堆或读取时钟。两种 ABI 共用同一 profile/VFS 行为，输出指针保留完整架构宽度。
