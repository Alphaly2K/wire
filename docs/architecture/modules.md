# 模块身份与生命周期

PE32 的 `win32::x86::modules::Modules` 拥有 guest images、路径、引用计数、pin、TLS 和初始化/退休顺序；内建逻辑 provider 的名字、handle 反查及 numeric allocator 也由它拥有。`modules/providers.rs` 私有实现保留初值 `0x70001000`、stride `0x10000`、ASCII 小写注册与进程级持久寿命。查找只消费既有 lowercase key，不能顺便注册模块。根 `modules::Modules` 保留公开重导出。

`ApiState::module_handle_for` 保留并委托 owner。原 `module_handles/handle_to_dll/next_module_handle` 三个公开可写字段已移除，属于 Rust 源接口变化；仓库中除运行时外只有一个测试直接读取它们，改用独立的 legacy handle 黄金值。外部调用者使用 `state.modules.provider_handle_for/provider_handle/provider_name`，不再写两张表或改 allocator。公开 root 模块路径保持兼容。

Win64 的 `win32::x64::modules::Modules` 私有持有 handle/name/ref、pin、exports、guest path、image size/entry、DLL directory 与 next handle。Runtime64 的公开注册入口仍委托该 owner；metadata 查询只借用只读 name/export/path/image view。所有表的创建、retain/release、pin 和 provider 分配通过方法执行，没有公开字段或 Deref 到总运行时。

各架构原有差异继续保留：

- PE32 支持 guest DLL 的链接/初始化/卸载、forwarder 和 static TLS；provider 不随 FreeLibrary 退休。guest callback 和线程通知仍由 Process 经兼容 facade 执行，搬状态时没有重排这些动作。
- Win64 继续按有序 handle 枚举；guest DLL 的最终 detach/unload 仍拒绝，保留过量引用的释放和 pinned image 存活。kernel32 与显式 pin 的 provider 保持原有特殊寿命。原始 guest Mono 按普通 DLL 装载。
- Win64 basename、canonical system32 provider、guest path 与 DLL 搜索仍使用原规则：exe/altered directory、SetDllDirectory、system32、部分 Windows/cwd；PATH/AddDllDirectory 尚不支持。None 与空 DLL directory 的区别保留。
- `GetModuleHandleEx` 的受检输出和引用提交由架构 adapter 安排。Win64 先检查输出和 flags，解析 handle，再准备 ReferenceUpdate；guest 写入成功后才更新 ref/pin。FreeLibrary 的 guest/provider outcome 保留原来的 CPU/trace 提交路径。
- resource/USER 只读查询 image metadata，经 owner view 读取；PE 地址/结构范围仍由原有 checked codec 验证。

PE32 的 TLS 回调、线程通知和进程退出通知由 `x86/process/module_notifications.rs` 执行，公开 modules 通知入口保留重导出。Modules 私有拥有 image/phase/order，提供原快照、当前 image 存在性和通知资格；Process 不访问其内部表。TLS 列表先取快照、每次执行前重查 image，TLS 完成后才重查 DllMain entry；线程通知按原顺序逐个重查，进程 detach 标志在取退出顺序快照前提交。直接 call_guest 的原恢复策略不变。

通知边界基线 `6e13fe9`，五个执行主体及八个 metadata 操作对照保持，link/resolve/initialize/unload/rollback/call 主体未改；完整 release Win32 259、IR 31、fmt/release lib/bin strict Clippy/build 通过。不跑性能或游戏矩阵，证据 `wire-out/refactoring-2026-10-14/r4-module-notifications32-{source-checks,verification}.json`。

初始化和卸载执行由 `x86/process/module_lifecycle.rs` 拥有。Modules 在原位置标记 Initializing 并返回 dependencies/entry/path 快照；Process 处理可选 sidecar IR、依赖初始化、TLS 与 DllMain，然后 owner 提交 Ready/transaction/init order。卸载由 owner 处理 provider/pin/ref/phase，再由 Process 执行 TLS/DllMain，owner 依次移除 image、释放 TLS、移除路径/order 并 decommit；正常完成后 Process 删除对应 IR 并递归释放依赖。FALSE attach 的 detach 记录和控制转移时保留映射的策略保持。

生命周期基线 `db3c4fe`，初始化/卸载与四个准备/提交方法对照保持；link/resolve/rollback/call 和通知主体未改。完整 release Win32 259、IR 31、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵，证据 `wire-out/refactoring-2026-10-14/r4-module-lifecycle32-{source-checks,verification}.json`。

