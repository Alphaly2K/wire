# 文件状态与标准句柄

Win64 的 `x64::runtime::files::Files` 拥有真实文件描述符、sharing/access/flags、目录 cursor、受限 GuestFs、NOT_CONTENT_INDEXED 元数据和标准句柄状态。Runtime64 的 set_filesystem 和供 Process 使用的 filesystem facade 继续委托；文件系统只以明确的只读/可写借用交给需要它的 adapter、loader 和目录查询，没有公开字段或 Deref 到 Runtime64。

移动、删除、目录删除和属性更新由 Files 完成。`PathMutation` 区分各操作；`files/calls.rs` adapter 读取 guest 路径和原始 flags，在原错误分支只读借 Threads 写 LastError，Runtime 保留返回 frame 和 trace。路径受限规则、open-handle/cwd 冲突、replace/read-only 检查与原错误码保持原样。

元数据的提交顺序保持原行为：rename 成功后解析目标 canonical path，再收集源目录树的 metadata；先清目标前缀，再将源键迁入。删除成功后清理源键。host 操作与 canonicalize 之间仍可能产生已有的失败状态，没有顺便改为通用原子事务。FindFirst/Next 的原路径键查询与其它 API 的 canonicalize 查询区别也保留。

标准句柄由私有 StandardHandles 拥有：

- redirects 保存 GetStdHandle/SetStdHandle 的三个进程槽位，初始值 `0x100/0x101/0x102`。改槽位不关闭 endpoint；关闭 endpoint 后 GetStdHandle 仍返回已保存的数值。
- open/flags 保存原 endpoint 的寿命和继承/保护关闭标志；aliases 保存复制句柄的 endpoint 身份及独立标志。原 endpoint 关闭后，已有 alias 继续存活。
- allocator 保留 `0x7fff3000..0x7fff4000` 和最多 128 个活动 alias；文件描述符仍保留原数值空间和 1024 上限，不统一 handle arena。
- DuplicateHandle 的输出 preflight、process/options 校验和 source-protect 检查顺序保持。新 alias 分配后由 adapter 写出 guest handle；仅在输出成功后执行 CLOSE_SOURCE。
- narrowed standard access 仍显式拒绝；标准 endpoint 作为重定向 host pipe 查询，console mode 仍失败。stdio 数据传输与 guest 输出检查留在 adapter。

目录 batch 的 cursor 准备/输出/提交、文件 section owner 和 coherence 行为保持各自既有实现。Mappings、Pipes、Searches 的内部状态仍有自己的 owner；本批不合并这些资源。

核心文件 API 的 11 个 export 由 `win32/catalog/files.rs` 统一声明：CreateFileA/W、ReadFile、WriteFile、GetFileSize/Ex、SetFilePointer/Ex、SetEndOfFile、FlushFileBuffers、GetFileType。PE32 全部为 typed 路由，DLL 保持 kernel32；Win64 其中 9 个为 typed，DLL 保持 kernel32/kernelbase，SetFilePointer 与 SetEndOfFile 仍为 absent。Int64 参数明确描述 SetFilePointerEx 的有符号距离，PE32 占两个栈 slot，Win64 占一个 64 位 carrier；DWORD 截断只在 gateway 取参时进行。

路径与属性的 10 个 export 也由同一契约声明：GetCurrentDirectoryA/W、SetCurrentDirectoryA/W、GetFullPathNameA/W、GetFileAttributesA/W、SetFileAttributesA/W。PE32 全部 typed/kernel32，Win64 其中 8 个 typed/kernel32+kernelbase，GetFullPathNameA/W 保持 absent。查询容量和属性 flags 是 DWORD，指针不截断，A/W 选择由绑定 ID 决定。

PE32 GetFullPathName 的小 buffer 只返回所需容量；成功写字符串后再写可选 file-part 指针，因此第二个输出 fault 时字符串已提交。现有宽字符 file-part 偏移也原样保留。Win64 CWD 输出可以覆盖 guest return slot，gateway 继续使用提前保存的返回地址；成功调用保持 LastError。

元数据的五个 export 也已迁入契约：GetFileAttributesExA/W、GetFileInformationByHandle、GetFileInformationByHandleEx、GetFileTime。PE32 五个均为 LegacyUnimplemented，Win64 五个均为 typed。PE32 保留清栈、零返回、LastError=120 与输出不变；Win64 保留原信息类子集、目录 class 14/15 cursor、长度检查、可选时间输出及故障顺序。GetFileAttributesEx 的无效信息类先于路径/输出检查，GetFileTime 先检查全部非空输出再写出；返回槽别名继续使用提前保存的 frame。

