# Local/Global 内存契约迁移

## 独立旧行为基线

Local/GlobalAlloc、ReAlloc、Free、Size、Lock、Unlock、Flags 共 14 个 export。运行时基线 `a7fb7a9`，before 为 `r2-heaps-wire/wire`，SHA256 `e17b21f7d7917fa495e896378721e6f156e4ce778ee251ef5a0049ccf63edc51`。现有 Local/Global 成对入口共用相同行为和资源；PE32 十项 typed、ReAlloc/Flags 四项 ABI-only，Win64 全部 typed，provider 分别为 kernel32 和 kernel32/kernelbase。

两项新增旧行为基线及 20 项内存领域回归通过。PE32 固定 process GuestHeap 互操作、拒绝私有堆归属、identity Lock、无效 Free 回传输入、成功/失败分配保持 LastError，以及 ReAlloc/Flags 的 error 120 和独立清栈。Win64 固定完整指针/SIZE_T、DWORD flags、Local/Global 互操作、Flags 0x8000、Unlock error 158、无效归属 error 6、不支持 flags error 50、超限 error 8，以及坏返回帧在 Free 前失败且不提交 CPU/API count，扩容移动保留 payload 和零填充。日志 `r2-local-memory-baseline-tests.txt`、`r2-local-memory-baseline-memory-tests.txt`。

原创可选 `gateway-local-memory` 先 LocalAlloc(0x40, 23)，再循环 GlobalSize，固定成对接口的共享资源行为。n=max(1, work//6)，两端返回 23n、API calls=n+1；Win64 retired=5n+16、work=6n+17，PE32 retired=5n+13。独立 PE inventory 核对恰好两个导入及 IAT 地址；before 的 Win64、PE32 raw/IR 12K smoke 和 20 项工具检查通过。默认 case 集合保持，日志前缀 `r2-local-memory-baseline-`。
