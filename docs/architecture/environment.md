# 环境变量与环境块

新 Wasm 入口接入十一项环境服务，两种 ABI 均接受 kernel32/kernelbase：GetEnvironmentStrings、GetEnvironmentStringsA/W、FreeEnvironmentStringsA/W、GetEnvironmentVariableA/W、SetEnvironmentVariableA/W 和 ExpandEnvironmentStringsA/W。共享目录中的旧 PE32 空环境及 Win64 ANSI absent 策略仅用于旧入口。

进程复用 `src/win32/environment_profile.rs` 的变量模型与不可变虚拟 profile，不继承宿主环境。初始变量包括 SystemRoot/windir、SystemDrive、TEMP/TMP、用户目录、用户/计算机名、OS、PATH、单处理器数量及客体架构；取值来自虚拟 Windows 默认值。变量以 Unicode 小写键匹配，保留最近写入的名称和值，并生成有序环境块。

Set 成功后更新 PEB.ProcessParameters 的 Environment 指针和环境块字节数；当前块为双 NUL 终止的 UTF-16。启动分配同时避开调用者已有映射与本次暂存映射。输入复制及元数据写权限、旧块释放范围全部验证后才分配替换块、发布指针并退休旧块。线程共享同一变量模型；PEB 环境块可供 CRT 直接读取，原始指针在变量修改后可能失效。直接修改客体环境块不会更新变量模型，应通过 SetEnvironmentVariable 修改变量。

变量查询的 A 版本使用 CP932，W 版本按 UTF-16 code unit 计数；容量为 DWORD。不足容量返回包含 NUL 的所需长度并保持输出，成功返回不含 NUL 的长度，保留 LastError。名称为 NULL、空或包含 `=` 返回零/87；缺失变量及删除不存在的变量返回零/203。NULL value 表示删除，非 NULL 的空串作为空值保存。输入沿统一的 32768 单位终止字符串上限读取，输出故障可修复后重试，返回槽在写输出前保存。

GetEnvironmentStrings 与 A 后缀返回 ANSI 快照，W 后缀返回 UTF-16 快照；快照不随变量修改而改变。每个快照必须由对应的 FreeEnvironmentStringsA/W 释放，错误指针、编码不匹配和重复释放返回零/87。当前 PEB 环境块与快照均由同一 VMM 提供 runtime-owned backing，普通 HeapFree/VirtualFree 不释放这些块。静态启动拒绝统一回收当前块和全部快照，线程退出保留进程环境。

变量最多 1024 项，UTF-16 环境块最多 2 MiB；已发出快照最多 4096 项、合计 64 MiB 页额度，超限返回零/8。失败不发布变量或块身份；成功服务计一 work、不计客体退休，全部环境服务均不采样时间。

ExpandEnvironmentStrings 从同一模型做一次 `%name%` 替换，名称不区分大小写；不存在的变量、空名称和没有闭合的 `%` 保持原文本，变量值中的 `%` 不继续展开。返回值始终包含终止 NUL；容量为零或不足时只返回所需长度，保持输出和 LastError。源与非查询目标为同一地址返回零/87；其他输出先保存输入并预检完整写入范围，允许返回槽别名。输入沿统一终止字符串上限读取，展开文本及编码结果各有 16 MiB 额度，放大超限返回零/8。
