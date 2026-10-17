# Variant 调用与资源边界

四项声明在 `win32/catalog/variant.rs`：VariantInit、VariantClear、VariantCopy、VariantChangeType。PE32 仅 oleaut32 typed，Win64 保持 absent。参数数、stdcall 字节数和绑定 ID 从契约派生；flags 与目标类型保留 DWORD carrier，目标类型由 adapter 取低 WORD。PE32 ordinal 继续使用既有 Wine spec 解析，literal `#9` 的旧 ABI 条目保持独立。

`win32/x86/variant.rs` 只借 Args、GuestMem、GuestHeap、Bstrings32、进程 heap ID 与 LastError。入口继续负责参数读取、CPU 返回、API/trace 计数和返回槽提交。Variant 和 WMI 都直接使用同一 Bstrings32 owner；common_api 的名称分发与内部 allocation facade 已删除。

现有 PE32 VARIANT 子集使用 16 字节布局。scalar copy 保留 reserved 和 payload 字节；Clear 只清 tag。BSTR 深拷贝先获取新资源，再释放旧目的资源，目的清理失败时通过同一个 Bstrings32 owner 回滚；自复制仍执行深拷贝。BYREF Clear 保留所指资源。未实现的 managed 类型仍返回原 HRESULT，VariantChangeType 不扩展转换集合，flags 仍忽略。

Init/Clear/Copy/ChangeType 保持原访存检查和发布顺序，失败 HRESULT 同时写 LastError，成功保留 LastError。Win64 不因增加契约而出现新 import 或动态查询能力。以上为已实现行为的结构迁移；新增类型转换和 Win64 支持需单独行为验收。
