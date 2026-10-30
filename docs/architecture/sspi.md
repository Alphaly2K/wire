# SSPI 服务发现

34 项入口的 ABI、参数类型与 SspiApi 由 `SSPI_CONTRACTS` 统一声明。Wasm 入口接受两种 ABI 的 secur32 provider；不开放 kernel32/kernelbase 或 sspicli 别名。

`win32/sspi_state.rs` 在新旧入口间共享 A/W 函数表缓存、槽位编码和空安全包策略。函数表为 28 个指针宽度，version 1，保留槽为 NULL；函数指针指向同一 Wasm JIT 的服务网关。表存放在进程独立只读页中，不属于公共 guest heap，不受 module refcount 或 FreeContextBuffer 释放；启动拒绝回收 backing。

首次创建先在临时绑定表注册全部槽位，分配、写入并封为只读后才发布绑定与表地址。失败不会发布部分函数表。

当前没有 Schannel/NTLM/Kerberos 安全包或 credential/context handle。枚举先预检 count/pointer，再分别写零；查找/获取/导入按原位置和 CP932/UTF-16 读取包名，返回 PACKAGE_NOT_FOUND，空包指针返回 INVALID_PARAMETER。NULL FreeContextBuffer 成功，其余未签发对象操作返回 INVALID_HANDLE；保留输出及 LastError。没有新增认证、签名或 TLS 成功路径。
