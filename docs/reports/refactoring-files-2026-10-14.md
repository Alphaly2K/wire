# R3 Win64 文件状态归属

代码基线为 `4333428`。Win64 现有 Files owner 接管受限 GuestFs、NOT_CONTENT_INDEXED 元数据和标准句柄；Runtime64 保留公开 filesystem 安装入口与 Process facade，调用者经显式借用访问资源。PE32 文件状态和两架构 file API catalog 仍是后续工作，Mono/R6 跳过。

## 实现与行为基线

路径移动、删除、目录删除及属性修改由 Files 执行；ABI adapter 继续负责 guest 路径读取、LastError、返回 frame 和 trace。真实文件描述符、sharing/access、目录 cursor、mapping coherence 的原 owner 与提交顺序保持。元数据迁移与 canonicalize 的已有失败边界见 [文件契约](../architecture/files.md)。

私有 StandardHandles 分别管理 redirect 槽、原 endpoint 寿命和 alias。SetStdHandle 不关闭旧 endpoint；关闭原 endpoint 后 alias 仍存活，redirect 槽仍可返回原数值。保护关闭、继承 flags、allocator 范围和活动 alias 上限保持。DuplicateHandle 在 guest 输出成功后才关闭 source。

重构前先新增并运行独立 standard-handle 行为基线，涵盖 redirect/endpoint 区别、alias 独立 flags、protected CLOSE_SOURCE、无效输出的 CPU/资源不提交，以及下一成功 alias 的固定黄金值 `0x7fff3000`。重构后同一用例通过。新增原创 `gateway-file-type` fixture 累加标准 pipe 的 GetFileType 返回值，独立校验 API count、work、retired 与 checksum；默认性能 case 集合不变。

## 拒绝候选与代码生成调查

首版 `r3-files-wire/wire` SHA-256 为 `dac8f63cdcd89ff4b87014e2abe93ce284ff531743b7589fd0d1127c5911cf84`。正确性通过，但 100M 初筛的 AMD64 memory +5.96%、PE32 integer/IR +3.30%、data-alias +3.38% 触发调查。700M 扩样的 memory +7.80%（三对均超过 5%）与 integer/IR +4.11% 使该候选被拒绝；全部轮次保留在 `r3-files-*-timing` 和 `r3-files-{memory,integer}-investigation`。

在正式计时结束后采样 AMD64 memory。原始 sample 有效，但普通 probe64 不经过 Process64::run，现有 summary 报 no guest runtime samples found；未将该汇总失败当作通过。只读解析原始树显示 CheckedSegment、checked_access、Memory integer access 和 Pages 为热点。反汇编中候选 checked_access 调用了独立 Pages::get，而基线将 getter 内联。对小型 getter 增加 `#[inline(always)]`，没有更改 cache 容量、slot 身份验证、权限、page 寿命或 fault/ISA 语义。

修复版 checked_access 已恢复内联 Pages::get，调用 Pages::slot；独立 getter 符号消失。CheckedSegment/Descriptor 的整体布局也随编译变化，这些证据用于解释组合候选的接受，不推导 getter 是全部性能变化的唯一原因。

修复版补充初筛还发现 PE32 bit-test/IR +17.30%、FP/IR +3.09%。扩大到 700M 后为 +1.21%/−0.38%，三对分别为 +1.12%/+1.05%/+1.74% 与 −0.40%/−0.45%/+0.32%；原始短样本保留。单次独立 3 秒采样的 runtime 样本为 before 2257、after 2181，均主要落在 Descriptor::execute_inner、execute_mixed_run、PendingFlags 与 steps_cached。两版 Descriptor/mixed-run 的反汇编各有相同的 1659/503 条指令及相同调用目标类别，但地址/常量及部分生成指令存在差异；没有为短样本回退额外修改 PE32 ISA。证据位于 `r3-files-inline-bit-fp-investigation`、`r3-files-inline-bit-{before,after}-profile` 和 `r3-files-inline-*.asm`。

## 最终验收

冻结修复版为 `r3-files-inline-wire/wire`，SHA-256 `9ad226f421044a504e77b520345a215bd0f5df724507f6a33115dd7a2672721c`，5,869,048 bytes，相对模块基线减少 24,032 bytes。source/diff/toolchain 与输入 hash 由各 manifest 保存，原始数据位于 `wire-out/refactoring-2026-10-14/`。

- 完整 release：401 passed / 0 failed / 7 ignored；debug Win32/AMD64/FPU/Unicorn：286 passed；fmt、strict all-targets/all-features Clippy、Python 工具 12 项通过。沙箱内一次 Unicorn SIGILL、最初两处 filesystem 漏调用的编译失败均保留，最终验证在允许参照执行的环境完成。
- 24 个 case/path，各至少三对串行 AB/BA，全部 checksum/work/retired/API count 校验通过。700M 的 AMD64 memory −0.64%、PE32 integer/IR +0.38%、data-alias −2.12%；其余 100M 矩阵及上述 bit/FP 扩样的最大 CPU 中位数回退为 AMD64 SSE +2.71%。RSS 中位数最大增加 128KiB，原有资源/cache 上限没有增加。结果见 `r3-files-inline-{memory,integer,data-alias}-investigation`、`r3-files-inline-{pe32,amd64,v2,integer-raw}-timing`。
- 正式计时与 Cargo、运行时采样分开。计时后外部 native GPU/Metal 的 7 项 ignored 显式执行，7 passed，输入复用此前冻结的 graphics/MoltenVK/GS/DXBC。
- Alice 使用现有软链接隔离目录，200M work 按预算出口结束：retired 199588559、API calls 390531，隐藏窗口 client 314×211，draw/present 0。可核对启动边界与模块基线相同，计时/线程参与导致退休数与 PC 略变；这不是完整游戏 FPS 验收。日志为 `r3-files-inline-{native-tests,alice-smoke}.txt`，没有复制或硬链接游戏。
