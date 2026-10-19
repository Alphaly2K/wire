# Win64 BCrypt RNG

`x64::runtime::bcrypt::RngProviders` 私有拥有 RNG 句柄集合，处理函数只借 Memory、BcryptApi 和参数。三项 export 的 provider/ABI/类型集中于 `BCRYPT_CONTRACTS`；静态导入接受 bcrypt/kernel32/kernelbase，动态查找沿用已有 gateway fallback。PE32 保持 absent/未知 ABI。

当前仅支持 RNG 和可选的 Microsoft Primitive Provider。最多 1024 个同时打开的 provider，句柄从 TEB+0x700000 起间隔 16，关闭后重新使用最低空闲值；比较完整 64 位身份。伪句柄 0x81 和 system-preferred flag 保持原行为，flags/length 仍截断 DWORD。返回 NTSTATUS，LastError 保持。

生成随机数先校验 flags、handle、16MiB 长度上限，再预检输出范围，随后以 256-byte 块调用原 OS entropy 并一次写回；失败不发布部分随机数据。open 先读算法和 implementation 名称，再验证 flags/名称与输出，最后检查容量并登记。输出可以覆盖保存过的 guest 返回地址；访存故障不提交 CPU 返回或 API count。

独立 provider/容量/句柄回归位于 `tests/x64/bcrypt_contract.rs`，原 OS entropy/边界测试位于 `tests/x64/kernel.rs`。本次不扩展密码学能力；见[迁移验收](../reports/refactoring-bcrypt-2026-10-20.md)。
