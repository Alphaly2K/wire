# Windows version 边界

本页记录 OS profile 查询；文件中的版本资源使用独立 [version.dll 边界](version-resources.md)。

新 Wasm 入口在两种 ABI 接入八项版本服务，使用不可变 GuestEnvironment。层级比较复用 `src/win32/kernel/version.rs`，不依赖 CPU 或客体内存；原 Wine LGPL 来源和版权声明保留。

| 行为 | PE32 | Win64 |
| --- | --- | --- |
| 默认 profile | 6.1.7601 / Service Pack 1 | 10.0.19045 / 无 Service Pack |
| GetVersion/Ex、VerifyVersionInfoA/W | kernel32/kernelbase | kernel32/kernelbase |
| RtlGetVersion/RtlVerifyVersionInfo | ntdll | kernel32/kernelbase/ntdll（历史范围） |
| VerSetConditionMask | kernel32/kernelbase | kernel32/kernelbase/ntdll（历史范围） |
| 查询结构大小 | A：148/156；W/Rtl：276/284 | A：148/156；W/Rtl：276/284 |
| Rtl 查询非法 size | INVALID_PARAMETER，保持 LastError | INVALID_PARAMETER，保持 LastError |
| 参数与返回 | ConditionMask 完整 64 位栈参数；mask 返回 EDX:EAX | ConditionMask 与 mask 返回均保持 64 位 |

查询输出包含 profile 的 major/minor/build/platform；扩展结构另含 Service Pack、suite 和 workstation 产品类型，PE32 的 CSD 字符串为 Service Pack 1。GetVersionEx 的非法大小返回零/87，RtlGetVersion 返回 STATUS_INVALID_PARAMETER 并保持 LastError。完整输出写入前预检，返回槽可与输出重叠；输出之外的字节保持不变。

VerifyVersionInfoA 先读取完整 156 字节，W/Rtl 读取 284 字节，再校验结构大小、TypeMask 和 ConditionMask。产品类型、suite、platform/build 按既有顺序比较，major/minor/service-pack 使用层级关系；更高的 major 可满足 `>=`，不再独立要求 minor 同时更高。普通 BOOL 校验失败分别映射 160（参数无效）和 1150（版本不满足）；RtlVerifyVersionInfo 直接返回 NTSTATUS，保持 LastError。

VerSetConditionMask 保留传入 mask 并加入所选字段的条件低三位，保持完整 64 位结果。全部调用不读取宿主 OS 或平台时间；访存故障不提交 CPU/work，成功完成计一 work、不计客体退休。共享目录中旧 PE32 ABI-only 与 Win64 absent 策略仅供旧入口使用。
