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

PE32 目前仍使用 ApiState 的混合 HandleObj 表与 filesystem 字段，属于后续 R3 工作。两架构 file API 元数据仍待 R2 家族迁移，本页不表示整个文件领域已完成。
