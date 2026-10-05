# 真实游戏解释执行 CPU profile（2026-10-05）

真实目标的主要 CPU 热点是软件 x87。基线四次采样中，x87 占运行期样本约 **64%**，
IR 执行与缓存约 **20%**；反复解析固定十进制浮点常量约 **16%**。
此前对整数 IR、typed memory 和连续调度的优化只覆盖了其中一部分成本。
本轮按 profile 缓存常量，并以逐位等价的小整数除法替换超越函数级数里的通用除法。
四对交替运行的游戏 MIPS 中位数为 **15.02 → 18.76（约 +25%）**，
整个进程的 CPU 时间中位数为 **1.402s → 1.044s（约 −26%）**。性能目标仍未达到。

## 采集方法与范围

- 基线为 `a3de840` 的 release 可执行文件，保留于本机 `/private/tmp/wire-profile-before`。
- Apple M4（Mac16,12）、macOS 26.5.1；release 保留 debug symbols。
- 使用《王様恋愛【体験版】》完整游戏目录及相同版本 3 `.wir`，预算 50M guest 指令。
  每次约退休 21.42M 指令，仍停在未实现的 `RtlUnwind/MSVC stack unwind`。
- 未开启 CPU trace 或 opcode stats；API 日志配置保持相同。
- `/usr/bin/sample PID 5 1 -mayDie -fullPaths`：四次基线 IR、两次 raw 参照、三次优化后 IR。
  请求间隔 1ms，进程退出后采集结束，并非每次都有五秒运行期样本。
- 另以 Instruments `Time Profiler` 采集了一份基线 IR trace。
  `xctrace` 返回 54，但产生了可导出、可打开的有效 trace；guest exit 1 是既有 RtlUnwind 阻塞。

分析只保留 `Cpu::run_loop` 及其后代的样本，排除启动、PE/IR 读取绑定及结束报告。
sample 调用树的叶权重为节点计数减去直接子节点计数；分类按叶权重分区，
嵌套 guest callback 不重复累计。Rust demangler 会将模块内的 inherent method 打印为
`<wire::cpu::Cpu>::fpu_op`，所以 x87 分类同时识别 `fpu` 模块和该方法的后代。

## Profile 结果

下表各行分区互斥；分母为基线 4,781、优化后 2,420 个运行期样本。

| 运行期分类 | 基线四次 IR | 优化后三次 IR |
| --- | ---: | ---: |
| 软件 x87（包括其 APFloat 调用） | 63.75% | 48.68% |
| typed/cold IR 执行与缓存 | 20.14% | 30.29% |
| 其他 raw fallback | 10.04% | 12.44% |
| 外层调度及其他 | 4.16% | 5.29% |
| CPU stepping 外的 API 工作 | 1.90% | 3.31% |

样本占比是相对值：x87 加速后，IR 调度等工作占比上升不代表它们变慢。
raw 参照也经过 `steps_cached` 外壳，不能把 raw 样本中这个名字当成 IR 命中率。

| 热函数/调用链 | 基线运行期占比 | 优化后运行期占比 | 口径 |
| --- | ---: | ---: | --- |
| `Fpu::log_multiply`（FYL2X/FYL2XP1） | 32.84% | 26.20% | inclusive |
| `Fpu::exp2_minus_one`（F2XM1） | 24.66% | 16.82% | inclusive |
| APFloat `from_str_r` | 15.92% | 未采到 | inclusive |
| APFloat `sig::div` | 27.88% | 11.20% | self |
| `quad_divide_small` | 无 | 8.76% | inclusive |

inclusive 热点会包含其他行的工作，不能相加。单独看到的 `GuestMem::ptr` 约 2%、
`PendingFlags` 约 1.5% 也不能当作完整访存/FLAGS 成本；相当一部分检查已内联到调用者。
没有据此放宽 FLAGS 消除或访存检查。

Instruments 独立记录的 1,176 个运行期 CPU 样本同样以软件浮点为热点，
其中软件 x87 调用链约占 55%；这份记录没有作为上述分区表的分母。
该次记录有 976 个样本在 E core、200 个在 P core，约 **83% / 17%**。
不同采样和绝对 MIPS 存在明显调度/背景负载波动；未调整 QoS、绑定 core 或修改系统配置。

## 已落实的优化及精度边界

1. 用 `LazyLock` 仅解析一次原有 binary128 LN2；FLD 常数分别缓存四种 RC 的原有 FP80 结果。
   `.75`、`±.5` 这些精确二进制常量直接用位模式构造。没有缓存 guest 的运算结果。
