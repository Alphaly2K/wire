# Win64 SSPI 服务发现

34 项 provider、ABI、参数类型与 SspiApi 由 `SSPI_CONTRACTS` 统一声明。静态导入接受 secur32/kernel32/kernelbase，动态导出默认只在 secur32 可见，其余保持已注册 gateway fallback。PE32 仍为 absent/未知 ABI，未扩展到 sspicli。

私有 `x64::runtime::sspi::Security` 拥有 A/W 两个函数表地址。调用只借用 Memory、绑定 ID、参数、分配游标与 Gateways；不访问整个 Runtime64。函数表使用原 224-byte 布局、version 1、既有空槽和 secur32 gateway，放在独立只读页中，不属于 guest heap，不受 module refcount 或 FreeContextBuffer 释放。首次创建保持 map → 注册槽位 → write → protect → 更新分配游标/缓存的顺序。

网关的冷阶段注册逻辑移至 Gateways，Runtime64::register 保留委托。名称规范化、已有 export 去重、地址分配、绑定和范围更新均沿用旧算法；热查找与调用不变。

当前没有 Schannel/NTLM/Kerberos 安全包或 credential/context handle。枚举先预检 count/pointer，再分别写零；查找/获取/导入按原位置和 CP932/UTF-16 读取包名，返回 PACKAGE_NOT_FOUND，空包指针返回 INVALID_PARAMETER。NULL FreeContextBuffer 成功，其余未签发对象操作返回 INVALID_HANDLE；保留输出及 LastError。没有新增认证、签名或 TLS 成功路径。

独立 provider 基线在 `tests/x64/sspi_contract.rs`；函数表稳定、只读页、heap 身份、错误输出、12 参数 ABI 和真实 guest 间接调用继续由 `tests/x64/network.rs` 验证。见[验收](../reports/refactoring-sspi-2026-10-20.md)。
