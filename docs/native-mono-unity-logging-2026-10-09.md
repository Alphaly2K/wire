# 原生 Mono 的 Unity 日志、异常注册和启动对象桥

本轮接在 classlib redirections 之后，仍在独立实施 worktree。实际 Unity 已执行
ClassLibraryInitializer.Init → UnityLogWriter.Init，随后执行
UnhandledExceptionHandler.RegisterUECatcher。完整游戏 backend/Admission 仍为 false。

## Windows classlib 服务

`mono_startup/services.rs` 及 `services/io.rs` 精确核对锁定 corelib 的 InternalCall
和 P/Invoke 签名、method identity/token、scope、entry、flags。服务只借用同线程 live
callback owner 中现有的 Runtime64/Memory，复用真实 guest handles/VFS/TEB；不创建
第二个 Runtime，也不向 guest Memory 传入 native buffer pointer。

覆盖 Windows platform/CRLF/OSVersion、路径分隔符/当前目录/temp/标准句柄，以及
MonoIO open/close/flush/filetype/length/seek/read/write。byte[] 在有界服务期间用 v2
handle pin，核对元素类型和 bounds，输入输出经 owned bytes 拷贝。
6 个 exact P/Invoke 是 attrs/fullpath/console CP 两项/BCrypt random/SetThreadErrorMode。
未准入的 P/Invoke 返回拒绝，plugin finder 保持 fail-stop，不加载未知宿主 DLL。
attrs/random 失败保留输出；fullpath 的 file_part 指向 native 输出 buffer 内。
现有 Runtime64 不支持 FILE_FLAG_DELETE_ON_CLOSE，桥保留错误 50，没有伪造成功。

## 固定日志 ABI 和回调栈

日志 InternalCall 为 UnityEngine.UnityLogWriter::WriteStringToUnityLogImpl_Injected，
锁定目标 0x1800b0b80，static void(ref ManagedSpanWrapper)，token 0x06001d07。
安装前核对 native metadata：value size 16、alignment 8、void* begin 与 int32 length；
Mono field offset 包含 16B object header，实际 payload offsets 为 0 和 8。

固定 ARM64 adapter 在同一 native Mono 调用线程接收 pinned UTF16，复制为 owned units。
Win64 调用使用临时 guest-local UTF16 buffer/span header；native pointer 从未成为
解释器地址。保留 NULL begin=0 与 empty begin=1/length=0 的差异，限制长度和重入深度。
真实 CoreModule 的 marshal IL 验证 NULL、empty、Unicode，以及 Init 后 Console.Write
的虚调用路由，四个实际 callback payload 精确匹配。

首次实际输出定位到 __chkstk 读取原主栈 TEB StackLimit。callback 期间现在设置
TEB StackBase/StackLimit 指向本层临时栈，返回或错误时恢复。每层保留独立 128KiB
栈区和 Win64 shadow space；guest 指令测试核对 gs:[StackLimit]、恢复、UTF16 数据、
sentinel 和失败后的 payload 撤销。没有更改 CPU、浮点、IR 或翻译缓存实现。

## 异常注册与启动对象生命周期

RegisterUECatcher（0x060024b2）创建真实 managed UnhandledExceptionEventHandler 并
订阅 AppDomain.UnhandledException；准入严格按 class/image/token/name/static void()
和顺序核对。注册通过不等于已经验证未处理异常输出、线程池或游戏异常恢复。

`mono_class_get_image` 仅复用已登记、已验证 image 的身份及 provider-owned reference。
补齐标准基础类 getter、array class、类型关系和接口元数据查询；所有输入按 kind
检查，结果仍为 opaque metadata token，宿主指针不会暴露到 guest。

Unity 随后实际创建 44 个 zero-length SZ arrays。空数组由 Mono 真正分配并 strong-root；
只发布经过 native header 核对的只读 bounds/length cells，object header/vtable/data
不开放。外部 mono_gchandle_*_v2 强句柄另有 token registry，核对对象身份、拒绝伪造
和重复释放，cleanup 先释放外部句柄再释放对象 registry 根。弱句柄、非空数组和
通用对象布局不因这些诊断被准入。

