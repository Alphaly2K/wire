# Local/Global 内存契约迁移

## 独立旧行为基线

Local/GlobalAlloc、ReAlloc、Free、Size、Lock、Unlock、Flags 共 14 个 export。运行时基线 `a7fb7a9`，before 为 `r2-heaps-wire/wire`，SHA256 `e17b21f7d7917fa495e896378721e6f156e4ce778ee251ef5a0049ccf63edc51`。现有 Local/Global 成对入口共用相同行为和资源；PE32 十项 typed、ReAlloc/Flags 四项 ABI-only，Win64 全部 typed，provider 分别为 kernel32 和 kernel32/kernelbase。

两项新增旧行为基线及 20 项内存领域回归通过。PE32 固定 process GuestHeap 互操作、拒绝私有堆归属、identity Lock、无效 Free 回传输入、成功/失败分配保持 LastError，以及 ReAlloc/Flags 的 error 120 和独立清栈。Win64 固定完整指针/SIZE_T、DWORD flags、Local/Global 互操作、Flags 0x8000、Unlock error 158、无效归属 error 6、不支持 flags error 50、超限 error 8，以及坏返回帧在 Free 前失败且不提交 CPU/API count，扩容移动保留 payload 和零填充。日志 `r2-local-memory-baseline-tests.txt`、`r2-local-memory-baseline-memory-tests.txt`。

原创可选 `gateway-local-memory` 先 LocalAlloc(0x40, 23)，再循环 GlobalSize，固定成对接口的共享资源行为。n=max(1, work//6)，两端返回 23n、API calls=n+1；Win64 retired=5n+16、work=6n+17，PE32 retired=5n+13。独立 PE inventory 核对恰好两个导入及 IAT 地址；before 的 Win64、PE32 raw/IR 12K smoke 和 20 项工具检查通过。默认 case 集合保持，日志前缀 `r2-local-memory-baseline-`。

## 候选接入

14 项 export 接入七个 LocalMemoryApi ID，成对 Local/Global body 共用处理 ID，原 trace/export 身份保持。删除 PE32 十项重复手工 ABI、Win64 arity/DWORD 和根名称/后缀分发；PE32 adapter 只借原 GuestHeap、内存和进程堆 handle，Win64 adapter 保留 allocations/local_blocks、页分配、copy/map/unmap 与错误顺序。20 项内存领域回归通过，原 FormatMessage、CommandLineToArgvW 等生产的 local block 继续使用同一资源归属，见[调用边界](../architecture/local-memory.md)。

完整 release **467 passed / 7 ignored**、debug Win32/AMD64/FPU **315 passed**、显式 native **7 passed**、工具 **20 passed** 通过；fmt、strict all-target/all-feature Clippy 和 release build 通过。冻结 `r2-local-memory-wire/wire`，SHA256 `4554773e51c5f828d21c637b2286b81c047bbb218e5272612cb90f85767ecae8`，5,982,296 bytes，比 before 增加 **16,992 bytes**。准备后续系统信息测试前，**254 个 Rust 文件**（src/tests **246 个**）全部核对 manifest 哈希一致，见 `r2-local-memory-source-verification.json`；后续测试与未注册 fixture 不混入本候选。

## 性能与接受决定

28 条路径、各三对全部结果校验：AMD64 CPU 700M、七项 gateway 100M、V2 code/data alias 300M，PE32 四类 CPU 300M raw/IR 与 Local/Global、heap、vmem、TLS 四项 gateway 50M raw/IR。稳定启动及 PE32 utility/discard 协议保持，汇总 `r2-local-memory-stable-summary-initial.json`。最大 CPU 回退 **+2.979%**（PE32 integer/raw，三对 +4.450%/+2.979%/+0.839%），RSS 中位数最多增加 **112KiB**。没有路径触及 3% 扩样门槛，接受本候选。

LocalAlloc/GlobalSize 共享资源网关 Win64 **−2.624%**、PE32 IR/raw **+0.555%/−2.166%**；普通 CPU case 和 delay 网关的变化保留为观测结果，未归因为本次分发迁移，也不据此推算游戏 FPS。

Alice 200M work 六轮原始 guest Mono 启动前缀全部通过，保持预算出口、隐藏 314×211 窗口及 Draw/Present 0。CPU 中位数 2.703607s → 2.605765s（**−3.619%**），wall 2.789480s → 2.659738s（**−4.651%**），RSS 中位数增加 **112KiB**。沿用原 runtime，没有复制素材；全部轮次见 `r2-local-memory-stable-alice`。该前缀尚未进入实际绘制。
