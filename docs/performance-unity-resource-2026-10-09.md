# Unity 资源加载阶段的解释执行优化（2026-10-09）

接续 `9a22054` 及本轮资源加载修复，按用户要求先冻结功能、优化执行速度。
本次 before 已包含 SRV 剩余范围、SIMD 插入/提取、procedural Draw、目录枚举、
extended DOS 路径、GetSystemTimes 和有效 blend target 校验；after 保持相同功能。
此前到 SRV 错误的启动时间不能作为这个更长路径的 before。
真实重放的共同终点是 `kernel32.dll!SleepConditionVariableSRW` 未实现，
不是耗尽预算、首帧、标题画面或可玩验收。
两组真实窗口 AB/BA 到同一错误的 wall 中位数 **147.462→105.639 秒**，
缩短 **28.4%**；退休指令吞吐 **31.968→43.927 MIPS**，增加 **37.4%**。
独立逐帧捕获已确认 Logo 淡入淡出；沙漏只有首帧后遇到 SRW 未实现错误，持续加载仍未恢复。
用户对照 CrossOver 确认蓝色加载背景正确，背景问题已关闭。

## Profile 与实现

基线线索来自 pass161 后段的独立采样，16,380 个运行期样本：
ScalarIr::step self 21.06%、Memory::cached_ir 12.33%、active_suspended 5.45%、
memmove 5.52%、ScalarIr::read 5.02%，API 分类 13.02%。
该采样请求 45 秒，但进程中途退出；只用于确定热点，不能当作完整启动对照。
原始栈在 `wire-out/games64/alice/profile-pass-161.txt`，汇总在同目录的
`profile-pass-161-summary/`。inclusive 和 self 不相加。

最终保留以下实现：

- 16,384 项 translation cache 的 PC/epoch/IR 热数组与 iced 冷数组分开。
  scalar hit 只读密集热记录，容量及完整 PC/epoch 验证保持原设计。
- 内联 cached IR 查找、标量 dispatch、operand 读写和 FLAGS 计算，
  未命中进入原 checked decode/fallback。仍为 24B typed IR，未做宽度四份展开。
- CF 用本宽度比较替代 u128。ADD/ADC 使用截断结果判断进位，保留
  `b=mask, carry=1` 的全周期进位；SUB/SBB 保留相等且 carry=1 的借位。
  所有算术 FLAGS 仍完整 eager，未扩大标志丢弃。
- 普通 guest 指令进入紧凑循环，到预算、下一窗口轮询或 10,000-work quantum
  中最早的边界返回。每步仍检查 gateway 完整地址及 RETURN/CALLBACK_RETURN。
  active suspension 在 run 入口与每个 API 后检查，避免每条普通指令查询 host 线程表。
  此证明依赖当前单 host CPU 合作调度；不能原样用于未来异步线程执行。

checked memory、一次性 guard、fault 回滚、自修改失效、retired/work 和原始 guest
指令边界保持不变。没有宿主 x86 执行、guest CPU JIT、跨指令融合或 unchecked 指针。
缓存拆分不改变条目数；新增函数内联可能增加宿主代码体积，未宣称没有空间成本。
实现与边界见 [IR 设计](ir-aot-design.md)。

## 确定性负载

Apple M4，release 默认优化并保留调试符号。五类原创 PE32+，每类约 200M
退休指令，三轮交替 before/中间候选/after 与反向顺序，共 45 次、约 9B 指令。
全部版本的返回 checksum 与精确退休数一致。CPU 统计、GPU、采样关闭；计时期间
没有同时编译、测试或执行其它基准。下表是全进程 wall MIPS 的三次中位数。

| 负载 | before MIPS | after MIPS | 吞吐增加 |
| --- | ---: | ---: | ---: |
| 寄存器 / FLAGS / CMOV | 86.947 | 128.483 | 47.8% |
| checked 内存 | 73.016 | 107.054 | 46.6% |
| SSE 与标量混合 | 32.649 | 36.130 | 10.7% |
| 16,384 指令 / 约 53KiB 代码 | 33.321 | 43.334 | 30.0% |
| 间接 CALL / RET / 栈 | 83.951 | 144.686 | 72.3% |

