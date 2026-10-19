# SID 调用与字节布局边界

九项 `SID_CONTRACTS` 统一 provider、参数、返回约定与 SidApi。PE32 仅 advapi32 有已知 stdcall ABI，保持零返回、error 120、输出不变；其它 DLL 保持未知 ABI/零清栈。Win64 静态导入接受 advapi32/kernel32/kernelbase，动态查询仅发现已注册 gateway，不增加 fresh-provider 导出。

`win32::x64::sid` 只借 Memory、typed ID、参数与当前线程 LastError guest 地址，拥有 SID 的受检读取和调用逻辑，不再依赖 Runtime64 或 token owner。SID 读取 codec 除函数名/可见性及函数间空行外保持；token 的资源、权限和信息编码不变。revision 1、count 不超过 15，指针保持完整 64 位；subauthority index 和 copy capacity 在 gateway 截断为 DWORD。两个 BYTE count 保留 Win64 原始 trace carrier，由 adapter 消费低八位。

GetSidLengthRequired 对 BYTE 计算 `8+4*count`，原本不验证十五个 subauthority 上限；InitializeSid 才检查上限并返回 87。初始化先预检完整目标范围，再检查/读取六字节 authority，只写八字节 header，原 subauthority 数据保持。输出可与返回槽别名，gateway 使用事先保存的返回帧。

CopySid 保留原两次 SID 读取：可选 preflight 读取有效且容量足够时，目标故障向外传播；随后逻辑读取和写入中的 SID/访存错误映射为 1337。源 guard 可能在第一次读取时被消费，之后第二次读取成功；源与目标同时带 guard 时，目标写故障映射为 1337，并提交零返回与 API count。这一行为由独立旧实现测试固定，抽取时没有合并检查。短容量返回 122；subauthority 越界为 87；成功保留 LastError。

独立基线固定九项 provider/arity、静态/动态差异、BYTE trace、DWORD index、完整高位指针、header-only 初始化及 guard/error/fault 提交。按低风险结构迁移执行定向正确性、契约一致性与构建检查，不运行性能或无关游戏/native 矩阵；见[验收记录](../reports/refactoring-sid-contract-2026-10-20.md)。
