# 安全随机数

新 Wasm 入口在 PE32/AMD64 提供 bcrypt.dll 的 BCryptOpenAlgorithmProvider、BCryptGenRandom 和 BCryptCloseAlgorithmProvider，返回 NTSTATUS，保持 LastError。仅支持 RNG 与可选的 Microsoft Primitive Provider；其他算法或实现返回 STATUS_NOT_SUPPORTED。旧 kernel provider 别名不对新入口开放。

每个进程复用共享的 RNG provider 状态，最多同时打开 1024 个句柄，关闭后可复用。完整句柄身份参与验证；支持伪句柄 0x81，以及空句柄配合 BCRYPT_USE_SYSTEM_PREFERRED_RNG。flags 和长度按 DWORD 处理，单次长度上限 16 MiB。零长度调用不访问输出，也不请求随机源。

随机数据通过 Web 入口的 `random(length)` 提供，默认使用 Web Crypto，每次采样不超过 65536 字节。提供者必须同步返回精确长度的 Uint8Array 或 `null`；`null` 返回 STATUS_UNSUCCESSFUL，抛错或非法返回值保留调用供重试。核心在采样前观察参数、返回帧与完整输出权限，guard 不被预检消耗；实际调用通过统一 VMM 提交输出，故障不发布部分数据或 CPU 返回。

open 先读取算法与实现名，再验证 flags、名称、输出及容量，输出成功后登记句柄。close 只移除本进程的有效句柄。进程退出与启动拒绝会清理 provider；随机字节不跨调用保留。统一服务调用帧允许输出覆盖已保存的返回槽。

底层采样协议和 `wire_service_random_bytes` 见[Wasm 运行时](wasm-runtime.md)。旧原生入口复用相同的 provider 状态与校验，熵源仍由其宿主提供。

advapi32 的 SystemFunction036（公开名称 RtlGenRandom）复用上述熵源与完整输出预检，参数为 buffer 和 DWORD length，不需要 provider 句柄。返回 BOOLEAN，成功 1、失败 0，保持 LastError；零长度不访问 buffer 或采样并成功，超出 16 MiB 或熵源不可用返回 FALSE 并保持输出。提供者抛错、非法返回值和访存故障沿同一可重试边界处理。该入口仅从 advapi32 绑定，不增加客体执行循环中的宿主调用。