2. 对级数中的正小整数除数（1…255），提取 binary128 的 113-bit significand，
   用 u128 商/余数归一化后进行一次 nearest-even 舍入。
   非正规数、可能下溢、NaN/Inf 和范围外除数继续走原 APFloat 通用除法。
   使用范围内的分子最多占 121 bits；规范化时丢弃的位纳入余数。

没有改变级数项数、收敛停止条件、binary128 中间精度、最终 FP80 舍入及异常处理。
原先中间除法只取 `.value`；新函数也只替换这个数值结果，不改变 guest sticky 状态逻辑。
本轮没有引入 QEMU 代码。

`small_integer_quad_division_matches_apfloat_bits` 对照独立的 APFloat 通用除法，
做 **26,536** 次逐位比较：全部 1…255 除数、符号、指数/尾数边界、
subnormal、NaN/Inf、邻近 1 的 significand 及 4,096 个随机位模式。
架构回归另外检查 FLDPI 在连续切换四种 RC 后的 FP80 位模式及不置 inexact。
完整 `cargo test --release` **72 项通过**（2 unit、34 differential、5 FPU、10 IR、1 Wasm、20 Win32）；
Clippy all-targets、format、Python 语法与 diff whitespace 检查通过。
这证明本轮替换与原实现的数值等价，不新增“所有 x87 超越输入均正确舍入”的声明。

## 不带 profiler 的交替性能对照

顺序为 AB / BA / AB / BA，不与 build/test 并发。`wait4` 的 user + system 时间统计
整个子进程，**包含装载、IR 绑定及结束报告**；CLI 的 guest MIPS 使用运行期 wall time，
**排除装载/AOT**。二者不能混为同一计时口径。

| 对 | 基线 guest MIPS | 新版 guest MIPS | 基线进程 CPU 秒 | 新版进程 CPU 秒 |
| --- | ---: | ---: | ---: | ---: |
| 1（AB） | 16.74 | 21.08 | 1.270 | 1.026 |
| 2（BA） | 14.23 | 18.45 | 1.445 | 1.022 |
| 3（AB） | 14.27 | 18.19 | 1.445 | 1.087 |
| 4（BA） | 15.77 | 19.06 | 1.360 | 1.062 |

每对进程 CPU 时间减少约 19%…29%，guest MIPS 增加约 21%…30%。
guest 计数在 21,422,846…21,424,707 之间；计时相关的初始化路径及 API 次数略有差异，
故这四对不能称为严格同路径实验。三对全部终态 GPR/EIP/EFLAGS 相同，另一对 ESI 分配地址不同；
所有重复都在同一 RtlUnwind 阻塞结束。数值正确性以回归和逐位 oracle 为依据。
此前另一批 wall-time 对照波动更大，保留原日志但不将其约 +60% 的中位数比作为稳定收益。

## 复现与本机产物

`tools/cpu_profile.py` 仅依赖 Python 标准库，采集需要 macOS `sample`，符号解码需要
`xcrun llvm-cxxfilt`。例如：

```sh
cargo build --release
python3 tools/cpu_profile.py capture "$GAME" --ir wire-out/game-v3.wir \
  --out wire-out/profile-repeat --runs 4
python3 tools/cpu_profile.py summarize wire-out/profile-repeat/run-*/sample.txt \
  --out wire-out/profile-repeat/summary
```

将 `GAME` 设为自己的完整目标 EXE 路径；IR 需事先 prepare 且与 EXE 匹配。
汇总生成 `summary.json`、`runtime-folded.txt`、可 hover 完整符号的 `flame.svg`。

本机原始样本、报告和派生数据在忽略目录 `wire-out/cpu-profile/`：

- `ir-0…3`、`raw-0…1`、`after-ir-0…2`：原始 sample 与 guest report。
- `before-summary/flame.svg`、`after-summary/flame.svg`：运行期火焰图及汇总。
- `ir-time-profiler.trace`：可在 Instruments 打开的原始记录；另有导出的 TOC/samples XML。
- `cpu-bench-summary.json`、`cpu-bench-*`：四对进程 CPU/wall 计时及完整 guest report。
- `bench-summary.json`、`bench-*`：之前那批波动较大的 wall-time 对照。

下一轮优先检查剩余的 APFloat 通用 division（例如 log2 的最终 LN2 除法），
再用带行号/反汇编的 profile 分解 `steps_cached` self time，判断调度、代码页验证和内联访存各自成本。
这些检查应继续以真实游戏和数值等价为准。功能阻塞仍为 RtlUnwind；M2 尚未验收。
