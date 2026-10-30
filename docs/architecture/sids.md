# SID 调用与字节布局

新 Wasm JIT 运行时在 PE32 与 AMD64 接入 advapi32 的九项 SID 接口，静态与动态查询使用同一契约；kernel32/kernelbase 的旧私有别名不进入新路径。操作只涉及调用者的客体字节，不解析宿主账户或凭据。

SID 头包含 revision、BYTE subauthority count 和六字节 identifier authority，之后是 count 个小端 DWORD。合法 SID 的 revision 为 1、count 不超过 15；长度为 `8 + 4 * count`。纯布局函数在 `src/win32/sid_values.rs`，新旧适配器与 token 编码共用，均不依赖执行器。

GetSidIdentifierAuthority、GetSidSubAuthorityCount 与 GetSidSubAuthority 返回客体字段地址，不读取 SID 或自行验证索引。subauthority 索引按 DWORD 处理，地址按 ABI 宽度回绕。GetLengthSid 只读取 count 字节；GetSidLengthRequired 消费参数低 BYTE 并计算长度，不额外限制十五项上限。

IsValidSid 先检查 NULL 和 revision，再检查 count；不读取 authority 或 subauthority 数据。无效头及不可读的普通映射返回 FALSE，保持 LastError；真实 guard 沿客体故障投递，不吞掉后重读。错误 revision 不触及下一字节，避免消耗下一页 guard。EqualSid 先按同一规则验证两个头，首个无效时不读取第二个 SID；count 相等后才比较完整字节。相同或不同均保持 LastError。

InitializeSid 消费低 BYTE count，大于 15 返回错误 87。非 NULL authority 复制六字节；NULL authority 保持目标中的六字节。成功写 revision/count，并清零全部 subauthority DWORD。输入先捕获，全部实际输出统一预检，跨页或 guard 故障不留下部分头或数组。

CopySid 从源 count 计算长度，短 DWORD 容量返回错误 122，保持目标。足够容量时复制完整字节，允许源和目标重叠；不额外调用 IsValidSid。首次源故障直接交还客体，不沿用旧适配器吞掉 guard 再重读的路径。源或目标访存失败不提交输出、返回或 work，修复后可重试。

InitializeSid 与 CopySid 的普通错误写 LastError；成功保持它。其他 SID 查询/比较保持 LastError；所有接口不采样时钟。参数和返回地址由统一调用帧捕获，输出可覆盖原返回槽。旧 Win64 内存适配器保留原有 header-only 初始化和错误/guard 政策，作为迁移回归来源，不进入新核心的编译闭包。

公开行为依据包括 Wine 的 [NTDLL SID 实现](https://github.com/wine-mirror/wine/blob/master/dlls/ntdll/sec.c)。`node crates/wire-wasm/tests/sids.mjs` 检查两种 ABI、精确访存宽度、初始化、重叠复制、guard 与保存返回槽；`node tools/probes/check_wasm_pe_entry.mjs --tokens-sids-only` 为需要 MinGW 的显式 C 编译检查，输入来自项目源码。

AllocateAndInitializeSid 另提供最多八个 subauthority 的进程堆分配，捕获全部十一参数，count 按 BYTE 消费。超限为 FALSE/1337；完整输出指针先预检，分配体复用共享 SID 编码与过程堆 owner。FreeSid(NULL) 为空操作，其他指针经同一过程堆释放，非法或外来 owner 明确拒绝；返回 NULL 并保留 LastError。

BuildTrusteeWithSidW 写 20/32 字节 TRUSTEE：空 multiple trustee、NO_MULTIPLE_TRUSTEE、TRUSTEE_IS_SID、TRUSTEE_IS_UNKNOWN 和完整 SID 指针。AMD64 的 20..24 padding 保持；预检整个输出，允许结构与 SID 参数别名，不解引用 SID。

LookupAccountSidW 只解析本地虚拟 TokenUser SID，账户/域名称来自环境 profile，SidType 为 User。NULL、空、点号或本机 profile 名称作为本地查询；其他 system 返回 50，非法 SID 返回 1337，未知 SID 返回 1332。容量以 UTF-16 单元计，短容量返回 122并写两个含 NUL 的所需长度；成功长度不含 NUL。全部字符串、长度和类型先预检，短容量时也先预检错误槽，故障不发布部分输出。没有账户数据库、ACL 或安全描述符后端。

GetEffectiveRightsFromAclW、GetNamedSecurityInfoW、SetNamedSecurityInfoW、SetEntriesInAclW 返回 DWORD 50，保持输出与 LastError；CreateProcessWithLogonW 返回 FALSE/50，保持凭据、启动记录与进程输出。稳定输入验证位于 `node crates/wire-wasm/tests/managed-runtime.mjs`。
