# Virtual memory 契约迁移

## 独立旧行为基线

八项 export 为 VirtualAlloc/Free/Protect/Query 及其 Ex 版本。运行时基线 `7b9caf6`，before 为 `r2-timers-wire/wire`，SHA256 `cf7f8d6f6bf3f24c48086bad3d8569fd89a11f591123d15a8513731a33488027`。双架构保持独立内存实现及缓存归属，PE32 接受 kernel32，Win64 接受 kernel32/kernelbase。

两项新增基线在旧 handler 上通过：PE32 固定 VirtualProtect 输出覆盖未保存返回槽时 EIP=4、成功 LastError 不变，以及远程无效 handle 先于输出检查；Win64 固定 SIZE_T 64 位、DWORD 截断、完整 64 位 handle、预先保存返回 frame，以及输出 alias 后的权限与 LastError。既有内存领域 16 项回归通过，日志 `r2-vmem-baseline-tests.txt`、`r2-vmem-baseline-memory-tests.txt`。

原创可选 `gateway-vmem` 先分配 4096 字节，循环 VirtualQuery 并累加返回结构大小及查询 BaseAddress 与分配地址的差值。独立预期 n=max(1, work//11)，PE32 EAX=28n、retired=11n+17；Win64 RAX=48n、retired=10n+18、work=11n+19；两端 API calls=n+1。独立 PE inventory 核对恰好两个导入及 IAT 地址；before 的 Win64、PE32 raw/IR 12K smoke 与 20 项工具检查通过。默认 case 集合保持，日志 `r2-vmem-baseline-*-smoke.txt`、`r2-vmem-baseline-fixture-imports.json`。

## 候选接入

双架构八项接口接入 VirtualMemoryApi，删除重复 PE32 ABI、Win64 arity/DWORD 和名称/Ex 后缀分发。PE32 私有 `vmem_api` 迁入 `win32/x86/virtual_memory.rs`，只借内存与 LastError；Win64 adapter 只借内存和当前 TEB 错误槽地址。原受检内存 body、错误/输出顺序、缓存和内存资源 owner 保持，见[调用边界](../architecture/virtual-memory.md)。

首次编译捕获新 adapter 引用了不存在的 `x64::mem`，修为原 `x64::memory` 后，16 项内存领域回归通过；日志 `r2-vmem-focused-tests.txt`、`r2-vmem-focused-tests-fixed.txt`。首次全量 release 检查又捕获 GUID 试点测试仍用 VirtualAlloc 代表 legacy：改用明确未注册的 WireUnregisteredExport 检查相同 fallback，避免后续领域迁移反复破坏该测试。原失败日志 `r2-vmem-release-tests.txt` 保留。

完整 release **463 passed / 7 ignored**、debug Win32/AMD64/FPU **311 passed**、显式 native **7 passed**、工具 **20 passed**，fmt、strict all-target/all-feature Clippy 与 release build 通过。冻结 `r2-vmem-wire/wire`，SHA256 `91ea119a86746258c54dc19bd9582411c14282748c3a747a824dfc4481553568`，5,963,464 bytes，比 before 减少 **2,000 bytes**。准备后续 heap 基线前，**249 个 Rust 文件**（src/tests **241 个**）全部核对 manifest 哈希一致，见 `r2-vmem-source-verification.json`；后续未注册草稿和测试不混入本候选。

## 性能与接受决定

初筛 28 条相关路径、各三对，结果、work、retired 和 API count 全部校验：AMD64 CPU 700M、七项 gateway 100M、V2 code/data alias 300M，PE32 四类 CPU 300M raw/IR 与 vmem/delay/wait/local 四项 gateway 50M raw/IR。沿用稳定启动、utility/discard 和输入哈希协议。初筛 Win64 delay **+24.391%**、PE32 integer/IR **+18.210%**、local/IR **+24.584%**，其中 integer 三对均大于 5%，按门槛进入调查，未直接接受。其余最大回退 **+2.679%**，RSS 中位数最多增加 **96KiB**；完整结果保存在 `r2-vmem-stable-initial-metrics.json`。

三项扩大至 300M/900M/150M work，采用 before/after/control 三标签轮换三对，control 直接指向 before 的原路径及同一哈希，未复制二进制。Win64 delay 为 **−1.227%**，local/IR 为 **+1.569%**；integer/IR 仍为 **+3.548%**，三对分别 −0.526%/+3.548%/+2.656%，同哈希 control 相对 before **+4.749%**、after 相对 control **−1.146%**。记录这项残留差异和同二进制漂移；扩样未复现三对均超过 5% 的拒绝条件，结合控制样本接受候选。不同规模独立保留，不跨 work 合并，汇总 `r2-vmem-stable-long-metrics.json`。

VirtualQuery 网关初筛 Win64 **−11.438%**、PE32 IR/raw **−3.903%/−3.692%**。未修改 ISA/IR 热循环，普通 CPU case 的大幅下降和离散轮次全部保留，不归因为本次接口迁移，也不推导游戏 FPS。

Alice 200M work 六轮原始 guest Mono 启动前缀通过，保持预算出口、隐藏 314×211 窗口及 Draw/Present 0。CPU 中位数 2.715107s → 2.736041s（**+0.771%**），wall 2.781116s → 2.859517s（**+2.819%**），RSS 中位数增加约 **688KiB**。复用原 runtime，没有复制素材，日志 `r2-vmem-stable-alice`；该前缀尚未进入实际绘制。