PE32 的 immutable binding 同时缓存 LegacyUnimplemented 执行策略，在读取原参数后直接执行旧未实现返回，保留原诊断和 frame 提交。公开 ApiEntry 变化时重新绑定；未知 ABI 与未迁移 API 仍走旧策略。公开 file facade 仅将 Typed 声明当作可执行实现。

目录与文件变更另有 12 个 export：CreateDirectoryA/W、DeleteFileA/W、MoveFileA/W、MoveFileExA/W、RemoveDirectoryA/W、CopyFileA/W。PE32 仅 CreateDirectory/DeleteFile 使用 typed 路由；其余保留未实现策略。Win64 CopyFile 保持 absent。PE32 CreateDirectory 继续忽略 SECURITY_ATTRIBUTES，Win64 先读取 lpSecurityDescriptor 并拒绝显式 descriptor；后者提前完成时的 unblock/API count/trace 不变。MoveFileEx flags 是 DWORD，source/destination 解码先于 flags/空目标校验，Files 的 mutation 和 metadata 提交顺序保持。

目录枚举的 7 个 export 已迁入同一契约：FindFirstFileA/W、FindFirstFileExA/W、FindNextFileA/W、FindClose。文件契约合计 45 个，PE32 32 Typed/13 ABI-only，Win64 39 Typed/6 Absent；DLL 仍为 PE32 kernel32、Win64 kernel32/kernelbase。HANDLE 使用指针宽度，Ex 的信息类、搜索操作和 flags 使用 DWORD。绑定 ID 决定 A/W、First/Next/Close 和 Ex 子集，主 gateway 不再逐调用解析名称。

枚举保持无效 handle/Ex flags 先于路径和输出检查，成功输出后才分配搜索句柄或推进游标。PE32 FindClose 不会删除普通文件句柄；搜索输出仍按原顺序分步写入，保留可能的部分提交。Win64 先检查整块输出再读取路径，输出可覆盖返回槽和栈参数，gateway 使用预先保存的参数及返回 frame。原搜索句柄空间、entry/handle 上限、metadata 和 LastError 行为不变，Searches 继续拥有 Win64 的枚举状态。

Win64 FindFirst/Next/Close 的处理也由 Searches 执行，只借内存、只读 Files 和 Threads；Files 提供原 metadata 编码。全文目录信息调用归 Files，batch 准备、guest 输出、cursor 提交和 NO_MORE_FILES 后更新的顺序保持。Threads 拥有当前 TEB 的 LastError 写入，错误分支才查询 TEB；成功路径不增加错误槽读取。Runtime64 的旧调用入口仅组合这些资源，原 NtQueryDirectoryFile 与 directory_batch 主体保持。

PE32 adapter 位于 `win32/x86/files.rs`：主 gateway 用 bound FileApi/PathApi ID 调用，公开 `file_api::dispatch` 保留原来的大小写/DLL 和函数范围，作为冷解析 facade。adapter 只借 GuestMem、KernelObjects32、filesystem 和 LastError 槽，不接收 ApiState；公开 read_name/write_string 路径继续重导出。内部互斥 Core/Helper ID 区分文件与 SHLWAPI/imagehlp 操作，已迁移调用不再解析 DLL 或 export。GetFileType 仍由主 gateway 执行，invalid/non-file handle 返回 0 并保持 LastError。读写先清零 count，再查 handle/权限和数据 buffer；SetFilePointer/Ex 先定位并清 LastError，再写可选输出，输出失败时定位已提交。错误仍映射成原来的返回值、LastError 和日志文本。

Win64 普通 metadata、大小/flush、seek、attributes 和 cwd adapter 位于 `files/calls.rs`，由 Files 执行，只借 Memory 与只读 Threads。普通文件的 read/write 输出也归 Files；write 明确只读借 Mappings 更新 view。Runtime64 按 bound FileApi 选择原分支，保留跨 Files/Pipes/Mappings/标准 endpoint 的协调。打开/读取/写入的权限、sharing、16MiB transfer 上限、section coherence、可选输出及 confined path 保持。定位输出先检查，再改变 offset；GetFileSize 清 LastError 的次序保持。GetFileType 按 standard → file → pipe 查询，未知 handle 返回 0/LastError=6。

