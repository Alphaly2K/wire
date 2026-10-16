# 虚拟内存调用边界

VirtualAlloc/Free/Protect/Query 及 Ex 八项接口由 `win32/catalog/virtual_memory.rs` 声明唯一 provider、签名和 VirtualMemoryApi ID。PE32 接受 kernel32，Win64 接受 kernel32/kernelbase。SIZE_T 使用架构位宽的参数载体：PE32 一个四字节 slot，Win64 完整八字节；DWORD flags/protect 保持 32 位。调用阶段按 ID 分发，不再解析 API 名称或 Ex 后缀。

`win32/x86/virtual_memory.rs` 仅借用 GuestMem、参数和 LastError 槽；`win32/x64/virtual_memory.rs` 仅借用 Memory、参数和当前 TEB LastError 地址。原 `vmem_api` 是私有入口，已迁入 PE32 adapter。根对象继续负责 ABI、trace、API count 和返回 frame，内存资源、保护和代码缓存失效仍由各架构内存 owner 管理。

Ex 版本先验证完整架构位宽的当前进程伪 handle，再访问其它参数。PE32 VirtualQuery 保持 28 字节结构及 error 87，Win64 保持 48 字节结构及短长度 error 24；原输出访存错误策略分别保留。VirtualProtect 继续先验证输出与目标页，再写旧权限，最后应用新权限；允许输出与目标别名，故障顺序保持。

PE32 的旧 gateway 在 handler 之后读取返回槽，因此 VirtualProtect 输出若覆盖该槽，会改变返回 EIP；Win64 gateway 先保存返回 frame，输出写入不改变 RIP。此处保留两套既有行为，后续语义修正应独立验收。成功保持原 LastError，API 失败写入原错误；Win64 checked output fault 不提交 CPU/API count。见[独立基线与验收](../reports/refactoring-virtual-memory-2026-10-16.md)。
