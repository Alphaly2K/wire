# Windows version 边界

本页记录 OS profile 查询；文件中的版本资源使用独立 [version.dll 边界](version-resources.md)。

唯一声明位于 `win32/catalog/version.rs`，冷绑定派生 PE32 栈字节数、Win64 arity/DWORD 截断与 typed ID。适配器借用架构内存、不可变 GuestEnvironment 和 LastError 位置；Win64 层级比较算法位于 `win32/kernel/version.rs`，不依赖 Runtime64 或 guest 内存。原 Wine LGPL 来源与版权声明保留。

| 行为 | PE32 | Win64 |
| --- | --- | --- |
| 默认 profile | 6.1.7601 | 10.0.19045 |
| GetVersion/Ex provider | kernel32 | kernel32/kernelbase |
| RtlGetVersion provider | ntdll | kernel32/kernelbase/ntdll（保留历史范围） |
| 查询结构大小 | A/W/Rtl 均接受 148/156/276/284 | A 接受 148/156；W/Rtl 接受 276/284 |
| Rtl 查询非法 size | 返回 0，LastError=87 | INVALID_PARAMETER，LastError 保留 |
| VerifyVersionInfoA/W、VerSetConditionMask | known stdcall ABI=16，未实现返回 0/LastError=120 | W/mask typed；A 拒绝 |
| RtlVerifyVersionInfo | ABI unknown，零清栈 legacy | 三个 kernel/ntdll provider，NTSTATUS |
| output alias return slot | 输出后读取返回地址 | handler 前保存返回 frame |

Win64 verify 先受检读取完整 284 字节，再检查 size/mask/conditions；访存 fault 不提交 CPU/API count。产品类型、suite、platform/build、major/minor/service-pack 的比较顺序保留，BOOL 错误分别映射 160/1150，NTSTATUS 直接返回。条件 mask 保留已有位并消费条件参数低三位；不额外截断旧 trace 的 BYTE carrier。

`tests/version_contract.rs` 是迁移前固定的独立基线；现有 Win64 集成测试继续验证层级比较、GetProcAddress、LastError 与跨页 output fault。性能工具显式选择 `gateway-version`，两种架构从各自固定 profile 的数值计算 checksum；默认 fixture 集合保持原样。
