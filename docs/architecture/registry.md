# Win64 guest 注册表

`x64::runtime::registry::Registry` 私有拥有 key/value、完整 64 位句柄、访问权限与单调分配器。处理函数只借用 Memory、RegistryApi 和参数；Runtime64 负责 ABI、返回帧、计数与 trace。名称解码复用 `win32::x64::text::path_name`，旧 Runtime64 helper 保留委托供文件等现有调用使用。

注册表仅存在于当前 guest 进程，不访问宿主、不持久化、不虚构软件安装。HKCU 可写，系统根只读，WOW32 view 不可用。根身份接受既有零扩展/符号扩展形式；普通句柄完整比较，从 TEB+0x500000 每次增加 16，关闭不复用，累计 4096 次后耗尽。key/value 名称不区分大小写，A 接口按 CP932、W 接口按 UTF-16 转换；字符串值保留嵌入 NUL。

open/create 先验证句柄、选项、路径、权限与容量，再预检输出；创建时句柄和 disposition 均检查通过才登记 key。查询先读容量，再检查 type、size、足够容量的数据范围；写出顺序为 type → required size → payload，输出允许别名。LSTATUS 直接返回，线程 LastError 不变；传播的访存故障不提交 CPU 返回和 API count。

17 项 export 的 provider、参数、架构能力与 RegistryApi 集中于 `REGISTRY_CONTRACTS`，Win64 不再逐调用解析名称。15 项 Win64 静态导入接受 advapi32/kernel32/kernelbase，动态查找仅 advapi32 默认导出，其余依赖既有 gateway fallback。旧式 RegQueryValueA/W 在 Win64 仍 absent。

PE32 的 advapi32 open/query 报告 guest 条目缺失、close 接受既有七个根值；open 先写零 output 再读名称，其 LastError、错误与诊断不变。其余 create/set/delete 保持 ABI-only。kernel32 的私有 registry ABI 继续由原 Wine spec fallback 提供，保留其不可用语义；没有为 kernelbase 发明清栈签名。PE32 适配器只借 Memory、ID、参数，不创建注册表资源。

独立回归位于 `tests/x64/registry_owner.rs`、两架构 `registry_contract.rs`，既有 CP932、权限和创建故障回归位于 `tests/x64/kernel.rs`。见[所有权迁移](../reports/refactoring-registry-owner-2026-10-20.md)和[统一契约验收](../reports/refactoring-registry-contract-2026-10-20.md)。
