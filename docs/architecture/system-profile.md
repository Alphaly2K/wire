# 系统信息与内存状态调用边界

GetSystemInfo、GetNativeSystemInfo、GlobalMemoryStatus、GlobalMemoryStatusEx 四项 export 在 `win32/catalog/system_profile.rs` 唯一声明 provider、签名、返回约定与三个 SystemProfileApi ID。GetSystemInfo 与 GetNativeSystemInfo 共用原本相同的 body，原 export/trace 身份保留。PE32 四项 typed、kernel32；Win64 三项 typed、kernel32/kernelbase，GlobalMemoryStatus 维持 absent。

PE32 适配器只借 GuestMem 和 LastError 槽。保留 36-byte SYSTEM_INFO、32-byte MEMORYSTATUS、64-byte MEMORYSTATUSEX 与原 x86 虚拟硬件取值；内存状态继续从 application committed/reserved 计算，不改 allocator 或内存 owner。Ex 先核对 length 64，失败保持输出；访存错误映射 error 87，成功保留 LastError。原输出覆盖返回槽时，根适配器仍在 copy-out 后读取返回地址。

Win64 适配器只借 Memory 与 active TEB 的 LastError 地址。保留 48-byte SYSTEM_INFO、AMD64 地址范围与单虚拟 CPU；Ex 继续由 Memory::memory_status 计算。公共 gateway 在 handler 前保存返回帧，受检访存失败不提交 CPU/API count；length 错误写 error 87 后正常返回。各架构的内存模型与虚拟硬件值分别保持。

`wire::system::dispatch` 是既有 public facade，继续存在并保留 DLL 精确大小写匹配。它从该家族契约冷解析 ID，再委托 PE32 adapter；正常已绑定执行直接调用 typed adapter。GetSystemTime/GetLocalTime 随[日历家族](calendar.md)迁移，winmm period/caps 随[多媒体计时器](multimedia.md)迁移。迁移后的名字只出现在 catalog 声明，根分发器和 ABI/arity 表不再重复维护这四项。

独立旧行为基线、GetSystemInfo 输出校验循环及验收结果见[系统信息报告](../reports/refactoring-system-profile-2026-10-16.md)。
