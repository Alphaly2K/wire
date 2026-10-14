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

## 模块 catalog 后的重试

在 `60cd2b8` 模块契约和两个独立 AMD64 executor 布局调整后，恢复保留 HandleObj 类型身份的 owner 候选。release 406、debug 291、fmt、strict Clippy 通过；2.1B V2 bit-test 为 +0.65%，先前的 AMD64 bit-test 回退消退。随后完整矩阵在 PE32 暴露新的持续回退：integer/IR 100M work +195.10%（三对 +197.00%/+187.91%/+196.57%），FP/IR +8.28%（+7.78%/+8.44%/+8.28%），bit/IR +11.87%（+12.55%/+10.80%/+13.09%）。按门槛拒绝该候选并停止剩余计时，所有完成和中断的轮次保留；没有将它们计为 28 项完整通过。

冻结源码与二进制位于 `r3-pe32-kernel-{retry,final}-wire`，正式结果在 `r3-pe32-kernel-final-pe32-timing`，bit 预检在 `r3-pe32-kernel-retry-bit-preflight`。独立 700M integer/IR 采样的 guest self time 主要落在 steps_cached，FLAGS compute 的占比约 20%，两版相近。steps_cached 反汇编为 2208/2223 条指令，新增 15 条为非寄存器访存路径给 access_ptr 显式传常量参数；寄存器热路径结构与工作计数保持。defer_flags 两版同为 54 条、opcode 序列相同，调用点均为 4 个，没有发现新增 FLAGS 调用。代码和 switch 表地址改变，不能由这些证据确定全部成本的单一来源。

重试 owner 已从工作区撤出；PE32 executor 的布局稳定性将独立验证后再重试。新 `gateway-file-type` 原创 fixture 已单独提交为 `89a8f9a`，两版本 raw/IR 的 12K work 预检校验 checksum、retired 和 API count；只读打开自身 PE，无游戏或额外文件拷贝。

两个独立 PE32 布局候选也被拒绝。仅隔离 steps_cached 的 `ir32-layout-wire` 在完整矩阵的 SSE/IR 三对回退 +15.92%/+15.68%/+15.53%，停止剩余计时；其 owner 组合 `r3-pe32-kernel-ir32-wire` 只有预检结果，未完成验收。随后将 steps_cached、execute_mixed_run、SIMD execute_inner 分别放入独立 4KiB 对齐代码节，冻结为 `ir32-family-layout-wire`（SHA256 `cfcbd3f44f6c094b15d11a775a2d95af3c61121783fcbac371ae090a548b9dc1`）。三函数版正确性 release 406/debug 291、fmt、strict Clippy 通过；100M 四类 raw/IR 三对预检的 48 轮全部结果校验通过，但 bit-test/raw 连续三对 +5.02%/+6.12%/+6.10%，CPU 中位数 +5.57%，SSE/raw +4.47%。integer/IR 首对存在 +58.60% 离散值，后两对 +0.19%/−0.31%；所有轮次保留，未把中位数单独作为通过依据。

三函数候选没有继续完整矩阵、native GPU 或游戏 smoke，代码已撤回。证据前缀分别为 `ir32-layout-*`、`r3-pe32-kernel-ir32-*`、`ir32-family-layout-*`。布局调整没有改变 ISA/FLAGS/cache/预算；这些结果仍未定位回退的唯一成因。暂缓依赖布局调查的 PE32 资源 owner，继续独立 file catalog；不使用 padding 调试地址或更改验收门槛。

## 五执行边界与文件契约后的重试（2026-10-15）

以 `6e16790` 为代码基线、`r2-file-core-aligned-wire/wire` 为性能 before，适配新 FileApi 路由后重试 KernelObjects：保留 HandleObj 类型身份、event/readiness/acquire 内联与惰性 tid 查询。文件/同步对象仍共享旧句柄空间；五个 PE32 执行边界的节属性和 linker 配置保持。没有修改 CPU、FLAGS、IR/cache 或 guest 语义。

正确性完整 release 408/7 ignored、debug Win32/AMD64/FPU/Unicorn 293、fmt、strict Clippy、build、native7 全过。冻结 `r3-pe32-kernel-aligned-wire/wire`，SHA256 `92c74aa6a923385685bf9fdf98cf1c267b3a3b673ba7114fd8be23f62e91d687`，5,906,856 bytes。

明确使用与 before 相同的 utility QoS 条件，PE32 四类 raw/IR 各三对共 48 轮结果全部校验，但 integer/IR 三对 +219.43%/+207.64%/+203.62%，CPU 中位数 +206.81%；FP/IR +8.28%/+9.48%/+7.56%，中位数 +8.30%；bit/IR +10.54%/+8.52%/+10.77%，中位数 +10.23%。三项均持续超过拒绝门槛，不以 SSE/IR −12.70% 或 raw 中位数变化抵消失败，也不将先前默认条件的噪声对照用于豁免这批受控三对失败。

停止依赖该候选的后续矩阵，未做其 AMD64/V2 完整验收或游戏 smoke。源码全部撤回，tracked 工作区恢复 `6e16790`；已验证的布局、工具和核心文件契约保留。证据前缀 `r3-pe32-kernel-aligned-*`，所有完成轮次保留。按计划 §8 暂停这条 owner 迁移，继续独立路径/目录 API 契约；后续重试需要先形成新的可验证方案。
