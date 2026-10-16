# Heap 契约迁移

## 独立旧行为基线

范围为 GetProcessHeap、HeapCreate/Destroy/Alloc/Free/ReAlloc/Size/Validate、GetProcessHeaps，以及 PE32 ABI-only 的 HeapSetInformation/HeapQueryInformation/HeapCompact/HeapWalk，共 13 项。运行时基线 `21f7b74`，before 为 `r2-vmem-wire/wire`，SHA256 `91ea119a86746258c54dc19bd9582411c14282748c3a747a824dfc4481553568`。Win64 只保留原 GetProcessHeap 与 Alloc/Free/ReAlloc/Size 五项、kernel32/kernelbase provider，其余继续拒绝；双架构 allocator 和原资源归属保持。

两项新增旧行为基线通过：PE32 固定私有堆容量/归属、失败 LastError、成功保持旧错误、GetProcessHeaps 的单堆结果与忽略输出故障、输出覆盖返回槽，以及四项 ABI-only 的独立清栈和输出不变；Win64 固定完整 handle/SIZE_T、DWORD flags、非法 heap/flags 与坏返回帧在 HeapFree 前失败且不提交 CPU/API count、未迁移实现范围。既有相关八项 heap 回归通过，日志 `r2-heaps-baseline-tests.txt`、`r2-heaps-baseline-related-tests.txt`。

原创可选 `gateway-heap` 先获取进程堆并分配 23 字节，再循环 HeapSize 累加每次原始分配长度。n=max(1, work//8)，两端返回 23n、API calls=n+2；Win64 retired=7n+21、work=8n+23，PE32 retired=7n+18。独立 PE inventory 核对三个导入及 IAT 地址。Win64 12K smoke 首次通过；PE32 首次预期多计了不退休的终止 INT3，修正预期后在原 PE 字节上通过 raw/IR smoke。失败日志 `r2-heaps-baseline-pe32-smoke.txt` 保留；默认 case 集合保持。

## 候选接入

13 项接口接入 HeapApi，删除 PE32 重复 ABI、Win64 arity/DWORD 和根分发器中的 heap 名称分支。PE32 typed adapter 只借 allocator、内存、进程堆 handle 和 LastError；Win64 原 heap helper 按 typed ID 选择 body，原 allocator/slab/page 所有权和通用资源仍保持。没有修改 Local/Global、CRT 或原生 COM/callback 的分配策略，见[调用边界](../architecture/heaps.md)。

八项相关领域回归及完整 release **465 passed / 7 ignored**、debug Win32/AMD64/FPU **313 passed**、显式 native **7 passed**、工具 **20 passed** 通过；fmt、strict all-target/all-feature Clippy 和 release build 通过。冻结 `r2-heaps-wire/wire`，SHA256 `e17b21f7d7917fa495e896378721e6f156e4ce778ee251ef5a0049ccf63edc51`，5,965,304 bytes，比 before 增加 **1,840 bytes**。准备后续 Local/Global 基线前，**251 个 Rust 文件**（src/tests **243 个**）全部核对 manifest 哈希一致，见 `r2-heaps-source-verification.json`；后续未注册草稿和测试不混入本候选。

## 性能与接受决定

28 条初筛路径、各三对全部结果验证：AMD64 CPU 700M、七项 gateway 100M、V2 code/data alias 300M，PE32 四类 CPU 300M raw/IR 与 heap/vmem/delay/TLS 四项 gateway 50M raw/IR。稳定启动、utility/discard 和输入哈希协议保持，汇总 `r2-heaps-stable-initial-metrics.json`。PE32 FP/IR 初筛 **+3.736%**，三对为 −0.785%/+3.736%/−1.114%，进入扩样调查；其它最大回退 **+2.181%**，RSS 中位数最多增加 **128KiB**。

FP/IR 扩到 900M work，before/after/control 三标签轮换三对；control 直接使用 before 原路径和同一哈希。CPU 中位数差异 **+0.265%**，三对 +0.984%/+0.030%/+0.138%，同哈希 control **−0.046%**，after 相对 control **+0.311%**。接受该候选；初筛和扩样分别保留，不跨 work 合并，汇总 `r2-heaps-stable-long-metrics.json`。

HeapSize 网关初筛 Win64 **−25.973%**、PE32 IR/raw **−2.316%/−2.573%**。未改 ISA/IR 热循环，普通 CPU case 的下降和离散轮次保留，不归因为本次 heap 迁移；接口密集循环结果不推导游戏 FPS。

Alice 200M work 六轮原始 guest Mono 启动前缀通过，保持预算出口、隐藏 314×211 窗口及 Draw/Present 0。CPU 中位数 2.564462s → 2.556184s（**−0.323%**），wall 2.615675s → 2.609808s（**−0.224%**），RSS 中位数增加 **16KiB**。沿用原 runtime，没有复制素材，日志 `r2-heaps-stable-alice`；该前缀尚未进入实际绘制。