Win64 普通文件读写和管道写入继续在原位置提前返回 Control::Returned，gateway 提交提前保存的 CPU return frame、计数和 trace，保留这些 endpoint 原来不调用 scheduler unblock 的行为。管道读入和标准输出写入走原来的通用 unblock/提交路径。没有新建文件完成框架或另移 CPU 提交；这项边界保留是前几个候选性能失败后的渐进取舍。WriteConsoleW 保持原来的拒绝结果，没有借迁移扩展 console 能力。

普通查询和文件读写提取基线 `8f73bc8`。处理体经资源替换、FileApi 模式规范化及 rustfmt 对照一致，根分发除对应调用替换外保持；读入 preflight 仍在原位置。Files 模型、Mapping/目录主体和文件回归期望未改；Threads 只在原错误分支写 LastError，错误槽不提前取样。

后续基线 `e67b4e0` 将路径变更、SetFileAttributes、普通创建目录及 CreateFile 的 guest 解码/输出也归 Files。打开文件只读借 Mappings 核对活动映射，truncate 与 sharing 的原次序保持；创建目录的安全描述符检查及其提前提交仍在 Runtime。四个处理体及根调用替换经 rustfmt 对照一致，未改文件/Mapping 模型或既有 query/I/O 核。

文件 section 的八个 export 也由 FileApi 声明：CreateFileMappingA/W、OpenFileMappingA/W、MapViewOfFile/Ex、UnmapViewOfFile、FlushViewOfFile。文件契约在该批合计 53 个，PE32 32 Typed/21 ABI-only，Win64 45 Typed/8 Absent。PE32 八项均保留未实现策略；Win64 OpenFileMappingA/W 保持 absent，其余为原有 unnamed、read-only、file-backed 子集。HANDLE、view size 和可选 base 保留指针宽度，protection/access/offset high/low 使用 DWORD；SECURITY_ATTRIBUTES、access/alignment、handle 查询、错误码和故障提交顺序保持。

AreFileApisANSI 随后汇入同一契约，该批合计 54 项：PE32 32 Typed/22 ABI-only，Win64 46 Typed/8 Absent。Win64 仍返回 1，PE32 保留零参数 ABI 与未实现策略，公开 file facade 继续排除 ABI-only 项，见[标量入口补齐](../reports/refactoring-scalar-followup-2026-10-20.md)。

CreatePipe、GetConsoleMode、WriteConsoleW 与 NtQueryDirectoryFile 接入后，当前合计 58 项：PE32 32 Typed/25 ABI-only/1 Absent，Win64 50 Typed/8 Absent。管道创建归 Pipes，原生目录查询及 cursor 提交归 Files；标准 console 拒绝、BOOLEAN carrier、输出检查及 LastError 顺序保持，见[进程与 endpoint 边界](process-control.md)。

Mappings 独立拥有 section/view 与 retained file descriptor，view 持有 section 引用，文件或 section handle 关闭后已有 view 继续存活。guest WriteFile 更新同 inode 的活动 view；64 位 offset、64KiB 对齐、资源上限及原 backing 生命周期保持。adapter 按冷绑定 ID 调用原 Mappings 方法，不新增通用结果框架。独立行为与性能证据见 [section 验收](../reports/refactoring-file-section-catalog-2026-10-15.md)。

SHLWAPI/imagehlp 的 13 项与六项通用句柄契约已在 section/Interlocked 新基线上通过验收，详见 [路径与句柄边界](paths-handles.md)。标准句柄及跨资源 CloseHandle 按冷绑定 HandleApi 选择原 owner 的分支，保留保护位、查询顺序和故障提交。PE32 的混合 HandleObj 表、命名索引与分配位置现由 [KernelObjects32](kernel-objects32.md) 私有拥有。

PE32 文件窄借用迁移基线为 `9090b62`。原文件/path 处理体、文本 codec、公开 facade 及 wildcard 归一核对保持；通配算法移入 `win32/kernel/file_patterns.rs`，Win64 Find 和 NtQueryDirectory 直接复用纯算法。release 完整 Win32 248、Win64 文件 17、fmt/release lib/bin strict Clippy/build 通过。证据位于 `wire-out/refactoring-2026-10-14/r3-files32-{source-checks,verification}.json` 及 validation 日志，不重复性能或游戏矩阵。
