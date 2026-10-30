# 路径辅助与通用句柄

新 Wasm 入口在两种 ABI 接入 FileApi 的 GetFullPathNameA/W，接受 kernel32/kernelbase。它按虚拟 C 盘当前目录解析绝对、相对、drive-relative 路径及 `.`/`..`，不查询宿主文件系统，也不要求目标存在；其他卷、UNC 和根逃逸沿现有虚拟命名空间规则拒绝。

容量为 DWORD，A 按 CP932 字节、W 按 UTF-16 单位计数。零容量或不足容量返回包含 NUL 的所需长度，两个输出均保持原值；成功返回不含 NUL 的长度。末尾分隔符保留，根或以分隔符结尾时 file-part 为 NULL，否则指向输出中的最后一个名称分量。ANSI 偏移在编码后计算，不把 CP932 trail byte 当作路径分隔符。

完整字符串与可选 file-part 指针按对应 ABI 宽度统一预检，再依次写入；故障不留下字符串前缀或半个指针。指针输出可与字符串重叠，最后写入的指针保留；返回槽在输出前保存。成功保持 LastError，完成计一 work、不计客体退休，也不采样时间。当前目录与 DLL 搜索目录见[模块契约](modules.md)，环境展开见[环境变量](environment.md)。

SHLWAPI 的 Exists/IsDirectory/RemoveFileSpec/RenameExtension/AddExtension/UnquoteSpaces、imagehlp 的 MakeSureDirectoryPathExists 和 MPR 的 WNetGetUniversalNameA/W 已在两种 ABI 接入，严格限制各自 provider。新旧入口共用 `win32/path_helpers.rs` 的 Wine 衍生根路径、CP932 trail byte、扩展名和引号算法，保留原许可证声明。RemoveFileSpec 按原 byte/UTF-16 单元操作，只将选中的末级分量写为 NUL；扩展名按最后一个分隔符/空格后的点选择位置，以 A 字节或 W 单元检查 MAX_PATH=260。Unquote 保留零返回。NULL 转换输入短路返回零，查询/建目录的 NULL 输入返回错误 87。

文件存在/目录查询使用同一进程 VFS 和当前目录，返回实际元数据；递归创建按输入是否有尾分隔符选择整个路径或其父目录。各级目录在配额与类型检查后整批发布，删除末级不删除已创建父目录。MPR 无网络映射时返回 ERROR_NOT_CONNECTED=2250，忽略指针并保持 LastError。新入口保存原返回帧、复制原输入后提交完整输出，源/输出/错误槽故障可修复重试；成功保持 LastError，调用无需时钟。

PathCanonicalizeA/W 接受 shlwapi provider，使用同一份已保留许可的 Wine 词法算法；不查询 VFS，其他驱动器、UNC 和不存在的路径均可作为字符串处理。反斜线参与 `.`/`..` 归约，正斜线保持普通字符语义；空输入产生反斜线，裸驱动器后补反斜线。A 使用 CP932 转换，W 保留原 UTF-16 单元，包括未配对代理项。输入与输出按 MAX_PATH=260 个相应单位约束，超长返回 FALSE/206。

规范化先暂存完整输入，支持原地和重叠输出。NULL 输入/输出返回 FALSE/87；失败时非空输出清为 NUL。错误槽和要清除的输出在发布前统一预检，故障不会先清空输出；成功保留 LastError，输出覆盖返回帧仍使用已捕获的目标。算法来源为已固定的 Hangover/Wine path.c，普通词法用例另对照公开 Wine SHLWAPI 接口检查。

通用句柄通过各资源 owner 管理。两种 ABI 已接入 GetStdHandle/SetStdHandle、CloseHandle、DuplicateHandle、GetHandleInformation/SetHandleInformation；PE32 另接入 SetHandleCount 的容量提示，Win64 不发布该入口。完整 HANDLE 与输出指针保留架构宽度，selector、access、options、mask/flags 取 DWORD。

关闭、复制与标志操作按普通文件、标准端点、pipe、section、线程和同步对象分别验证资源身份、访问权与保护位；当前进程句柄使用完整伪句柄。调用帧和输出在资源修改前捕获/预检，故障不提交资源变化。详细寿命规则见[文件](files.md)、[线程句柄](thread-properties.md)和[同步对象](sync-objects.md)。

内核资源使用互不重叠的数值区间；下表右端均不包含。PE32 token 和注册表键采用独立区间，AMD64 的这两类句柄使用各自完整高地址，见对应契约。

| 资源 | 区间 |
| --- | --- |
| 普通文件 | `0x00100000..0x7ffe0000`，跳过 section 的 `0x00180000..0x00190000`。 |
| Toolhelp 快照 | `0x7ffe0000..0x7ffe1000`。 |
| 当前进程真实句柄 | `0x7fff0000..0x7fff3000`。 |
| 标准端点别名 | `0x7fff3000..0x7fff4000`。 |
| 线程句柄 | `0x7fff4000..0x80000000`。 |
| 文件查找 | `0x80000000..0x90000000`。 |
| 同步对象 | `0x90000000..0xa0000000`。 |
| PE32 token | `0xa0000000..0xa0001000`。 |
| PE32 注册表键 | `0xa0010000..0xa0020000`。 |

文件上界同时给出新快照区间的起点；进程句柄在标准别名起点前耗尽，线程从标准别名上界开始。同步对象在 PE32 token 起点前耗尽。关闭不复用身份，达到各资源活动或累计容量后明确失败；分派顺序不用于区分重叠身份。
