# Token 与虚拟线程身份

新 Wasm JIT 运行时在 PE32 与 AMD64 接入 advapi32 的 OpenProcessToken、OpenThreadToken 和 GetTokenInformation，支持静态导入和动态查询。kernel32/kernelbase 的旧私有别名不进入新路径。token 描述进程内的虚拟普通用户，不读取宿主凭据，不提供宿主提权；线程模拟限定为相同虚拟用户的身份级别。

OpenProcessToken 接受当前进程伪句柄及拥有 PROCESS_QUERY_INFORMATION 或 PROCESS_QUERY_LIMITED_INFORMATION 的真实进程句柄。支持零权限和 TOKEN_QUERY（8）、TOKEN_DUPLICATE（2）、TOKEN_IMPERSONATE（4）的组合；其他请求返回错误 50。成功按 ABI 指针宽度写入句柄后才发布资源。权限与容量检查先于输出，输出或保存返回帧故障不消耗身份。无查询权限的 token 在 GetTokenInformation 返回错误 5。

每个进程最多 128 个活动 token 和 4096 个累计句柄身份，关闭释放活动容量且不复用编号。PE32 句柄从 `0xa0000000` 开始，AMD64 从 `0x7ffd05000000` 开始，均按完整宽度匹配。所有线程和 DLL 共用状态，其他 VM 隔离；进程退出及静态启动拒绝清理活动 token。

GetTokenInformation 支持以下固定虚拟身份记录，内部指针指向调用者缓冲区，不指向宿主或核心内存。padding 为零，所有整数为 Windows 小端序。

| 信息类 | PE32 / AMD64 字节数 | 内容 |
| --- | --- | --- |
| TokenUser（1） | 36 / 44 | SID_AND_ATTRIBUTES 后接 S-1-5-21-1-1-1-1000，属性 0；SID 指针分别为输出基址 +8 / +16。 |
| TokenType（8） | 4 / 4 | 主 token 为 TokenPrimary（1），复制/线程 token 为 TokenImpersonation（2）。 |
| TokenImpersonationLevel（9） | 4 / 4 | 模拟级别 0..3；主 token 返回错误 87。 |
| TokenElevationType（18） | 4 / 4 | TokenElevationTypeDefault（1），没有拆分的提升令牌。 |
| TokenElevation（20） | 4 / 4 | TokenIsElevated 为 0。 |
| TokenIntegrityLevel（25） | 20 / 28 | SID_AND_ATTRIBUTES 后接 S-1-16-8192，属性 0x60。 |
| TokenIsAppContainer（29） | 4 / 4 | 0。 |

其他信息类返回错误 50。查询先验证完整句柄和 TOKEN_QUERY，再构造记录并检查 ReturnLength；短容量返回错误 122，写所需字节数并保持数据缓冲区。足够容量时先检查全部实际输出，再写数据、最后写 ReturnLength。短容量分支同时预检错误槽，错误槽故障也不提前改写 ReturnLength。输出允许相互别名或覆盖已保存返回槽；指针溢出返回错误 87。

CloseHandle、Get/SetHandleInformation 和 DuplicateHandle 已接入同一 token owner。支持继承/防关闭标志、相同或缩减权限的复制及 DUPLICATE_CLOSE_SOURCE；不支持扩权。防关闭 token 返回错误 6，解除保护后才可关闭。复制先验证进程的 DUP_HANDLE 权限、输出及资源容量，再发布别名并按需关闭源。

DuplicateToken 要求源 TOKEN_DUPLICATE，级别为 0..3；输出成功后才发布具有 TOKEN_QUERY|TOKEN_IMPERSONATE 的独立 token。非法级别返回 87，复制不足权限返回 5。DuplicateHandle 保留类型与模拟级别。

ImpersonateLoggedOnUser 对主 token 要求 TOKEN_QUERY|TOKEN_DUPLICATE，对模拟 token 要求 TOKEN_QUERY|TOKEN_IMPERSONATE 且级别至少为 SecurityImpersonation（2），低级别返回 1346。成功只修改当前虚拟线程；关闭源 token 后线程身份仍有效。新线程从无模拟状态开始，线程退出清理，RevertToSelf 恢复当前线程，未模拟时调用也成功。没有宿主凭据切换或 ACL 后端。

OpenThreadToken 对拥有 THREAD_QUERY_INFORMATION 或 THREAD_QUERY_LIMITED_INFORMATION 的目标线程查询模拟状态，无 token 返回 ERROR_NO_TOKEN（1008），无权限返回 5，无效句柄返回 6。有模拟时在输出成功后发布对应级别的新 token。固定单用户环境没有 OpenAsSelf 的身份差异。失败不读取或改写输出句柄。

成功调用保持 LastError；普通错误返回 FALSE 并写当前线程错误槽。首帧、输入、输出或 guard 故障保持 CPU 和 work，修复映射后可以重试。所有接口不采样时钟。逻辑状态及双 ABI 编码位于 `src/win32/token_state.rs`，新适配器位于 `crates/wire-wasm/src/win32/tokens.rs`。修改 token、更多信息类和其他用户身份尚未接通。

`node crates/wire-wasm/tests/tokens.mjs` 验证布局、权限、复制/关闭、容量和故障事务。显式 `node tools/probes/check_wasm_pe_entry.mjs --tokens-sids-only` 需要 MinGW 两种交叉编译器及支持 Wasm 的 Node，验证项目 C 程序的两种 ABI、O0/O2 和 worker 共享，不依赖游戏文件。

`node crates/wire-wasm/tests/managed-runtime.mjs` 另检查复制类型/级别、权限、线程隔离、关闭后模拟保持和输出故障不消耗身份。