原始 DLL 的 loader-error API 已核对 RVA 0x270b90：33c0c3，即 xor eax,eax; ret；
锁定 Unity Mono 的同名零参数导出也直接返回 NULL。桥调用 native leaf 并验证 NULL，
不是按函数名猜测或伪造错误成功；未来非 NULL exception 仍拒绝。

## 实证范围

`native-mono-unity-exceptions-probe-02`：四条实际日志路由、三个 static void 初始化、
35092 metadata queries、82 image cycles、两次 GetType、strings/reflection GC 身份，
exit 0，guard 178/0，无 original Mono attach。证据文件都位于 worktree 的 wire-out。

`native-mono-unity-synchronization-probe-05`：44 个实际数组及外部强句柄已创建，44 个
数组 GC 验证，全部 roots/views cleanup。同步上下文仍未执行，probe harness 正确
报告该边界未完成；下一处为已确认的 loader-error leaf。后续结果追加到本文件。

GC runtime 和 8 套 native fixture 证据见 native-mono-incremental-gc-2026-10-09.md。
本轮日志/异常阶段 full feature 回归为 399 passed / 7 ignored；新增数组/handle 之后
16 项 mono_startup 定向测试通过、all-target Clippy clean，全量 feature 回归 401 passed / 7 ignored、Clippy clean。
上述时间不是与完整游戏启动相同终点的比较，不宣称整款游戏已获得性能收益。

## 同步上下文已执行

`native-mono-unity-synchronization-probe-10` exit 0，guard 231/0，原始 Mono attach 未发生。
真实有序初始化为：InitAssemblyRedirections → Init/UnityLogWriter → RegisterUECatcher →
StackTraceUtility.SetProjectFolder(String) → InitializeSynchronizationContext。
step 3 不是先前推测的同步上下文；probe-08 的真实 method token 0x06002478 与 owned IL
确认项目路径先写入静态 String，再把反斜杠替换为斜杠。参数仅接受 rooted string token，
NULL 保留；native 静态字段在调用后与强制 GC 后均核对规范化后的 UTF16。

UnitySynchronizationContext.InitializeSynchronizationContext（0x0600257f）已实际执行。
验证 Current 为对应真实类型、m_MainThreadID/Awaitable._mainThreadId 为当前 native Mono
线程 ID 1、Awaitable._synchronizationContext 与 Current 是同一对象；强制 GC 后身份和
TLS owner 不变。原有 Thread/GC contract 没有被跳过。

本次完成 130 image open/load/close/getter cycles、43623 metadata queries，585 空数组及
585 外部强句柄，7 RuntimeType、12 strings。数组/类型/字符串全部经 GC 验证，外部句柄、
roots、受检 views、metadata/image refs 均释放或退休。class userdata exported get/set 与
guest 直接访问 +232 共用 owned 8B scalar cell；源代码审计确认 unity_user_data 仅用于
embedding getter/setter，native class 仍保持 NULL，不安装 guest 地址。

probe-07/08 在未知 invoke 签名处失败，未执行终点 GC 验证；probe-09 项目路径完成，
585 数组已验证，正常 stop-before userdata，尚未执行同步上下文。不要把这些失败/中间
结果当作同步上下文或游戏完整准入成功。

下一实际 invoke 为 UnityEngine.SubsystemsImplementation.SubsystemDescriptorStore::
ClearManagedDescriptors，SubsystemsModule token 0x06000057，static void()。IL 为真实
List<IntegratedSubsystemDescriptor> 清空和 descriptor native ptr 归零；下一阶段按该
image 指纹和 exact signature 继续。完整游戏 backend/Admission 仍 false。
新增代码最近定向 16 项通过、Clippy clean；全量 401/7 是数组/handle 阶段的完整回归，
后续启动方法另以真实隔离 probe 和定向回归验证。本次没有修改 CPU/浮点/IR/翻译缓存。
