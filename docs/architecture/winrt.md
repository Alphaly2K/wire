# WinRT 字符串与激活边界

十一项 export 的 ABI 与参数宽度集中在 `win32/catalog/winrt.rs`。WindowsCreateString/Reference、Delete/Duplicate、Len/IsEmpty/RawBuffer、HasEmbeddedNull/CompareOrdinal 以及 RoGetActivationFactory/ActivateInstance 使用 WinrtApi 冷绑定，执行时按 enum 分发。长度截断 DWORD，指针保留客体宽度。

Wasm 入口的两种 ABI 接受 combase；字符串另外接受 string API set，激活另外接受 WinRT API set；拒绝 ole32 别名。

`win32/com/hstrings.rs` 在新旧入口间共享调用处理、owned 成员、长度、引用数和空字符串 buffer 身份。各入口仅适配内存访问、分配和释放；Wasm 使用进程独立数据页，不与 task/Local/Heap 混合所有权。启动拒绝释放全部数据页，进程终态保留内存快照。

reference handle 是调用者持有的 24 字节 header。每次访问都重新读取 flags、长度和指针，校验输入范围；PE32/AMD64 的指针偏移分别为 12/16。删除 reference 只验证，不释放调用者 storage。owned handle 使用 owner 记录的长度与 handle+32 buffer，引用归零时先释放 backing，再移除成员。保留 4M UTF-16 units、4096 个 owned block 的原限制、嵌入 NUL 和孤立 surrogate 的处理。空 RawBuffer 惰性分配只读缓存。

adapter 维持必需指针、输出预检、长度、terminator、header/输入的原检查与写入次序。重复 owned string 先预检输出，再增加引用并发布 handle；输出或返回帧故障不提交 CPU/API 计数。调用入口负责保存返回帧，成功保持 LastError。激活接口验证 IID/字符串，返回 class-not-registered HRESULT，保留 lookup 失败时的输出值；旧适配器另保留类名诊断。当前没有 factory/backend。