各轮命令、wall、process CPU、checksum 与计数保存在
`wire-out/perf-unity-resource-2026-10-09/inline-matrix/compare.json`。
寄存器测试 after 的 wall 中位数 1.557 秒、CPU 1.381 秒，有宿主未获调度的噪声；
不能用最短一轮代替中位数，也不能把矩阵吞吐当作游戏启动加速。

前两组同规模实验已淘汰：仅缓存布局/紧凑循环没有一致收益；四种 const BITS
展开虽改善大代码负载，却使 checked 内存和寄存器回退，已经从源码撤掉。
相应数据分别在 `matrix/compare.json`、`typed-matrix/compare.json`。
最终实现采用动态 typed opcode 与编译器内联，不保留未经真实负载验证的四份执行器。

## 真实窗口对照

隔离副本、真实 AppKit/GPU Present、冻结图形库及相同 10B work 预算，
每次均在达到精确同一 SRW 错误后提前结束。CPU 统计、capture、API 聚合与 profiler
关闭；计时期间没有编译、测试或其它基准。两组 AB/BA 共四次、约 18.7B 退休指令。

| 顺序 | before wall 秒 | after wall 秒 | before MIPS | after MIPS |
| --- | ---: | ---: | ---: | ---: |
| 1，before / after | 148.372 | 105.459 | 31.777 | 44.000 |
| 2，after / before | 146.552 | 105.819 | 32.158 | 43.854 |
| 中位数 | 147.462 | 105.639 | 31.968 | 43.927 |

| 路径指标 | 1 before | 1 after | 2 before | 2 after |
| --- | ---: | ---: | ---: | ---: |
| 退休指令 | 4,714,810,185 | 4,640,192,845 | 4,712,878,462 | 4,640,605,301 |
| 完成 API | 7,787,540 | 7,701,422 | 7,781,125 | 7,705,802 |
| GPU Draw / Present | 416 / 149 | 425 / 152 | 411 / 147 | 424 / 152 |
| domain 重置秒 | 0.349 | 0.213 | 0.295 | 0.260 |

进程 CPU 中位数 144.906→103.567 秒。全部停止于 thread 29 entry、
RIP=`0x7ffe000007c0`，窗口 visible=true、1024×720，错误为：

```text
kernel32.dll!SleepConditionVariableSRW: unimplemented Win64 import: kernel32.dll!SleepConditionVariableSRW
```

真实计时器与工作线程使退休数减少约 1.6%、API 数约 1.0%，因此不是相同的指令序列。
总延迟约 1.40 倍加速包含路径变化；按退休数计量的吞吐约 1.37 倍。
每轮完整命令、wall/CPU、report 与 Player.log 保存在
`wire-out/perf-unity-resource-2026-10-09/alice-full/`。

冻结文件及 SHA-256（均在 wire-out）：

```text
wire-resource-before-perf-2026-10-09
4298ef1eed4b50f66c4d7a81a5e26604169a561faf57d22afa6feee7a0b237f4
wire-resource-inline-perf-2026-10-09
54c0f91ebe181ee0cfc3f8887ea9942ab732651615e8fc228e66cb4494d70ca7
libwire-graphics-resource-perf-2026-10-09.dylib
815e53b566aa54d84751aad7a3e9a739474f822bedf076572b4bf3e6a74fb8bb
```

## 正确性

完整 release **336 passed、7 ignored**。新增 ADC/SBB 四种宽度的 784 个边界差分
状态，与 Unicorn 比较结果和完整 arithmetic FLAGS，包括窄寄存器保留高位、
I32 零扩展及全周期进位。线程上下文测试覆盖 1-work 和 100,000-work slice，
保持 Suspend/Resume/GetThreadContext 与 quantum 行为。
既有自修改、执行权限/guard、故障回滚、gateway 空隙、预算和 callback 回归全部通过。
Unicorn 仅为测试 oracle，不进入运行主路径。

