# 文件状态与标准句柄

Win64 的 `x64::runtime::files::Files` 拥有真实文件描述符、sharing/access/flags、目录 cursor、受限 GuestFs、NOT_CONTENT_INDEXED 元数据和标准句柄状态。Runtime64 的 set_filesystem 和供 Process 使用的 filesystem facade 继续委托；文件系统只以明确的只读/可写借用交给需要它的 adapter、loader 和目录查询，没有公开字段或 Deref 到 Runtime64。

移动、删除、目录删除和属性更新由 Files 完成。`PathMutation` 区分各操作；ABI adapter 读取 guest 路径和原始 flags，并处理 LastError、返回 frame 和 trace。路径受限规则、open-handle/cwd 冲突、replace/read-only 检查与原错误码保持原样。

元数据的提交顺序保持原行为：rename 成功后解析目标 canonical path，再收集源目录树的 metadata；先清目标前缀，再将源键迁入。删除成功后清理源键。host 操作与 canonicalize 之间仍可能产生已有的失败状态，没有顺便改为通用原子事务。FindFirst/Next 的原路径键查询与其它 API 的 canonicalize 查询区别也保留。

标准句柄由私有 StandardHandles 拥有：

- redirects 保存 GetStdHandle/SetStdHandle 的三个进程槽位，初始值 `0x100/0x101/0x102`。改槽位不关闭 endpoint；关闭 endpoint 后 GetStdHandle 仍返回已保存的数值。
- open/flags 保存原 endpoint 的寿命和继承/保护关闭标志；aliases 保存复制句柄的 endpoint 身份及独立标志。原 endpoint 关闭后，已有 alias 继续存活。
- allocator 保留 `0x7fff3000..0x7fff4000` 和最多 128 个活动 alias；文件描述符仍保留原数值空间和 1024 上限，不统一 handle arena。
- DuplicateHandle 的输出 preflight、process/options 校验和 source-protect 检查顺序保持。新 alias 分配后由 adapter 写出 guest handle；仅在输出成功后执行 CLOSE_SOURCE。
- narrowed standard access 仍显式拒绝；标准 endpoint 作为重定向 host pipe 查询，console mode 仍失败。stdio 数据传输与 guest 输出检查留在 adapter。

目录 batch 的 cursor 准备/输出/提交、文件 section owner 和 coherence 行为保持各自既有实现。Mappings、Pipes、Searches 的内部状态仍有自己的 owner；本批不合并这些资源。

核心文件 API 的 11 个 export 由 `win32/catalog/files.rs` 统一声明：CreateFileA/W、ReadFile、WriteFile、GetFileSize/Ex、SetFilePointer/Ex、SetEndOfFile、FlushFileBuffers、GetFileType。PE32 全部为 typed 路由，DLL 保持 kernel32；Win64 其中 9 个为 typed，DLL 保持 kernel32/kernelbase，SetFilePointer 与 SetEndOfFile 仍为 absent。Int64 参数明确描述 SetFilePointerEx 的有符号距离，PE32 占两个栈 slot，Win64 占一个 64 位 carrier；DWORD 截断只在 gateway 取参时进行。

路径与属性的 10 个 export 也由同一契约声明：GetCurrentDirectoryA/W、SetCurrentDirectoryA/W、GetFullPathNameA/W、GetFileAttributesA/W、SetFileAttributesA/W。PE32 全部 typed/kernel32，Win64 新增 8 个 typed/kernel32+kernelbase，GetFullPathNameA/W 保持 absent；文件契约合计 21 个 export，Win64 17 typed、4 absent。查询容量和属性 flags 是 DWORD，指针不截断，A/W 选择由绑定 ID 决定。

PE32 GetFullPathName 的小 buffer 只返回所需容量；成功写字符串后再写可选 file-part 指针，因此第二个输出 fault 时字符串已提交。现有宽字符 file-part 偏移也原样保留。Win64 CWD 输出可以覆盖 guest return slot，gateway 继续使用提前保存的返回地址；成功调用保持 LastError。

元数据的五个 export 也已迁入契约：GetFileAttributesExA/W、GetFileInformationByHandle、GetFileInformationByHandleEx、GetFileTime。文件契约合计 26 个 export；PE32 为 21 typed、5 LegacyUnimplemented，Win64 为 22 typed、4 absent。PE32 保留清栈、零返回、LastError=120 与输出不变；Win64 保留原信息类子集、目录 class 14/15 cursor、长度检查、可选时间输出及故障顺序。GetFileAttributesEx 的无效信息类先于路径/输出检查，GetFileTime 先检查全部非空输出再写出；返回槽别名继续使用提前保存的 frame。

PE32 的 immutable binding 同时缓存 LegacyUnimplemented 执行策略，在读取原参数后直接执行旧未实现返回，保留原诊断和 frame 提交。公开 ApiEntry 变化时重新绑定；未知 ABI 与未迁移 API 仍走旧策略。公开 file facade 仅将 Typed 声明当作可执行实现。

PE32 adapter 暂留 `file_api.rs` 的原领域调用边界：主 gateway 用 bound FileApi ID 调用，公开 `file_api::dispatch` 保留原来的大小写/DLL 和函数范围，作为冷解析 facade。内部执行器区分 typed ID 与尚未迁移的目录/SHLWAPI 名称，已迁移调用不再解析 DLL 或 export。GetFileType 仍由主 gateway 执行，invalid/non-file handle 返回 0 并保持 LastError。读写先清零 count，再查 handle/权限和数据 buffer；SetFilePointer/Ex 先定位并清 LastError，再写可选输出，输出失败时定位已提交。错误仍映射成原来的返回值、LastError 和日志文本。

Win64 adapter 暂留 Runtime64 的原 gateway 分支位置，分支由 bound FileApi ID 选择，继续借用已存在的 Files、Pipes、Mappings、标准 endpoint owner 和 guest 内存。打开/读取/写入的权限、sharing、16MiB transfer 上限、section coherence、可选输出及 confined path 保持。定位输出先检查，再改变 offset；GetFileSize 清 LastError 的次序保持。GetFileType 按 standard → file → pipe 查询，未知 handle 返回 0/LastError=6。

Win64 普通文件读写和管道写入继续在原位置提前返回 Control::Returned，gateway 提交提前保存的 CPU return frame、计数和 trace，保留这些 endpoint 原来不调用 scheduler unblock 的行为。管道读入和标准输出写入走原来的通用 unblock/提交路径。没有新建文件完成框架或另移 CPU 提交；这项边界保留是前几个候选性能失败后的渐进取舍。WriteConsoleW 保持原来的拒绝结果，没有借迁移扩展 console 能力。

PE32 目前仍使用 ApiState 的混合 HandleObj 表与 filesystem 字段，属于后续 R3 工作。目录创建/变更/枚举、文件 section、SHLWAPI、标准句柄和跨资源 CloseHandle 等 API 的 catalog 迁移仍待完成，本页不表示整个文件领域已完成。
