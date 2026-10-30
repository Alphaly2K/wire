# 映像资源与字符串

Wasm 入口在 PE32 与 AMD64 接入十项资源接口。LoadStringA/W 只接受 user32，FindResourceA/W/ExA/ExW、LoadResource、SizeofResource、LockResource、FreeResource 接受 kernel32/kernelbase。参数契约来自 `win32/catalog/resources.rs`，新入口的绑定资格独立于该目录保留的旧执行策略。

资源树、名称匹配、语言选择及 RT_STRING 十六槽遍历由新旧入口共用。NULL module 使用主 EXE，module 和名称指针保留完整宽度；数字 key 使用低地址 ID，A 名称按 CP932、W 名称按 UTF-16 解码，目录名称保留 ASCII 大小写折叠。Ex 的语言参数保留完整 DWORD，缺失时回退首个语言项。目录、data entry 和 payload 必须处于声明的映像范围内。

新入口的 HRSRC 与 LoadResource 结果是两种独立的 opaque 身份，同一映像视图重复查询复用身份；Load/Size 必须使用原模块的 HRSRC，Lock 只接受 Load 产生的句柄。Lock 返回实际映像地址，不复制或分配 payload，也不探测 payload 当前可读性。Size/Load 重新读取描述符；已 Load 的视图保存当时的地址与长度。每进程最多 8192 个活动视图，累计身份空间耗尽也返回错误 8。DLL 实际卸载后移除其全部视图，原址重装不复用旧句柄。FreeResource 返回零并保持 LastError。

缺失资源返回零及错误 1814，无效模块/句柄返回 6，畸形资源返回 13。实际访存故障保持调用帧、work 和输出，可在修复权限后重试。普通成功保持 LastError。

LoadString 将 ID 截为 WORD。正容量包含终止 NUL，返回复制的 UTF-16 单元数或 ANSI 字节数；A 使用 CP932，保留 DBCS 内部的字节截断。W 保留原始 UTF-16 单元和嵌入 NUL。W 零容量模式写完整字符串指针及长度，空槽保留其地址，缺失块写零；返回的原始指针仅在相应映像寿命内有效。负容量和 A 零容量直接返回零，不访问输出。新入口保存原返回帧，读取源后一次性提交全部输出，支持源/输出及返回槽别名。

旧 PE32 适配继续保留原 HRSRC/data 地址、LockResource 原值返回和部分写入协议；旧输出仍先于 gateway 读取返回地址。共享遍历没有更改这部分旧入口约定。
