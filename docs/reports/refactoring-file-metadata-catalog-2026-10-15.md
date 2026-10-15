# 文件元数据 API 契约迁移

基线为路径/属性契约 `9f69b9e`，独立迁移前回归提交为 `81d5386`。本批只收敛五个 export 的 DLL、ABI、参数宽度与路由，不修改文件状态 owner、ISA、IR/cache 或 Mono。

GetFileAttributesExA/W、GetFileInformationByHandle、GetFileInformationByHandleEx、GetFileTime 追加到文件契约，合计 26 个 export。Win64 新增五个 typed/kernel32+kernelbase；PE32 五个保持 LegacyUnimplemented/kernel32，原 Wine spec 和手工表的清栈值一致，仍返回 0/LastError=120，不访问输出。

删除 PE32 GetFileAttributesExW 的手工清栈项，以及 Win64 的重复 arity/DWORD 项。Win64 原 handler 位置保持，以 bound FileApi 判断 basic/extended query、时间查询和目录信息分支。PE32 的冷 file facade 显式检查 Typed 执行策略，使 ABI-only 声明继续进入原未实现路径。

两项独立基线在切换前、后均通过：PE32 五个 export 的清栈、输出保持、诊断和 kernelbase unknown ABI；Win64 kernelbase、DWORD 高位截断、无效信息类先于坏路径/输出、短 buffer 不输出、时间/metadata 覆盖 return slot 后仍返回原地址、无效 handle 的错误顺序与 wrong DLL。已有真实 host 文件大小/identity/时间、共享目录 batch、cursor fault/restart 等用例同时通过。

新增原创 `metadata_gateway.py` 用无效信息类和 NULL 输入/输出调用 GetFileAttributesExA，校验累计零返回、退休数和 API count。Win64 走已有错误分支，PE32 保持 ABI-only 行为，不访问 host 文件/时间戳；case 只在显式选择 gateway-metadata 时运行。迁移前/后两架构 12K smoke 全部通过。

完整 release 412 passed / 7 ignored、debug Win32/AMD64/FPU/Unicorn 297、fmt、strict all-targets/all-features Clippy、build、工具12 通过。提供现有 native graphics/MoltenVK/原创 shader 输入后，7 项 GPU/Metal ignored 显式通过。未重建 Mono，也未复制游戏。

冻结 `wire-out/refactoring-2026-10-14/r2-file-metadata-wire/wire`，SHA256 `f6a05897f8f25d20730e94eaa9d7fcfd6974c0039d488aaa233b93273d6d1d2a`，5,915,512 bytes，比路径基线增加 5,584 bytes。所有日志与输入/源码快照仍在同一证据目录，前缀 `r2-file-metadata-`。

## 尚待性能验收

首组 utility 100M PE32 预检的 CPU/wall 显著离散。只读进程快照显示用户游戏 Client-Mac-Shipping 与系统 StorageManagementService 同时占用约 40%/57% CPU，另有 WindowServer 等负载；FP raw 单轮 wall/CPU 达 12.16s/6.65s，FP IR 的三对也无法作为稳定结构对照。确认本任务的 Python 进程组 19226 后尝试停止，但进程已经自行完成，kill 返回 no such process；未实际终止任何进程。全部 10 条路径的结果和完整日志保留，没有关闭用户应用，没有将这组计时计作性能通过或据此拒绝代码。

该候选的正确性和 native 检查已完成，性能验收未完成。随后独立准备目录/文件变更旧行为基线并提交为 `de4bf29`；首次将空移动目标错误码误写为 87，核对旧实现后修正为实际的 50，失败与修正日志均保留。

## ABI-only 路由补充

大型负载退出后重跑 `r2-file-metadata-replay-preflight`，10 条路径全部校验；CPU 路径最大中位数回退 +2.29%，但 ABI-only metadata IR 中位数 +12.16%，三对 −0.37%/+15.78%/+12.16%。这项未实现调用仍经过旧领域分发链，冷 file facade 在每次调用时再次解析契约。

后续候选在 BoundEntry 中缓存契约的 LegacyUnimplemented 策略。执行时直接保留原错误码、返回值、trace、参数读取和返回 frame 提交，未知 ABI 和未迁移 API 仍使用原策略；不再为已声明的 ABI-only 调用扫描领域 handler。该规则同样适用于此前已声明 ABI-only 的 identity/version/NLS/module 条目。

