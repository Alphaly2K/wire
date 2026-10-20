# 环境变量与环境块

九项 export 的 provider、ABI 与执行策略统一由 `win32::catalog::environment` 声明。GetEnvironmentStrings、GetEnvironmentStringsA/W、FreeEnvironmentStringsA/W、GetEnvironmentVariableA/W、SetEnvironmentVariableA/W 在冷阶段绑定 EnvironmentApi；执行时不再按字符串选择 A/W 或 get/set。

PE32 保留六项 typed、三项 ABI-only：变量查询忽略名称/输出指针并返回零，保持原 LastError 和 note；无后缀及 W 环境块使用原 scratch allocator，每次 16 字节，不随 FreeEnvironmentStrings 退休。ANSI 后缀环境块和两个 setter 继续 error 120。scratch 分配失败后保留原先写零地址导致故障的顺序：内部错误槽为 8，TEB/返回帧/API count 尚未提交。gateway 负责原 allocator 调用，`win32::x86::environment` 只借内存和已分配地址输出。

Win64 的 `runtime::environment::Environment` 私有拥有可变变量模型与已发出的环境块集合，组合根只保留一个 owner 字段。实际变量模型仍复用公共 `runtime::environment::EnvironmentVariables`，保持 Unicode 小写键、原名称/value 和有序 wide block；虚拟 GuestEnvironment profile 独立不变，不继承 host 环境。

Win64 六项 typed；无后缀/ANSI block 和 FreeEnvironmentStringsA 保持 absent。变量名校验、ANSI codepage、DWORD 容量、长度查询不写输出、错误 87/203、访存故障和返回帧顺序保持。wide block 通过原 page allocator 分配，与普通 guest heap 分开；写入后才登记 block，Free 先核对 owner 和 allocation、成功 unmap 后才删除记录。普通 HeapFree 拒绝环境块，失败后环境块仍能由专用接口释放。allocator/mapping 仍由运行时服务拥有，本批没有统一两类堆。

Environment 现直接执行变量取值/修改、wide block 输出和专用释放，不借 Runtime64。变量调用只借 Memory、codepage 标量与只读 Threads，保持只在错误分支查询当前 TEB；块创建借既有 Pages，释放只借原 allocation 账本与 Threads。Runtime 的协调入口按操作组合这些资源，原分配、写入后登记和 unmap 后退休顺序保持，未引入第二份分配状态。

四项新增独立测试及既有 Win64 环境测试固定旧行为。原创 `gateway-environment` 在开头只创建两个 scratch block，循环覆盖查询、释放及 ABI-only 项，避免工作量增长时反复分配环境块。验证与性能状态见[迁移记录](../reports/refactoring-environment-2026-10-19.md)。
