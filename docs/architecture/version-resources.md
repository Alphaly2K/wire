# 文件版本资源

新 Wasm 入口在 PE32/AMD64 均接受 version.dll 的 GetFileVersionInfoA/W、GetFileVersionInfoSizeA/W 和 VerQueryValueA/W。文件字节来自统一 VFS；任意一种客体 ABI 均可查询 PE32 或 PE32+ 文件，无须执行或初始化该文件。路径编码和共享权限沿现有文件接口，ANSI 使用进程 ACP 932。

GetFileVersionInfoSize 返回整个资源项大小的两倍加四字节，供原始资源和 ANSI 转换共同使用；非 NULL 的 handle 输出为零，API 返回零时也写零。GetFileVersionInfo 忽略旧 handle 参数，复制不超过 DWORD 容量的资源前缀，容量不足时仍返回成功；有足够空间时在根节点结尾写入转换标记，不清空其余转换空间。NULL 文件名、空文件名、NULL 数据输出分别返回错误 87、161 和 13；VFS 缺失路径和共享冲突保持文件服务的错误。两类获取接口成功后清零 LastError。

资源查找使用 RT_VERSION 下的 ID 1，语言依次尝试中性语言、当前线程 locale、用户/系统默认及其主语言、英语，最后回退到首个语言。当前用户和系统默认语言均为日语。资源数据必须落在实际文件 backing 内，单项上限为 1 MiB。不存在版本项返回错误 1813，不支持的 PE 格式返回 1812。

VerQueryValue 的 NULL/空路径查询根节点，其余路径以反斜杠分层，英文字母不区分大小写。根 VS_FIXEDFILEINFO 和 Translation 的长度以字节计，宽字符串长度以 UTF-16 单元计，ANSI 字符串长度以编码字节计；字符串长度包含资源中已有的结尾 NUL，空值长度为零。值指针始终指向调用者缓冲区，AMD64 保留完整指针宽度，长度输出固定为四字节且可为 NULL。ANSI 文本写入 GetFileVersionInfoSize 预留的转换区域，原宽文本保持不变；必须按返回的整体大小分配并保留缓冲区。

查询成功保持 LastError；路径不存在返回 FALSE、错误 1813，并将非 NULL 的长度输出清零，指针输出保持原值。NULL 根缓冲区返回 FALSE 并保持输出与错误槽。畸形节点返回错误 13。输入、调用帧或输出访存故障暂停在原服务位置，修复后可重试；全部输出及错误槽统一预检，故障不会留下已写前缀。参数和返回帧在输出前保存，输出覆盖原返回槽仍按保存的位置返回；输出彼此别名时按数据、指针、长度和 LastError 的顺序提交。

接口契约见 Microsoft 的 [GetFileVersionInfo](https://learn.microsoft.com/en-us/windows/win32/api/winver/nf-winver-getfileversioninfow)、[GetFileVersionInfoSize](https://learn.microsoft.com/en-us/windows/win32/api/winver/nf-winver-getfileversioninfosizew) 和 [VerQueryValue](https://learn.microsoft.com/en-us/windows/win32/api/winver/nf-winver-verqueryvaluew)。资源容量、转换区域和语言回退另核对了 Wine 的公开实现。NE/16 位资源、MUI 分离资源合并、完整 Unicode 键名大小写规则与 Ex 接口尚未接入。

共享资源/节点解析位于 `src/win32/kernel/version_resources.rs`，旧 PE32 适配器继续保留自身的输出与别名契约；新入口不调用旧文件系统或执行器。
