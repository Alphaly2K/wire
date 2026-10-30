# BSTR 契约与资源归属

Wasm 入口仅接受 oleaut32。PE32 提供 SysAllocString、SysAllocStringLen、SysReAllocString、SysReAllocStringLen、SysFreeString、SysStringLen、SysStringByteLen；AMD64 另提供 SysAllocStringByteLen。名称与仓库 Wine ABI 声明中的对应 ordinal 解析为同一网关。PE32 的 byte allocator 尚未接入，绑定时拒绝。

BSTR 指向可写 payload，其前四字节是 DWORD byte length，结尾另有两个零字节。长度查询直接读取前缀，接受可读的 foreign prefix；SysStringLen 按二除后截整。内容保留嵌入 NUL 和未配对 surrogate；SysAllocString 的 NULL 输入返回 NULL，显式零长度分配返回 owned 空 BSTR。

| 契约 | PE32 | AMD64 |
| --- | --- | --- |
| 所有权 | 与 Variant/WMI 共用进程堆及同一 BSTR 成员表；heap base 为 BSTR−4 | 独立进程数据页及成员表，公共 Heap/Local/task allocator 不拥有它 |
| 输入范围 | 显式 UTF-16 最多 1M units；终止字符串最多扫描 32768 units | 显式 byte payload 最多 16 MiB−6，UTF-16 长度先检查 DWORD 乘二 |
| NULL source 重分配 | 保留旧前缀并补零；source 等于旧 BSTR 时也按旧长度裁取 | NULL source 保留 owner 记录的旧 byte 前缀并补零；非 NULL source 按指定长度读取 |
| 错误 | 语义/容量错误返回零并写 LastError；非法访存报告可恢复故障 | 容量/长度超限返回零；未拥有或重复释放的 handle 明确拒绝，普通长度上限失败保持 LastError |

两种 ABI 都先保存调用参数与返回帧，预检完整输出，读取可能别名的 source，再分配 replacement。即将释放的旧 allocation 不能覆盖结果槽或目标 VARIANT；重叠在释放前拒绝。PE32 保留先发布新指针再释放旧资源的顺序，AMD64 先释放再发布。正常调用保持 FLAGS、FP/SIMD 与 LastError，故障不增加 API work 或客体退休数。

共享语义位于 `win32/com/automation32.rs` 和 `automation64.rs`，旧入口只保留内存/堆适配。进程启动拒绝释放堆与数据页；正常进程终态保留最终内存快照。旧 PE32 适配器继续保留原错误转换和晚返回帧规则，Wasm 入口使用上述统一受检边界。
