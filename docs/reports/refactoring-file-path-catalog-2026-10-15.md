# 路径与属性 API 契约迁移

本批基于核心文件契约 `6e16790`，保留 PE32 执行边界和文件 adapter 所在函数。迁移前的两项独立行为基线已提交为 `1975f03`。原生 Mono/R6 忽略，没有修改 ISA、FLAGS、持久 IR 或缓存容量。

## 范围与行为

向 FileApi 与 FILE_CONTRACTS 追加 10 个 export：GetCurrentDirectoryA/W、SetCurrentDirectoryA/W、GetFullPathNameA/W、GetFileAttributesA/W、SetFileAttributesA/W。PE32 全部 typed/kernel32；Win64 新增 8 个 typed/kernel32+kernelbase，GetFullPathNameA/W 仍 absent。文件契约合计 21 个 export，Win64 17 typed、4 absent。

删除已迁移项在 PE32 清栈表和 Win64 arity/DWORD 表的重复声明。原 handler 消费绑定 ID，宽字符选择从 ID 派生。Win64 GetFileAttributesExA/W 继续作为 legacy 条目复用原查询 body；本批保留分支位置、endpoint 完成和输出提交顺序。

迁移前/后的行为基线验证 PE32 小 buffer 不输出、stdcall 清栈、字符串先于 file-part 输出提交、UTF-16 file-part 偏移与 wrong DLL；Win64 验证 DWORD 高位截断、kernelbase、输出覆盖 return slot、成功保持 LastError 与 absent export。初版 harness 的 LastError/return frame 用法错误及修正日志均保留。

新增原创双架构 `path_gateway.py`：循环查询 GetCurrentDirectoryA(0, NULL)，每次返回根目录所需容量 4，独立校验累计值、退休数与 API count。工具只在显式选择 gateway-path 时启用，默认 case 集合保持。

## 正确性与冻结

完整 release 410 passed / 7 ignored，debug Win32/AMD64/FPU/Unicorn 295 项、fmt、strict all-targets/all-features Clippy、build、工具12 通过。提供现有外部 native graphics/MoltenVK/原创 shader 输入后，GPU/Metal 7 项显式通过。日志前缀为 `r2-file-path-`。

正式冻结 `wire-out/refactoring-2026-10-14/r2-file-path-source-wire/wire`，包含新增 fixture 的源码快照。SHA256 `815987d0bb214b019213986936ed26be68b6c70fab44829792b8bc559e863cb7`，5,909,928 bytes，相比核心文件基线减少 19,616 bytes。较早的 `r2-file-path-wire` 二进制相同，但未包含稍后新增的 Python fixture，正式复现使用 source-wire。

## 性能调查

初筛包含 31 个 case/path、各三对 100M work：PE32 四类 CPU raw/IR 和 GUID/module/event/file-type/path raw/IR，AMD64 integer/memory/SSE/thread-id/GUID/version/module/file-type/path，V2 FP/bit/code-alias/data-alias。全部 work/retired/checksum/API count 校验。PE32 使用 utility clamp；AMD64/V2 使用默认调度，各路径只在相同条件下比较，不混算绝对吞吐量。

初筛最大 CPU 中位数回退 +2.57%，但整数 raw 单对离散较大，因此两种路径补做三对 700M utility 扩样。raw 中位数 +1.02%，IR 三对 +14.48%/−7.65%/+6.90%，中位数 +6.90%，继续调查。

IR 与 mixed 执行节前后分别为 2203/502 条，opcode 序列相同；SIMD execute 则由 1664 变成 1734 条，源码 body 未修改，编译生成发生变化，不能声称全部执行器机器码相同。反汇编与摘要为 `r2-file-path-{before,after}-__wire_*.asm` / `r2-file-path-code-summary.json`。

同一候选二进制的 utility 700M 三对 CPU 差异为 −21.48%/−10.23%/+2.33%，单轮 2.636–3.572s，仍存在明显离散。计时期间未运行 Cargo、sample 或运行时 diagnostics。保留 `r2-file-path-integer-noise`，随后追加六对 candidate/baseline；最终统计将合并全部九对 700M IR，不只选后续轮次。

追加六对为 +6.65%/−10.40%/−1.16%/−0.98%/+0.65%/−25.98%；全部九对 700M IR 的 CPU 中位数比仍为 +4.77%，没有将单对噪声视为通过。因此继续增加三对 2.1B work，结果为 +32.07%/−4.86%/−25.97%，CPU 中位数比 −4.86%，baseline 单轮 7.785–14.949s、candidate 10.282–11.067s。长样本未复现持续回退，调度条件仍为 utility；离散的唯一成因尚未确定，不据此宣称整数执行器提速。

最终矩阵对整数 raw 使用三对 700M、整数 IR 使用三对 2.1B，其余 29 条使用原三对 100M；所有初筛、700M 九对、同二进制对照与长样本均保留。最终最大 CPU 中位数回退 +1.91%（PE32 GUID IR），RSS 中位数最多增加 81,920 bytes（event IR），没有出现三对均回退超过 5% 的持续拒绝条件。最终汇总为 `r2-file-path-final-metrics.json`；统计只比较各路径的同条件 before/after。

Alice 原始 guest Mono 窗口模式 200M 启动 smoke 按预算停止（exit1）：retired 199588555、API 390543、隐藏窗口 314×211、draw/present 0。保持核心文件基线的出口，未测完整游戏 FPS；游戏仍复用已有目录与软链接。日志 `r2-file-path-alice-window.txt`。
