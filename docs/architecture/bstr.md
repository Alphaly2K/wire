# BSTR 契约与资源归属

八项 export 的 provider、参数宽度和执行策略集中在 `win32/catalog/bstr.rs`，Win64 的既有八个 ordinal alias 也在同一模块。两架构仅接受 oleaut32。PE32 七项 typed、SysAllocStringByteLen 保留 ABI-only/error 120；Win64 八项 typed。PE32 ordinal 仍由既有 Wine spec 解析，不扩大直接 `#ordinal` 字符串的支持范围。

PE32 的 Bstrings32 私有拥有 BSTR pointer 到 heap base 的记录，分配显式借 GuestMem、GuestHeap 和 process heap ID。Variant 与 WMI 继续通过原 `common_api::allocate_bstr` 内部 facade 委托同一个 owner。release 保留先删除成员再释放 heap 的顺序，并把是否找到成员、heap 是否释放成功分别返回给 adapter：SysFreeString 映射原 error 87；Variant、重分配和清理路径继续各自原先对 heap free 结果的处理。共用 UTF-16 输入读取迁入 `win32/text.rs`，算法和错误不变。

Win64 的 Bstrings 私有拥有 pointer、base 和 byte length，负责 replacement 成员/输出重叠检查及受检释放，只借 Memory/backing allocation map。原 heap helper 仍负责分配；写 prefix、内容、双字节 terminator 后登记。free 先 unmap，再移除 backing 和成员，未把 BSTR 与 task/Local/Heap 归属合并。

长度查询在两架构都直接读 pointer−4 的 DWORD byte prefix，接受可读的非 allocator 输入；SysStringLen 按二除后截整。分配与重分配的长度保持 DWORD，Win64 指针完整。PE32 长度上限和 Win64 byte/UTF-16 上限分别保留，空源、空字符串与 odd byte length 的历史差异不统一。

PE32 保留晚返回帧与 HRESULT/LastError 约定，重分配先发布新 pointer、再释放旧资源。Win64 先保存返回帧并预检输出，读取可能别名的 source 后分配和释放旧资源，再发布新 pointer；输出与即将释放的 allocation 重叠仍拒绝。返回槽别名按保存的原返回地址提交，失败不新增 CPU/API count。Variant 的类型支持范围和深复制策略继续原实现。

独立回归覆盖 ordinal/provider、foreign prefix、完整指针/DWORD、严格成员、失败 free 与 heap 共享、OOM/输出保留、重叠和返回槽别名；原创双架构查询负载校验 checksum、退休数和 API count，Win64 另校验 work。详见 [BSTR 验收](../reports/refactoring-bstr-2026-10-17.md)。
