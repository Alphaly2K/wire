# TLS/FLS 调用边界

八个 TLS/FLS export 的 provider、scalar ABI 与 typed ID 已汇入 `win32/catalog/locals.rs`。独立基线、切换和完整验收见 [迁移记录](../reports/refactoring-locals-2026-10-16.md)。

PE32 的两张 map 与两个递增计数器由 `win32/kernel/locals32::LocalSlots32` 私有持有，ApiState 通过组件组合，旧四个公开内部字段不再暴露。allocate/get/set/free 只接收 LocalKind、TID、索引和值；不借用 CPU、guest 内存、ApiState、调度器或 LastError。适配器负责获取当前 TID、维护错误/trace/callback 说明及提交返回 frame。Win64 继续使用原 Runtime64/TEB owner；没有统一两端槽位策略。[状态拆分验收](../reports/refactoring-locals32-owner-2026-10-16.md)独立记录性能与故障语义。

| 行为 | PE32 | Win64 |
| --- | --- | --- |
| provider | kernel32 | kernel32/kernelbase |
| 索引分配 | 从 1 递增，释放后不复用 | TLS 64 / FLS 128 个槽，复用空槽 |
| 索引/值 | DWORD 索引和 32 位值 | DWORD 索引；值和 FLS callback 保留完整指针宽度 |
| 值的存储 | `(TID, slot)` map；接受未分配索引 | 当前 TEB 的 TLS/FLS 区域；先验证槽有效 |
| GetLastError 影响 | TlsGetValue 清零；FlsGetValue 保持 | 两种 getter 成功清零；无效槽返回 0/error 87 |
| 释放 | 删除所有 TID 的同索引值，无效索引也返回 1 | 保持槽标记与 TEB 写入的原顺序 |
| FLS callback | 分配时记录原“callback not invoked”说明 | FlsFree 若存在 callback 且任一存活线程有非零值，返回原错误，保留槽位 |

Win64 分配先检查所有存活线程的目标 TEB 槽可写，再逐一写零，最后提交槽标记。输出 fault 不提前占用槽。TLS free 仍只清当前 TEB，再退休全局槽；下次分配按原规则清所有存活线程。FLS free 在原回调检查之后退休槽，不顺带更改残留 TEB 数据。

Win64 冷绑定由 LocalApi 选择原 TlsGet/TlsSet/TlsFree HotImport；执行 body 与 delay-general frame 检查保留。PE32 仍在公共返回阶段提交 EAX、frame 和 TEB error；返回槽 fault 可以发生在分配索引或资源变更之后。两端不同的故障顺序不统一为事务。

动态查询通过同一 provider 契约；历史上剩余的 FLS 与 timeGetTime 手工 lookup 名单已删除，并以 fresh-provider 测试约束合法 DLL 范围。`GetProcAddress` 已有 gateway fallback 保留。
