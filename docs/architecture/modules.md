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

本批完成状态所有权，模块 API 的签名/provider/能力声明仍使用当前 legacy 路由，后续 R2 再按家族迁入 catalog。文件系统 owner 也仍是后续工作；没有建立万能 Context 或统一两架构 module arena。

验收与性能拒绝/修复记录见 [实施报告](../reports/refactoring-modules-2026-10-14.md)。
