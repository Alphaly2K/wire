# 目录枚举契约迁移

行为基线为目录/文件变更提交 `b81dc41`，独立迁移前回归与原创性能 fixture 为 `26aa4c5`。本批不修改搜索状态 owner、ISA、持久 IR v3、FLAGS、翻译缓存、图形或 Mono。

## 范围与提交顺序

追加 7 个 export：FindFirstFileA/W、FindFirstFileExA/W、FindNextFileA/W、FindClose。文件契约合计 45 个，PE32 32 Typed/13 ABI-only、Win64 39 Typed/6 Absent。新增七项均为双架构 Typed；DLL 范围仍为 PE32 kernel32、Win64 kernel32/kernelbase。

删除 PE32 的七项手写清栈，以及 Win64 的 find arity、Ex DWORD 与字符串路由重复声明。HANDLE 使用 Pointer；FirstEx 的信息类、搜索操作和 flags 为 DWORD。Wine 10.0 `kernel32.spec` 的 First/Next 两个 slot、FirstEx 六个 slot、Close 一个 slot 已核对；Win64 HANDLE 保持完整指针宽度。Win64 在 BoundImport 冷绑定时根据契约 ID 设置原 group bit 8，执行继续读取该缓存位，后续位不移位。

主 gateway 消费绑定的 FileApi ID，A/W、First/Next/Close 和 Ex 选择不再解析名称。原 handler 位置与算法保持：无效句柄/Ex flags 先于路径及输出检查，输出成功后才分配句柄或推进 cursor。PE32 FindClose 不移除普通文件句柄；分步输出、LastError 与错误返回保持。Win64 先检查整块输出再读取路径，输出覆盖 return slot/stack arguments 时继续使用预存的参数和返回 frame。Searches 的句柄空间、4096 活动搜索/65536 entries 上限保持。

公开 PE32 file facade 继续精确接受原 kernel32 范围，kernelbase 仍保持 unknown ABI 的零清栈诊断。GetFileType 继续由原主分发器处理。本批没有统一两架构的 metadata、错误映射或故障提交策略。

## 验证与冻结

新增两项独立 search 回归先在旧实现运行，通过后才切换路由。覆盖非法 flags/handle 的优先级、PE32 stdcall 清栈、错误类型句柄仍可读取、Win64 DWORD 高位截断、输出覆盖返回槽/栈参数、重复关闭与错误 DLL。现有 A/W 目录查询、真实 metadata、短输出和 cursor/fault 测试继续通过。新增 Win64 测试首次编译时误用了 Memory::read 的 buffer 参数，修正测试后重放通过，首次编译日志保留。

切换后文件领域 27 项通过；完整 release 417 passed / 7 ignored、debug Win32/AMD64/FPU/Unicorn 302、显式 native GPU/Metal 7、fmt、strict all-targets/all-features Clippy、release build 通过。工具12 已在 fixture 基线提交验证，本次工具实现未再修改。首次沙箱内完整 release 因 Unicorn SIGILL 中止；在允许参照执行的环境完整重跑通过，两份日志均保留。

最终冷绑定版本冻结为 `wire-out/refactoring-2026-10-14/r2-file-search-cold-route-wire/wire`，SHA256 `927662997fbc71b5102113ab1b5ca2f8f9f5c86d723010d177d7f56be659438f`，5,927,064 bytes，比目录/文件变更基线减少 192 bytes。source diff、输入 hash 与 toolchain 保存在同目录 manifest。

原创 FindClose(NULL) 循环不访问宿主文件或时间戳，独立校验返回值、retired/work 与 API count。基线及候选均通过 PE32 raw/IR 和 AMD64 的 12K smoke，默认性能 case 集合保持原样。

## 最终性能与启动验收

完成 37 个 case/path 的串行对照：PE32 四类 CPU 和七类 gateway 的 raw/IR 共 22 条，AMD64 11 条，V2 FP/bit/code-alias/data-alias 四条。每条至少三对 AB/BA，按各 fixture 声明校验 work、retired、checksum 和 API count。通常为 100M work；整数两条、AMD64 memory/metadata 和 V2 code-alias 使用 700M。PE32 保持 utility clamp 与 API trace 格式化/计数，日志写 null device；AMD64/V2 保持默认调度。计时与 Cargo、sample、runtime diagnostics 分开。

| 调查路径 | 首轮 CPU 中位数变化 | 补证与最终结果 |
| --- | ---: | --- |
| PE32 GUID raw | +3.93% | 再做三对相同 100M work，合并全部六对为 +2.79% |
| PE32 path raw | +3.23% | 再做三对相同 100M work，合并全部六对为 +2.65% |
| AMD64 metadata | +3.12% | 三对 700M 为 −0.07% |