公开 ApiEntry 的编辑仍使 immutable binding 重建。新增回归在改动前通过并提交为 `dec912b`，验证 ABI-only → typed CWD → ABI-only 的同一 stub 重绑定、清栈和输出；新候选将重新执行完整相关检查与性能矩阵，未沿用首版的性能结果。

绑定候选完整 release 415/7 ignored、debug 300、native7、fmt、strict Clippy 和 build 全部通过。冻结 `r2-file-metadata-bound-wire/wire`，SHA256 `6ddc5c56484c8024b45da7732b7d6eeea716356e489b6a8dca28fbe29d8e0dc5`，5,911,976 bytes，比路径基线增加 2,048 bytes。双架构 12K metadata smoke 再次通过。

该版 utility 100M 预检全部校验，metadata IR/raw 的 CPU 中位数 −17.85%/−9.02%；integer/IR +10.85%、FP/IR +7.30% 触发 700M 扩样，扩样结果分别为 +0.04%/+1.27%。各对整数 +21.07%/−21.35%/+1.77%、FP −7.84%/+4.12%/+3.91%，全部保留。其余八条预检路径的 CPU 中位数回退均低于 3%。

静态代码核对：IR/mixed 节前后为 2203/502 条、opcode 序列相同；SIMD execute 从 1734 变成 1664 条，opcode 序列不同。原 ISA/source body 没有修改，完整生成代码变化保存在 `r2-file-metadata-bound-code-summary.json` 与对应反汇编。未用指令数减少代替实际性能结果。

## 最终验收

最新绑定候选完成 34 个 case/path、每条三对串行 AB/BA：PE32 的四类 CPU 与六类 gateway 各 raw/IR，共 20 条；AMD64 integer/memory/SSE 和七类 gateway，共 10 条；V2 FP/bit/code-alias/data-alias 共四条。PE32 使用 utility clamp，AMD64/V2 默认调度；计时与 Cargo、采样、运行时 diagnostics 分开，全部 work/retired/checksum/API count 校验。

PE32 五类既有 gateway 的 CPU 中位数回退最大 +2.72%；新 metadata IR/raw 为 −17.85%/−9.02%。AMD64 thread-id 初筛 +4.67%，继续三对 700M 扩样后 +4.10%，各对 +4.40%/+2.75%/+4.46%，保留这项局部成本。独立 12K diagnostics 的 IR=9610、fallback=0、translations/cold=14、invalidations=0、API=2400 均一致。HotImport 和 dispatch_hot 源码未改；dispatch_hot 没有独立符号，位于已改动的总 dispatcher 中，不能据此声称生成代码相同或回退只有测量噪声。长样本未达到三对均超过 5% 的默认拒绝条件，按调查结果接受该候选，并在下一批继续对照此热路径。

最终矩阵用 700M 替换 PE32 integer/IR、FP/IR 与 AMD64 thread-id，其余使用初筛 100M。最大 CPU 中位数回退为上述 +4.10%，其它路径最大 +2.72%；RSS 中位数最多增加 147,456 bytes。完整汇总为 `r2-file-metadata-bound-final-metrics.json`；所有污染、首版失败、短样本、扩样与 diagnostics 结果都保留，各调度条件单独比较。

Alice 原始 guest Mono 的 200M 窗口 smoke 按预算退出（exit1）：retired 199589025、API 390129、隐藏窗口 314×211、draw/present 0。保持原启动前缀的窗口状态和预算出口，真实启动中的线程/时间调用使 API 数略有变化；没有将其记为完整游戏渲染或 FPS 测量。日志 `r2-file-metadata-bound-alice-window.txt`，继续复用已有目录与软链接。

正式接入使用绑定候选 freeze（SHA256 `6ddc5c56484c8024b45da7732b7d6eeea716356e489b6a8dca28fbe29d8e0dc5`），完整 release 415/7 ignored、debug 300、native7、工具12、fmt、strict Clippy、build 通过。提交前再次核对 runtime/test/tool 源码哈希与 freeze 一致。目录/文件变更、枚举、section、标准句柄等剩余契约和 PE32 资源 owner 仍待推进。
