# 模块身份与生命周期

新 Wasm 运行时的映像装载、provider、导出图、引用与 TLS 契约见 [Wasm 运行时](wasm-runtime.md)。provider、参数宽度及 API ID 来自 `win32/catalog/modules.rs`；实际接入范围由新入口的服务白名单和根 [GOAL.md](../../GOAL.md) 维护，不继承旧执行器的支持状态。

模块以完整映像基址标识；VFS 规范路径区分不同目录的同名 DLL，basename 查询使用已装载身份。逻辑 provider 与客户映像分别拥有身份和寿命，provider 查询不产生可执行字节，也不把尚未迁移的服务伪装为可调用函数。

GetModuleHandleA/W 查询不增加引用；GetModuleHandleExA/W 支持名称、FROM_ADDRESS、UNCHANGED_REFCOUNT 和 pin。完整 HMODULE 输出写入成功后才提交引用或 pin。GetModuleFileNameA/W 按调用 ABI 返回 CP932 或 UTF-16，截断及错误按统一输出检查处理。GetProcAddress 支持名称、ordinal、数据导出和转发，失败写错误 127。

PE32 CRT 的原始数据导出由暂存运行时页提供，静态 IAT、动态查询及访问器共用可写单元。装载失败不发布这些页，FROM_ADDRESS 可查到其 provider；数据地址不能作为函数 bind。

逻辑 provider 的 ordinal 与名称使用同一服务绑定器，静态导入、转发、GetProcAddress 和延迟导入都按当前 ABI 的已迁移别名判断。未知 ordinal、错误 provider 或尚未接入的对应服务返回错误 127；诊断装载仅为缺失导入建立停止网关，不覆盖已有服务。

两种 ABI 接通 psapi 的 EnumProcessModules/Ex、GetModuleInformation、GetModuleBaseNameA/W 和 GetModuleFileNameExA/W，以及 kernel32/kernelbase 的对应 K32 名称。无前缀名称只由 psapi 提供，K32 名称只由 kernel provider 提供。查询不改变模块引用，不采样时钟，成功计一 work、零客体退休并保持 LastError。

这些接口只查询当前进程，接受伪句柄或拥有 PROCESS_QUERY_INFORMATION 与 PROCESS_VM_READ 的真实进程句柄；合法远程句柄返回错误 50。AMD64 的 GetModuleFileNameEx 在 HMODULE 为 NULL 时另接受 QUERY_LIMITED_INFORMATION，返回当前主映像路径。无效句柄和权限不足分别返回错误 6 和 5。

模块列表以主映像为首，包含当前实际映像和已登记逻辑 provider，随装卸更新。枚举容量按字节处理，只写入可容纳的完整指针，needed 始终输出完整列表所需 DWORD 字节数；零容量不访问列表指针，容量不足仍成功。Ex 的过滤值 0/3 包含当前位宽，1/2 分别选择 PE32/AMD64，其余值返回错误 87。needed 和列表在任何写入前统一预检，列表先写、needed 后写，允许输出相互重叠或覆盖已保存的返回槽。

MODULEINFO 为 PE32 的 12 字节或 AMD64 的 24 字节布局，返回完整基址、映像大小和入口指针，AMD64 padding 清零。容量不足返回错误 122；逻辑 provider 没有 PE 映像元数据，查询返回错误 50。A/W 名称查询按 CP932 字节或 UTF-16 单元计数，包含终止符；完整输出返回不含终止符的长度，截断时保留终止符并返回容量，零容量返回错误 122。字符串和结构输出只预检实际写入范围，尾部保持原值。新旧入口共用输出编码和截断规则；访存故障不提交 CPU、计数或部分输出。

RtlPcToFileHeader 在两种 ABI 接受 kernel32/kernelbase/ntdll，返回包含地址的实际客体映像基址，并通过 BaseOfImage 写出相同值；未找到时返回并写零，保持 LastError 和模块引用。逻辑 provider 的身份区和服务入口没有 PE 文件头，查询返回零。地址归属随实际映像装卸更新，输出访问失败不提交 CPU/work；保存返回帧后才写输出，允许输出与返回槽别名。

静态依赖、LoadLibraryA/W/ExA/W、转发目标和延迟导入共用同一链接器、VFS 与搜索策略。映像、IAT、服务绑定和全部活动线程的 TLS 在发布前准备；缺失输入、非法 PE、保护或输出失败均不发布新图。依赖初始化去重，TLS 回调先于 DllMain；重入加载使用保存调用上下文的独立帧。静态与动态 attach 拒绝的精确清理协议见运行时文档。

FreeLibrary 释放一次外部引用，再从主映像、pin、剩余外部引用及活动通知帧计算可达图；不再可达的共享依赖和无根环路按依赖顺序退休。通知期间保留映射，全部 TLS 槽/块和映像释放范围检查后才移除元数据。卸载同时撤销源码证明和生成模块，逻辑 provider 保持进程级寿命。

FreeLibraryAndExitThread 使用同一卸载协议，完成后进入[线程退出](thread-control.md)续点，允许当前 worker 卸载自身 DLL。已释放映像的原返回地址不会被执行，已完成的分离通知不会因后续清理故障重放。

DisableThreadLibraryCalls 保存模块级通知禁用标志，含静态 TLS 的模块返回错误 126。它只抑制线程 attach/detach，不抑制进程 detach。线程通知由[线程生命周期协议](thread-control.md)在加载锁下执行；动态装卸同步更新所有活动线程的 TLS，含挂起及尚未启动线程。

装载与通知帧有明确的线程所有者；其他线程的模块操作等待加载锁，已启动线程可以继续普通执行和 I/O。普通查询不读取平台时钟，输出故障保留保存的调用帧和引用状态。更多搜索标志、provider 可见性及延迟解析细节集中在运行时文档，不在此重复维护。

资源 API 借用映像内存和模块身份，独立契约见[资源查询](resources.md)。旧模块 owner 的适配壳仍作为未迁移服务的来源，旧 IR、原生加载执行和兼容 facade 不进入新核心。