GUID 补样首对仍有 +19.46% 离散，随后两对 −5.21%/+0.55%；原始三对 +6.70%/+3.93%/+3.09% 全部保留，六对共同计算中位数。path 六对为 −0.43%/+3.23%/+0.37%/+2.07%/+0.20%/+1.35%。metadata 首轮的一对 +20.32% 也保留，700M 各对 −0.07%/−0.81%/+0.18%。没有持续三对均回退超过 5% 的情况。

最终 37 条矩阵最大 CPU 中位数回退 +2.79%，RSS 中位数最多增加 32KiB；完整汇总为 `r2-file-search-cold-route-final-metrics.json`。初筛、补样和 pooled 六对的来源分别列在每条记录中。FindClose 固定负载的 CPU 中位数 PE32 IR/raw 为 −16.23%/−12.73%，AMD64 为 −12.45%；其余整数等结果按本报告记录的离散条件评估。

Alice 原始 guest Mono 的 200M 窗口回归按预算停止（exit1）：retired 199588950、API 390153，隐藏窗口 client 314×211，draw/present 0，保持现有启动前缀的状态与出口。日志为 `r2-file-search-cold-route-alice-window.txt`；复用既有测试目录，没有复制或硬链接游戏素材。这是启动前缀回归，正式游戏渲染和 FPS 不在本批测量范围。

提交前核对 273 个 runtime/test/tool/example/native 与 Cargo/build 输入，均与 freeze manifest 一致；当前 release binary hash 也一致。结果为 `r2-file-search-cold-route-source-verification.json`。文件 section、SHLWAPI/imagehlp、标准句柄及跨资源 CloseHandle 等契约仍待迁移；PE32 资源 owner 暂缓，R6 继续忽略。

## 未接受的首版候选

首版 `r2-file-search-bound-wire`（SHA256 `6409c8a666b63809a30ca086aba4fb62dc4d6d801089d7d2c1958fe1d4dcd12d`）也通过了上述完整正确性检查。37 个 case/path、100M work、三对 AB/BA 全部校验，但 AMD64 memory 三对 CPU 均回退超过 5%（+6.73%/+10.28%/+6.73%，中位数 +7.34%）。700M 补样仍有 +4.72% 成本；另有 GUID/module、PE32 bit/SSE 和 V2 alias 等路径超过 3% 调查线，因此没有提交首版。

独立 12K diagnostics 的两版 IR=12008、fallback=0、translations=16、invalidations=0，cold/epoch/byte/mapping 原因一致。CheckedSegment::execute 均在 `0x1002db000`，均为 2156 条指令，opcode 序列相同；45 个 instruction word 随调用/常量地址变化。原始反汇编与摘要为 `r2-file-search-bound-memory-{before,after}.asm` 和 `r2-file-search-bound-memory-code-summary.json`。ISA/cache 源码未改。

最终实现将搜索分类从逐调用的 FileApi predicate 移回 BoundImport 冷绑定，保留原 group bit 分支和 typed handler 参数。新候选先做 700M memory/code-alias 三对预检，CPU 中位数分别约 −1.35%/−6.07%；其它 CPU 变化仍按最终矩阵验收，不归因为搜索分支的直接加速。首版全部计时、长样本、diagnostics、source manifest 和二进制保留在 `r2-file-search-bound-*`。

## 整数长样本与噪声

冷绑定候选 100M integer IR 的第二对出现 +211.84%，其它两对约 −0.78%/−0.32%，中位数 −0.13%；raw 也有单轮超过 10% 的离散。继续用 utility clamp 做 700M、三对、三个标签的独立调查：before 与 control 引用完全相同的基线 binary/hash，after 引用冷绑定候选，每个标签的指令数及结果均校验。

700M integer IR 的 CPU 中位数 before/after/control 为 8.9993s/7.4521s/8.4972s；候选 −17.19%，同二进制 control −5.58%，control 各对 +11.10%/−11.69%/−1.12%。raw 为 19.3738s/19.5792s/19.8831s；候选 +1.06%，同二进制 control +2.63%，control 各对 +5.13%/+0.78%/+1.91%。候选 raw 各对 +5.69%/−2.53%/+1.06%，没有持续超过 5% 的回退。

整数结果按这次布局与宿主条件记录；短样本的极慢轮次和同二进制离散完整保留。日志与 manifest 为 `r2-file-search-cold-route-integer-noise`，摘要为同名前缀的 `-summary.json`。测试与计时分开，未操作用户应用，ISA/cache 源码未修改。
