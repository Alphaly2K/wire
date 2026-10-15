# TLS/FLS 契约迁移

## 独立基线

代码基线 `e2839d5`；before 复用 `r2-context-wire/wire`，SHA256 `b6971fd9a39005601f2910da636187b84aed83b9e72f8d54ed09db99c0853175`。本批八项为 TlsAlloc/GetValue/SetValue/Free 与 FlsAlloc/GetValue/SetValue/Free，不更换线程、TEB 或 slot 资源 owner。

六项独立测试先在旧 handler 上通过。PE32 固定从 1 递增且不复用索引、接受未分配索引、线程隔离、global free、TLS/FLS 的 LastError 差异和返回槽 fault 前的分配提交。Win64 固定 DWORD index、完整 64 位值/回调、直接 TEB 数据、invalid-slot error 87、只读槽分配 preflight、释放复用，以及需要 FLS callback 时拒绝并保留槽位。fresh-provider lookup 检查四个 DLL 下的八项接口及 timeGetTime，约束删除重复名单后的范围。

原创 `gateway-local` 先分配 TLS、设置 42，再循环读取并累加返回，检查输出、退休数和 API count。PE32 raw/IR、Win64 的 12K smoke 在旧 handler 上全部通过；默认 case 集合保持。日志在 `wire-out/refactoring-2026-10-14/r2-locals-baseline-*`，无需游戏素材。