Clippy release all-targets `-D warnings` 通过。完整测试、Clippy 和 build 日志分别在
`/private/tmp/wire-resource-final-{tests,clippy,build}.txt`。
本轮功能的原生 GPU 回归与 geometry/Draw 探针见
[资源加载记录](unity-resource-loading-2026-10-08.md)。

## 优化后完整采样

独立运行仍到同一 SRW 错误，422 Draw / 151 Present，94,905 个运行期样本。
请求采样 180 秒，进程中途正常输出错误结束；带采样 wall 115.697 秒、process CPU
106.044 秒不混入前述无采样 AB。

| 热点 | self 占比 | inclusive 占比 |
| --- | ---: | ---: |
| Process64::run | 32.77% | 100% |
| memmove | 8.23% | 8.23% |
| Cpu64::step_fallback | 6.40% | 45.83% |
| iced Decoder::decode_out_ptr | 4.53% | 9.54% |
| ScalarIr::lower | 3.63% | 5.06% |
| Memory::read_int | 2.90% | 4.33% |
| Pages::slot | 2.89% | 2.89% |
| Cpu64::execute | 2.54% | 7.43% |

decode/fetch 分类占 25.89%，API 分类 9.62%。Cpu64::decode 还包含缓存读取与权限检查，
不能把 25.89% 全部算成重新解析 x86。step_fallback 的 inclusive 又包含 decode/execute，
不能将这些占比相加。
标量 dispatch 与 cached_ir 内联后大量样本归到 Process64::run；不能从剩余独立
ScalarIr 符号减少就声称 IR 执行成本几乎消失。旧基线只采样后段，当前为完整启动，
不直接比较两者的函数占比来推断绝对速度。

下一步通用 CPU 方向仍是 translation miss/冲突、真实重新解码及 cold fallback。
原生 Mono 可能消除其 VM 和托管方法体的 x64 解释成本，但这个 profile 没有 guest 模块/
托管方法归因，不能从宿主解释器热点推定它会带来几倍收益。
计划复核及原型验收边界见 [Mono 复核](native-mono-plan-review-2026-10-09.md)。
原始栈与汇总/flame.svg 分别在 `after-profile/alice-startup/`、`after-summary/`。

## 复现

```sh
python3 tools/amd64_perf.py matrix \
  --before wire-out/wire-resource-before-perf-2026-10-09 \
  --cached wire-out/wire-resource-typed-perf-2026-10-09 \
  --after wire-out/wire-resource-inline-perf-2026-10-09 \
  --out wire-out/perf-unity-resource-replay/matrix \
  --work 200000000 --pairs 3 --skip-coverage

python3 tools/amd64_perf.py game \
  --before wire-out/wire-resource-before-perf-2026-10-09 \
  --after wire-out/wire-resource-inline-perf-2026-10-09 \
  --out wire-out/perf-unity-resource-replay/alice-full \
  --work 10000000000 --pairs 2 --skip-coverage --window \
  --expected-error 'kernel32.dll!SleepConditionVariableSRW: unimplemented Win64 import: kernel32.dll!SleepConditionVariableSRW' \
  --player-log Users/wire/AppData/LocalLow/TukiMahiru/AliceToPapaADV_1-0-0/Player.log \
  --cwd wire-out/games64/alice/runtime \
  --graphics-backend wire-out/libwire-graphics-resource-perf-2026-10-09.dylib \
  --moltenvk /private/tmp/wire-moltenvk-1.4.2/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib
```

`capture --window --work 10000000000 --sample-seconds 180` 单独采样；
逐阶段画面诊断也独立运行，不能混入性能对照。游戏目录只使用隔离副本。
