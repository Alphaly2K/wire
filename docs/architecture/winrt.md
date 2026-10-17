# WinRT 字符串与激活边界

十一项 export 的静态 provider、ABI、参数宽度和执行策略集中在 `win32/catalog/winrt.rs`。WindowsCreateString/Reference、Delete/Duplicate、Len/IsEmpty/RawBuffer、HasEmbeddedNull/CompareOrdinal 以及 RoGetActivationFactory/ActivateInstance 使用 WinrtApi 冷绑定，执行时按 enum 分发。只有长度参数截断 DWORD，指针保持完整 Win64 carrier。

Win64 静态导入接受 combase；字符串另外接受 string API set，激活另外接受 WinRT API set。GetProcAddress 保留旧 loader 对两种 API set 到 combase 的统一映射，故动态查询范围与静态导入范围不同。模块装载准入保持原范围，未因契约注册自动开放 combase。PE32 全部 absent，继续原 unknown ABI、零参数清栈与 error 120；错误返回在 ole32 仍是 E_NOTIMPL，其他 provider 为零。

`x64/runtime/winrt/strings.rs` 私有拥有 owned 成员、长度、引用数和空字符串 buffer 身份。owner 只借受检 Memory 与 backing allocation map，读取、retain 和 release 不依赖总 Runtime64。分配、写入、设为只读后登记仍由 adapter 以原 heap helper 协调；未合并 task/Local/Heap 归属。

reference handle 是调用者持有的 24 字节 header。每次访问都重新读取 flags、长度和完整指针，校验输入范围；删除 reference 只验证，不释放调用者 storage。owned handle 则使用 owner 记录的长度与 handle+32 buffer，引用归零时先 unmap backing，再删除两处记录。保留 4M UTF-16 units、4096 个 owned block 的原限制、嵌入 NUL 和孤立 surrogate 的处理。空 RawBuffer 仍惰性分配只读缓存。

adapter 维持必需指针、输出预检、长度、terminator、header/输入的原检查与写入次序。重复 owned string 先预检输出，再增加引用并发布 handle；输出或返回帧故障不提交 CPU/API 计数。调用入口仍负责保存返回帧、unblock、LastError 和 trace。激活接口验证 IID/字符串并记录类名，返回原 class-not-registered HRESULT，保留 lookup 失败时的输出值；没有新增 factory/backend。

独立基线覆盖静态/动态 provider 差异、完整指针与 DWORD 长度、可变 reference header、owned metadata、输出/返回槽别名、错误优先级和激活输出/trace。原创 reference/length 负载循环复用调用者栈，校验 checksum、退休数、work 和 API 次数。验收见 [WinRT 重构记录](../reports/refactoring-winrt-2026-10-17.md)。