typed 模块入口及失败清理由 `x86/process/module_loading.rs` 协调，API 直接调用；公开 modules::dispatch 重导出兼容入口。Modules 独立执行 disable-thread、pin、metadata 查询和回滚资源操作，Process 保留原 expected/fatal/control 分支和 TLS/DllMain 失败清理。回滚仍先撤销边/引用，再逆序逐个退休 image，每次成功 decommit 后立即删除 CPU IR；后续失败不延迟已完成的失效动作。领域模块不再依赖 Cpu、scheduler、logger 或 guest 执行器，链接器对组合根的借用继续收窄。

领域 Failure 现只表达 Expected/Fatal，不再携带 GuestCallOutcome。`x86/process/module_error.rs` 拥有含 Control 的执行错误；从领域错误显式转换时原样移交 code/message，不改 callback 或清理次序。预期错误触发回滚、fatal 传播与异常/退出控制转移仍由原 Process 分支处理。

装载入口基线 `1e6eacc`，call/dispatch、回滚与领域操作对照保持，链接/生命周期方法未改；阶段整合 release 全目标 694 passed / 7 ignored（21 suites）、all-targets/all-features strict Clippy、build 和 fmt 通过。证据 `wire-out/refactoring-2026-10-14/r4-module-loading32-{source-checks,verification}.json`。

`win32/x86/modules/linker.rs` 的 Linker 只在映射/解析期间借 Modules、ApiTable、USER、scratch、错误槽、只读 filesystem/EXE path 和两个 cmdline 地址，回调前借用即结束。模块领域不再借 ApiState。依赖引用、去重与 abandon 由 Modules 方法处理；系统类注册直接借 USER 的明确资源。原查找/读取/映射/IAT/保护和 provider/data-export/forwarder 顺序保持，不增加逐调用解析。

R7 在职责分离后将 PE32 image/TLS owner 与链接器归入 `win32/x86/modules`；根模块只重导出原 Modules/native_module、Process 的公开装载/通知函数和内部类型。领域目录不导入 Process；错误转换与 guest 执行继续位于 Process。没有移动 fixture、第三方输入或构建路径。

链接资源基线 `8707f16`，link/resolve、引用/边/abandon、系统类与 Process 调用对照保持，生命周期/回滚/TLS 方法未改。首次编译遗漏 Desktop 使用的兼容类入口，保留入口后完整 release Win32 259、IR 31、all-targets/all-features strict Clippy、build/fmt 通过，原失败日志保留。不跑性能或游戏矩阵，证据 `wire-out/refactoring-2026-10-14/r3-module-linker32-{source-checks,verification}.json`。

模块 API 的 31 个 export 由 `win32::catalog::modules` 维护唯一 provider/ABI/执行声明：13 个核心装载、查询与引用 API，4 个 DLL directory API，以及 7 个 PSAPI 查询及其 7 个 K32 名称。PE32 核心 13 个 typed，directory/K32 的 11 个保留 Wine ABI-only 策略，7 个无前缀查询仍 absent；Win64 使用全部既有子集。未知 ABI、stub 地址、序数和 data export 行为保持。

PE32 映射 image 的十项资源 export 单独归入 [ResourceApi](resources.md)，不计入上述 31 项。它们借用 image 内存和 EXE 基址，不改变模块引用或资源寿命。

静态导入和动态查找是独立的可见性维度。Contract 的 `dlls` 决定 import binding，`lookup_dlls` 决定 Win64 GetProcAddress 的 fresh-provider 可见性，默认相同，PSAPI 显式覆盖：原 Win64 静态调用接受 kernel32/kernelbase/psapi 的有前缀和无前缀查询；尚未绑定对应 gateway 的 provider 动态查找仅接受 K32 的 kernel32/kernelbase 导出，其余查询返回 0/LastError 127。如果 provider/export 已通过静态导入或公开注册入口绑定 gateway，原 registry fallback 仍返回该地址；PSAPI 和 kernel 无前缀名称均保留这一行为。PE32 动态绑定继续使用原 Wine/native-export 解析及其 ABI-only/unknown 策略。

主路径在装载/lookup 阶段绑定 typed ID。PE32 `modules::dispatch` 保留为公开兼容 facade，根 gateway 直接调用 enum 分发，不逐调用查 catalog 或字符串。Win64 scalar query adapter 只操作 module 所属视图及明确 filesystem 借用；LoadLibrary/FreeLibrary/GetProcAddress 的 ControlImport 保存 ID，原 LoadModule、早返回、unblock、frame、API count 和 trace 顺序保持。query 的 K32/PSAPI 名称绑定到同一行为 ID，调用时不再剥前缀或判断字符串。

Win64 文件系统现由 [Files](files.md) 拥有，模块 adapter 显式借用；PE32 文件状态仍待迁移。没有建立万能 Context 或统一两架构 module arena。

验收与性能拒绝/修复记录见 [实施报告](../reports/refactoring-modules-2026-10-14.md)。
