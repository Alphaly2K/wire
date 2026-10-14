# PE32 kernel owner 候选拒绝记录

基线为 `4500aaf`，冻结 executable 为 `r3-files-inline-wire/wire`，SHA-256 `9ad226f421044a504e77b520345a215bd0f5df724507f6a33115dd7a2672721c`。本批资源 owner 抽取没有合入：持续性能回退超过计划拒绝门槛，运行时代码恢复到基线。Mono/R6 跳过，此前 Win64 文件、双架构模块状态与通用优化提交保留。

## 候选与拒绝证据

尝试将 PE32 GuestFs、混合 HandleObj 表、命名索引和 allocator 收拢到私有 KernelObjects。文件/search/event/mutex/semaphore/thread/process 保留共享 namespace；I/O 权限、guest 输出失败、搜索游标和同步状态的提交顺序保持。公开 map 和通用 get_mut 不进入 owner 接口。迁移前独立固定了句柄身份/关闭寿命、文件 buffer fault、搜索输出 fault 和信号量先更新 count 后输出的行为。

首版 `r3-pe32-kernel-wire/wire` SHA-256 `aa67e746938c6623cbd185b74de6e3c19ddc12733c50f1c5a1d8ae7aff656157`。event/IR 的 100M 三对中位数 +5.58%，三对均超过 5%。反汇编显示 Root 新增三个 set_event 调用；普通 inline 提示没有消除它们，该版未做正式验收。显式内联 event 操作后三个调用消失，event/IR 降至 +2.23%；event/raw 300M 扩样仍 +4.66%。各候选、完成轮次和中断均保留。

event 内联版 SHA-256 `3e5d78454eb9bca648ae9faddbf87b07dd1761ddd1b58a4b5c416f7880df8fce`。26 个 case/path 各三对，work/retired/checksum 校验通过：PE32 SSE/IR −19.31%，AMD64 最大回退 +2.25%，但 PE32 integer/SSE/GUID/module raw 约 +3%–4%，V2 bit-test 300M 为 +6.59%。bit-test 700M 扩样仍 +7.83%，三对均超过 5%，因此拒绝。

采样发现迁移提前查询 scheduler 当前 tid，原先只在 mutex 分支查询。恢复惰性查询，并明确内联 readiness/acquire；不更改 wait/wake、错误或 guest frame 顺序。该版 `r3-pe32-kernel-lazy-wire/wire` SHA-256 `2d0817981e4fe2f3809916b001fe1587cde63aa127768c06cf8b0c8d4a752a49`，700M bit-test 仍三对均超过 5%，同样拒绝。进一步将公开 HandleObj 类型搬迁与状态抽取分开、保留原类型身份，`r3-pe32-kernel-legacy-type-wire` 仍未满足门槛；该候选也撤回。没有用 ISA/cache 改动、放宽阈值或增加新执行语义来掩盖结构调整成本。

## 调查及验证

PE32 raw event 两版分别有 2921/2778 个 guest runtime 样本；热点分布均包括 step_x86、时间查询、字符串比较与 trace 格式化，没有出现持续的新 event owner 调用热点。V2 bit-test 两版分别有 2303/2302 个 Wire 调用栈样本，主要落在 RegisterSegment::execute 和 try_register_segment。独立 1M diagnostics 的 IR=1000000、fallback=0、translations=10、invalidations=0，所有 cold/epoch/byte/mapping 原因相同。

热点反汇编的指令数及 opcode 序列相同：PE32 step_x86 2238，AMD64 RegisterSegment::execute 1968、execute_mixed 2925、CheckedSegment::execute 2156。地址、重定位和布局变化仍存在；这些证据没有定位全部成本的单一原因。按实际计时拒绝候选，后续 owner 迁移需要新的稳定方案。

各最终候选完整 release 为 403 passed / 0 failed / 7 ignored，debug Win32/AMD64/FPU/Unicorn 288 项通过；原生 GPU/Metal 7 项在 lazy/legacy-type 两版显式执行通过。strict Clippy、fmt 和工具 12 项通过，legacy-type 遗留 unused import 的一次 Clippy 失败及修复日志保留。原生输入 hash 记录在 `r3-pe32-kernel-native-inputs.json`。采样、正式计时与 Cargo 分开；API 密集计时和采样均将 trace 写入 null device，保留格式化和计数，没有复制或硬链接游戏。

原始证据目录为 `wire-out/refactoring-2026-10-14/`，前缀为 `r3-pe32-kernel-*`；每个 freeze 保存源码、diff、toolchain 和 hash。拒绝后的运行时代码没有保留 KernelObjects 或移除 ApiState 公开字段，仅保留两项独立行为回归、改为 API 观察的现有回归及版本化 event fixture。回滚状态的完整 release 403 passed / 7 ignored、fmt、strict Clippy 和工具 12 项再次通过；event 5000 work 的 raw/IR checksum/retired 功能校验通过。这些小样本不用于性能结论。PE32 文件/同步状态所有权仍待完成，先继续独立 R2 家族。
