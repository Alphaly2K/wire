# 模块身份与生命周期

PE32 的 `modules::Modules` 继续拥有 guest images、路径、引用计数、pin、TLS 和初始化/退休顺序；内建逻辑 provider 的名字、handle 反查及 numeric allocator 现在也由它拥有。`win32::x86::modules::Providers` 私有实现保留初值 `0x70001000`、stride `0x10000`、ASCII 小写注册与进程级持久寿命。查找只消费既有 lowercase key，不能顺便注册模块。

`ApiState::module_handle_for` 保留并委托 owner。原 `module_handles/handle_to_dll/next_module_handle` 三个公开可写字段已移除，属于 Rust 源接口变化；仓库中除运行时外只有一个测试直接读取它们，改用独立的 legacy handle 黄金值。外部调用者使用 `state.modules.provider_handle_for/provider_handle/provider_name`，不再写两张表或改 allocator。公开 root 模块路径保持兼容。

Win64 的 `win32::x64::modules::Modules` 私有持有 handle/name/ref、pin、exports、guest path、image size/entry、DLL directory 与 next handle。Runtime64 的公开注册入口仍委托该 owner；metadata 查询只借用只读 name/export/path/image view。所有表的创建、retain/release、pin 和 provider 分配通过方法执行，没有公开字段或 Deref 到总运行时。

各架构原有差异继续保留：

- PE32 支持 guest DLL 的链接/初始化/卸载、forwarder 和 static TLS；provider 不随 FreeLibrary 退休。guest callback 和线程通知仍由 Process 经兼容 facade 执行，搬状态时没有重排这些动作。
- Win64 继续按有序 handle 枚举；guest DLL 的最终 detach/unload 仍拒绝，保留过量引用的释放和 pinned image 存活。kernel32 与显式 pin 的 provider 保持原有特殊寿命。原始 guest Mono 按普通 DLL 装载。
- Win64 basename、canonical system32 provider、guest path 与 DLL 搜索仍使用原规则：exe/altered directory、SetDllDirectory、system32、部分 Windows/cwd；PATH/AddDllDirectory 尚不支持。None 与空 DLL directory 的区别保留。
- `GetModuleHandleEx` 的受检输出和引用提交由架构 adapter 安排。Win64 先检查输出和 flags，解析 handle，再准备 ReferenceUpdate；guest 写入成功后才更新 ref/pin。FreeLibrary 的 guest/provider outcome 保留原来的 CPU/trace 提交路径。
- resource/USER 只读查询 image metadata，经 owner view 读取；PE 地址/结构范围仍由原有 checked codec 验证。

模块 API 的 31 个 export 由 `win32::catalog::modules` 维护唯一 provider/ABI/执行声明：13 个核心装载、查询与引用 API，4 个 DLL directory API，以及 7 个 PSAPI 查询及其 7 个 K32 名称。PE32 核心 13 个 typed，directory/K32 的 11 个保留 Wine ABI-only 策略，7 个无前缀查询仍 absent；Win64 使用全部既有子集。未知 ABI、stub 地址、序数和 data export 行为保持。

PE32 映射 image 的十项资源 export 单独归入 [ResourceApi](resources.md)，不计入上述 31 项。它们借用 image 内存和 EXE 基址，不改变模块引用或资源寿命。

静态导入和动态查找是独立的可见性维度。Contract 的 `dlls` 决定 import binding，`lookup_dlls` 决定 Win64 GetProcAddress 的 fresh-provider 可见性，默认相同，PSAPI 显式覆盖：原 Win64 静态调用接受 kernel32/kernelbase/psapi 的有前缀和无前缀查询；尚未绑定对应 gateway 的 provider 动态查找仅接受 K32 的 kernel32/kernelbase 导出，其余查询返回 0/LastError 127。如果 provider/export 已通过静态导入或公开注册入口绑定 gateway，原 registry fallback 仍返回该地址；PSAPI 和 kernel 无前缀名称均保留这一行为。PE32 动态绑定继续使用原 Wine/native-export 解析及其 ABI-only/unknown 策略。

主路径在装载/lookup 阶段绑定 typed ID。PE32 `modules::dispatch` 保留为公开兼容 facade，根 gateway 直接调用 enum 分发，不逐调用查 catalog 或字符串。Win64 scalar query adapter 只操作 module 所属视图及明确 filesystem 借用；LoadLibrary/FreeLibrary/GetProcAddress 的 ControlImport 保存 ID，原 LoadModule、早返回、unblock、frame、API count 和 trace 顺序保持。query 的 K32/PSAPI 名称绑定到同一行为 ID，调用时不再剥前缀或判断字符串。

Win64 文件系统现由 [Files](files.md) 拥有，模块 adapter 显式借用；PE32 文件状态仍待迁移。没有建立万能 Context 或统一两架构 module arena。

验收与性能拒绝/修复记录见 [实施报告](../reports/refactoring-modules-2026-10-14.md)。
