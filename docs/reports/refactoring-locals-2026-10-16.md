# TLS/FLS 契约迁移

## 独立基线

代码基线 `e2839d5`；before 复用 `r2-context-wire/wire`，SHA256 `b6971fd9a39005601f2910da636187b84aed83b9e72f8d54ed09db99c0853175`。本批八项为 TlsAlloc/GetValue/SetValue/Free 与 FlsAlloc/GetValue/SetValue/Free，不更换线程、TEB 或 slot 资源 owner。

六项独立测试先在旧 handler 上通过。PE32 固定从 1 递增且不复用索引、接受未分配索引、线程隔离、global free、TLS/FLS 的 LastError 差异和返回槽 fault 前的分配提交。Win64 固定 DWORD index、完整 64 位值/回调、直接 TEB 数据、invalid-slot error 87、只读槽分配 preflight、释放复用，以及需要 FLS callback 时拒绝并保留槽位。fresh-provider lookup 检查四个 DLL 下的八项接口及 timeGetTime，约束删除重复名单后的范围。

原创 `gateway-local` 先分配 TLS、设置 42，再循环读取并累加返回，检查输出、退休数和 API count。PE32 raw/IR、Win64 的 12K smoke 在旧 handler 上全部通过；默认 case 集合保持。日志在 `wire-out/refactoring-2026-10-14/r2-locals-baseline-*`，无需游戏素材。

## 接入与回归

八项接口接入 LocalApi，PE32 删除重复 ABI 与名称 handler；Win64 删除重复 arity/DWORD 和字符串分支，general adapter 只按 typed ID 分发，三个 TLS 热执行 body 保持。fresh-provider 的 FLS 与剩余 timeGetTime 名单删除，由已有 catalog lookup 范围接管；四 DLL 的独立查询基线切换后仍通过。

六项 focused 回归通过。首次完整 release 暂停于 GUID 试点测试：该测试曾把 TlsGetValue 作为未迁移 API 的哨兵，现已切换契约，旧断言不再适用。将哨兵换为仍未迁移的 VirtualAlloc，保留 Legacy 断言和全部 GUID provider 检查；原失败日志 `r2-locals-release-tests.txt` 保留，修正后的完整检查使用 `r2-locals-v2-*` 日志。

修正后的完整 release **448 passed / 7 ignored**、debug Win32/AMD64/FPU **296 passed**、显式 native GPU/Metal **7 passed**、工具 **19 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。候选 `r2-locals-wire/wire` 的 SHA256 为 `e471128c0d9ce664dcedf491b3874af335ad205f22190501a9b31b0741271a93`，5,937,000 bytes，比 before 减少 **6,880 bytes**。后续 PE32 slot owner 草稿尚未注册，不计入本批正确性结果。

## 性能调查

稳定启动协议下 **28 条**性能路径全部校验 guest 结果、退休/work 与适用的 API count。TLS 固定网关 AMD64、PE32 IR/raw 的 CPU 中位数分别 **+0.047%/−0.441%/−3.302%**。PE32 CPU 的 raw/IR integer、SSE、FP、bit 与 context/clock/module 网关均低于 3% 回退。

两项 AMD64 初筛超过调查线。memory/700M 为 **+4.643%**，三对 +5.718%/+5.129%/+0.227%；data-alias/300M 为 **+4.488%**，三对 +5.709%/+6.478%/+3.668%。分别扩为 **2.1B/900M**，各按 after/before/control、before/control/after、control/after/before 三种顺序执行。control 与 before 使用同一二进制哈希，全部初筛、离散轮次和扩样独立保留，不跨 work 混算。

memory 长样本中位数 **+3.511%**，三组 +5.174%/+1.551%/+2.697%，control/before +0.403%、after/control +3.096%。data-alias 长样本 **+3.071%**，三组 +4.876%/+2.730%/+2.326%，control/before −1.091%、after/control +4.209%；首组 before/control 同哈希仍有约 3.69% 差异。两项均未出现三组超过 5%，但局部成本仍存在，不能写成无回退。其它路径最大 CPU 中位数回退 **+1.884%**，RSS 中位数最多增加 **208KiB**。

计时结束后独立 diagnostics：memory 两版均 IR=12008、fallback=0、translations=16、invalidations=0；data-alias 均 IR=12007、fallback=0、translations=13、invalidations=0。CheckedSegment 与 RegisterSegment 的生成指令数分别为 **2156/1968**，各自助记符序列 SHA 相同；原始反汇编保留，不将该项比较描述为完整机器码/操作数相同。ISA/cache 源码和容量未改，具体成本机制仍未确证。本批记录调查后的成本，后续 owner 迁移继续独立测量。

全部计时汇总位于 `r2-locals-stable-final-metrics.json`，包含 28 条路径与两个独立扩样分组。诊断、反汇编和摘要位于 `r2-locals-{memory,alias}-diagnostics/`、`r2-locals-{before,after}-__wire_*.asm` 与 `r2-locals-executor-code-summary.json`。首次反汇编命令还输出了整个 text，其无效计数单独保留；随后按目标 section 重新提取，没有用无效计数参与判断。

## 接受与启动

提交前核对冻结 manifest 的 **232 个**已注册 Rust 文件，SHA 全部一致。Alice 200M 三对六轮均校验预算出口、隐藏 314×211 窗口、零 Draw/Present；CPU 中位数 **−1.219%**，wall **−2.088%**。沿用现有游戏目录和 fixed-argv-game-v1，不复制或硬链接素材，未进入实际绘制。证据在 `r2-locals-stable-alice/`。据完整正确性、28 条路径、扩样和独立诊断接受本批，保留 memory/data-alias 的局部成本；PE32 状态 owner 另作下一批，不能算入本批交付。
