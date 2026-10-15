# 目录与文件变更契约迁移

基线为元数据与 ABI-only 冷绑定提交 `84816b4`，独立迁移前行为回归为 `de4bf29`。本批不修改文件状态 owner、ISA、IR/cache、图形或 Mono。

## 范围与行为

追加 12 个 export：CreateDirectoryA/W、DeleteFileA/W、MoveFileA/W、MoveFileExA/W、RemoveDirectoryA/W、CopyFileA/W。前四个在 PE32 为 Typed，其余八个保持 LegacyUnimplemented；Win64 前十个为 Typed，CopyFileA/W 保持 Absent。文件契约合计 38 个，PE32 25 Typed/13 ABI-only、Win64 32 Typed/6 Absent，原 DLL 范围保持 kernel32（PE32）与 kernel32/kernelbase（Win64）。

删除 PE32 的九项手写清栈重复声明，以及 Win64 arity/DWORD 重复项；此前仅依赖 Wine spec 的三个 PE32 名称也由同一契约明确声明。Wine 10.0 的 stdcall slot 数已核对。A/W、移动类型和 Ex flags 选择使用绑定的 FileApi ID，保留旧 handler 位置、返回 frame、trace 与控制提交。

PE32 CreateDirectory 继续忽略 SECURITY_ATTRIBUTES；DeleteFile 的 confined path、只读和目录检查保持。Win64 CreateDirectory 先读取 lpSecurityDescriptor，显式 descriptor 返回 50，保留原提前完成、unblock 与 API count；读取 source/destination 路径仍先于 MoveFileEx flags 校验。DWORD 高位截断、空目标错误 50、sharing/cwd/replace/只读和 rename 后 metadata 迁移顺序均保持。

本批复用已提交的两项 mutation 基线和现有真实目录、移动替换、属性、sharing、故障提交回归，未扩大 API 能力。CopyFile 不访问 guest 路径：PE32 仍零返回/LastError=120，Win64 仍拒绝导入。

## 验证

两项 mutation 基线在切换后通过；完整 release 415 passed / 7 ignored、debug Win32/AMD64/FPU/Unicorn 300、显式 native GPU/Metal 7、fmt、strict all-targets/all-features Clippy 和 build 通过。没有修改工具或额外重建 Mono。

冻结 `wire-out/refactoring-2026-10-14/r2-file-mutation-bound-wire/wire`，SHA256 `ba86c5a3bc569de21ea0c5d58290fb862234ac202265e3dab737887c6cd01b9b`，5,927,256 bytes，比元数据基线增加 15,280 bytes。源码、toolchain 与构建 manifest 一并保存。

## 性能验收

完成与元数据批次相同的 34 个 case/path、每条三对串行 AB/BA：PE32 四类 CPU 与六类 gateway 的 raw/IR 共 20 条，AMD64 10 条，V2 FP/bit/code-alias/data-alias 四条。PE32 使用 utility clamp，API trace 写到 null device 但保留格式化和计数；AMD64/V2 默认调度。全部 work/retired/checksum/API count 校验，计时与 Cargo、sample、运行时 diagnostics 分开。

事件 raw 初筛 CPU 中位数 +4.76%，各对 +3.65%/+8.16%/+0.76%，继续三对 210M 扩样。扩样中位数 +0.15%，各对 +19.67%/+0.12%/+2.62%，未复现持续回退，首对明显离散仍保留。其它初筛路径中位数回退均低于 3%，没有三对均回退超过 5% 的拒绝情形。最终矩阵仅以扩样替换事件 raw，最大 CPU 中位数回退 +2.25%（文件类型 raw），RSS 中位数最多增加 98,304 bytes；汇总为 `r2-file-mutation-bound-final-metrics.json`。

宿主快照记录了视频 Renderer 约 43% CPU 和 WindowServer 约 12%，未操作用户应用。本批 CPU/wall 大体接近，但整数、SSE、GUID 和事件仍有单轮离散；没有将测到的下降归因为解释器算法提速。AMD64 thread-id 本批 100M 中位数 −3.27%，继续保留上一批 700M 的 +4.10% 成本记录，不混合不同工作量的绝对吞吐量。

Alice 原始 guest Mono 的 200M 窗口回归按预算停止（exit1）：retired 199588616、API 390496、隐藏窗口 314×211、draw/present 0；保持现有启动前缀的窗口状态和预算出口，线程/时间路径使 API 数略有变化。未将其计作正式游戏渲染或 FPS 测量。日志 `r2-file-mutation-bound-alice-window.txt`，继续复用既有软链接目录。

完整证据继续写入 `wire-out/refactoring-2026-10-14/`，前缀 `r2-file-mutation-bound-`。提交前再次核对 runtime/test/tool 源码哈希与 freeze 一致。目录枚举、section、SHLWAPI、标准句柄及跨资源 CloseHandle 的契约仍待迁移，PE32 资源 owner 暂缓。
