# 第二轮解释器优化与完整启动 profile（2026-10-06）

后续以本轮最终版 `2eea900` 为基线的块内批量执行和三对复测见
[第三轮静态序列记录](performance-scalar-runs-2026-10-06.md)。本文件保留本轮历史结果。

继续暂停功能扩展，优化真实游戏启动的解释主路径。基线为上一轮已提交的 `a694147`，
冻结文件 `/private/tmp/wire-perf-startup-final`，不复用更早的25秒基线计算本轮收益。
没有启用Wasm/JSC、runtime JIT、unchecked guest memory或降低x87精度。

## 执行路径统计

上一轮profile的运行期分区为IR/cache 53.44%、raw fallback 25.17%、API外层15.87%、
其他4.73%、x87 0.79%。只凭`steps_cached` self不能区分其内联的查表、分发、访存和FLAGS成本。
因此扩展`--op-stats`：保留原opcode统计，增加raw opcode、紧凑IR总数以及冷IR的操作/宽度/操作数分类。
计数仅在显式开启统计时更新；取指失败可能尚未记录opcode，其他fault指令可被记为尝试，
它们不能替代退休数。带统计的时间不用于性能收益。

完整统计跑到10 Draw / 9 Present / 8上传，共2,290,446,185条指令：

| 路径 | 动态数量 |
| --- | ---: |
| 已命中IR | 1,905,236,096 |
| 其中紧凑IR | 1,676,659,784 |
| 其中冷IR | 228,576,312 |
| raw fallback | 385,210,089 |

冷路径主要包括MOV I32内存←立即数21.62M、TEST I16寄存器20.24M、ADD I8内存17.29M、
CMP I32内存/寄存器13.33M、MOV I8寄存器12.94M、CMP I16内存/立即数12.56M，
以及CMP I32寄存器/内存9.35M等。不能把聚合x86 opcode数量当作紧凑IR覆盖。
raw仍有MOV 8B 49.41M、C1移位42.31M、D3移位12.16M、F7组22.04M、
0F组55.65M。现有guest DLL没有安装模块IR，但这些数量尚未按guest PC/模块归因，
不能断言所有普通MOV fallback都来自DLL。
记录：`wire-out/perf-ir-2-path-stats.txt`。

## 保留的实现

- bind把已验证v3记录的GPR8/16算术、窄寄存器MOV/CONST与到I32的扩展，以及
  I32寄存器/内存双向算术/比较降低到8B热操作。新增66个仅进程内opcode，
  持久v3、旧AOT兼容性、16B热条目与cache向量尺寸不变，无额外地址/常量表。
- 窄寄存器沿用原静态FLAGS证明和lazy恢复recipe；INC/DEC保留CF，ADC/SBB读取真实CF。
  内存算术只在defs/eager均为完整FLAGS时降低，保持checked read→FLAGS→checked write顺序，
  CMP/TEST不写内存，故障状态与代码写失效均不放宽。
- hint和IR尝试数在解释链退出时发布；fault包含失败的IR尝试，fallback前先发布已执行前缀。
  guest预算、协作分时、API、callback ESP和每条代码页generation检查保留。
- 现有寄存器型热opcode只使用已验证的GPR 0–7。专用GPR访问器向编译器暴露这个范围，
  避免每次6位字段访问8元素宿主数组时重复检查，不使用unchecked数组索引。
  磁盘字段仍6位，branch link不进入GPR访问器，通用64-vreg bank仍未完成。
- raw前缀扫描消费已经checked执行读取的opcode，去掉同地址的第二次fetch；
  checked fetch_u8/u16/u32可编译期内联。不提前读操作数，不预读跨页，也不缓存guest裸指针。

编码与不变量详见[IR设计](ir-aot-design.md)。

普通/trace/stats解释循环的const specialization试验即使修正了LLVM outlining，
两对长启动仍约慢6%，已撤回。只有新增紧凑操作时，两对吞吐收益约3%；
继续改checked fetch后约7–8%，不能把新增opcode本身宣传成全部收益。
相同代码的ThinLTO+单codegen unit试验中位吞吐只约+0.5%，单对方向不一致，
没有保留release构建配置改动。结果分别在`inline-compare`、`scalar-compare`、
`fetch-compare`、`lto-compare`，均属于诊断阶段，不替代最终复测。

## 用户要求重抓的完整 profile

10:26冻结版 `/private/tmp/wire-perf-ir-profile` 包含上述窄/内存操作、checked fetch与本地hint/计数，
尚未加入GPR范围访问器。使用`sample PID 50 2 -mayDie`，实际覆盖进程整个启动到9 Present。
trace/stats关闭，完整API日志与AppKit开启，无并发build/test或截图。
2,289,863,457条指令、3,528,840次API、10 Draw / 9 Present / 8上传，终态与基线相同。
采样下29.248秒、首Present19.241秒，不用于收益计算。

仅保留最外层run_loop及其后代，以最内层run_loop分类，递归guest callback不重复累计：

| 运行期分区 | 样本 | 占比 |
| --- | ---: | ---: |
| IR/cache | 6,951 | 55.35% |
| raw fallback（除x87） | 2,815 | 22.41% |
| CPU stepping外API | 2,242 | 17.85% |
| 外层调度与其他 | 457 | 3.64% |
| 软件x87 | 94 | 0.75% |

分母12,559个运行期主线程样本，包含阻塞样本，不是硬件CPU cycles。
API分区内的`std::thread::sleep / __semwait_signal`占1,175（9.36%），
来自`Sched::switch_away`的guest等待；不能把它当作API忙算，也不通过缩短guest deadline制造收益。

| 函数self time | 样本 | 运行期占比 |
| --- | ---: | ---: |
| steps_cached | 4,823 | 38.40% |
| step_x86 | 1,175 | 9.36% |
| access_ptr | 825 | 6.57% |
| PendingFlags::compute | 486 | 3.87% |
| fetch8 | 407 | 3.24% |
| modrm | 342 | 2.72% |
| run_loop | 341 | 2.72% |

上述self值互不重叠；inclusive不能相加。冷`execute_ir` inclusive已降至4.63%，
日志2.37%、UI poll0.84%、D3D9 draw0.87%、x870.75%。
`api::dispatch`的67.66% inclusive含大量guest WndProc执行，不能据此把主瓶颈归为Win32 API。

反汇编确认`steps_cached`内有公共成功收尾（+3444）、cache页代数/数组定位（+216）、
typed memory分发（+2752）和GPR数组bounds分支。采样器把多个PC合为一条节点，
不能从该节点的权重推断这些单个指令各占多少；热点依据足以优先优化解释循环和raw decode，
不足以支持关闭generation或访存权限检查。

原始sample、运行报告：`wire-out/perf-ir-2/after-profile/run-0/`；
可悬停查看完整符号的火焰图、self/inclusive排序与JSON：`after-profile-summary/`。
`tools/cpu_profile.py summarize`现自动输出self及去重后的inclusive函数热点，排除PE/IR装载与报告。

最终冻结版在 **10:37** 又做一次相同条件的完整采样（含GPR访问器）：
2,287,820,217条指令、3,525,078次API、10 Draw / 9 Present / 8上传，终态再次相同。
28.895秒、首Present18.725秒仍为带采样器时间，不放入下面的收益表。
12,427个运行期样本的结论与10:26采样一致：

| 最终版运行期分区 | 样本 | 占比 |
| --- | ---: | ---: |
| IR/cache | 6,879 | 55.35% |
| raw fallback（除x87） | 2,843 | 22.88% |
| CPU stepping外API | 2,165 | 17.42% |
| 外层调度与其他 | 446 | 3.59% |
| 软件x87 | 94 | 0.76% |

最终self热点：`steps_cached` **37.65%**、`step_x86` **10.05%**、
`access_ptr` **6.78%**、`PendingFlags::compute` **3.96%**、`fetch8` **3.13%**、
`modrm` **2.80%**。内联的访存/标志逻辑也在`steps_cached`等函数的self内，
不能把这些函数名占比解释为该子系统的全部成本。sleep inclusive约9.91%，属于等待；
日志inclusive2.12%、D3D9 draw0.88%、UI poll0.75%。
所以继续优先解释循环和decode覆盖，现阶段单独优化x87或图形不是主要收益来源。

最新sample与报告在`wire-out/perf-ir-2/final-profile/run-0/`，
最新火焰图和完整热点表在`wire-out/perf-ir-2/final-profile-summary/`。

## 无采样器的最终三对完整启动

最终冻结版 `/private/tmp/wire-perf-ir-final`，默认release配置（无ThinLTO改动）。
同一完整游戏副本、v3 IR、完整API日志与AppKit，trace/stats/采样/截图关闭，
最大预算100亿、成功Present阈值8；只在guest callback返回后停止，实际每次9 Present。
AB / BA / AB顺序，不与编译、测试或其他游戏运行并发。

| 对 | 基线首Present秒 | 新版首Present秒 | 基线测量段秒 | 新版测量段秒 | 基线进程CPU秒 | 新版进程CPU秒 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1（AB） | 19.330 | 18.032 | 29.966 | 27.830 | 27.029 | 24.969 |
| 2（BA） | 19.078 | 18.268 | 29.804 | 28.040 | 26.916 | 25.110 |
| 3（AB） | 19.039 | 17.981 | 29.657 | 27.746 | 26.824 | 24.851 |

中位数：首Present **19.078→18.032秒（−5.48%）**，测量段 **29.804→27.830秒（−6.62%）**，
全进程CPU **26.916→24.969秒（−7.23%）**，吞吐 **76.70→82.14 MIPS（+7.09%）**。
三对方向一致，六次全部GPR/EIP/EFLAGS与10 Draw / 9 Present / 8上传相同。
退休数2,285,943,630…2,286,923,936，API数3,519,371…3,522,175；guest timer/调度导致轻微差异，
不能称为严格同指令序列。没有把观测开销、等待缩短、资源跳过或错误退出当成收益。
这是本机启动负载的小幅收益，仍需约18秒首提交，性能问题尚未解决，也不代表稳定游戏帧率。
无法把总收益精确归因于其中某一条mask或计数改动。

完整数据：`wire-out/perf-ir-2/final-compare/compare.json`与六份逐次报告/API日志。
SHA-256：

```text
baseline 24d6df21b66d33a4e7a21c28c99efcf7fc6e27de32851ab99f268aa9b21bf899
final    ee684d0eaa8727bcd6a0bfbc65fab2d278e8588c39b01189b29118f4153262bb
10:26 profile 03348df9297dfcb0e5d87f3fc754b327b408b330faacef2ed648ffcb480734f2
```

## 正确性与接续

完整release **133项**通过：3 unit、36 Unicorn differential、5 FPU、22 IR、1 Wasm、66 Win32。
22项IR另以debug执行，检查GPR范围断言；release Clippy all-targets与fmt通过。
新增覆盖所有窄算术形式的高字节/别名/CF和逐预算恢复、窄MOV/扩展、
I32双向内存算术的FS/GS/SIB/回绕与read/write/guard/跨页fault、RMW自修改失效，
以及trace/stats路径、尝试数发布、带前缀的execute-only/取指fault。
错误的6位GPR字段必须在bind前被拒绝；没有用截断来接受非法持久记录。

单独以基线与更新后的`wire-out/Wire.app/Contents/MacOS/wire`捕获真实CPU帧，
两次均正常达到10 Draw / 9 Present / 8上传，GPR/EIP/EFLAGS一致。
`frame-000001/000005.ppm`黑场、`frame-000002.ppm`提示页、`frame-000008.ppm` STAFF页，
四份PPM均与基线**逐字节相同**。提示页/STAFF的SHA-256分别为
`0b00fa03e72f4f38ffa2f4281ad0d2eb8075ef978a602c7523dda9aa846ab6c4`和
`469fceaa2f673626f9abaf5ca72b78c49fde8892ff0c700d5303652b1ad498c5`。
捕获运行不参与性能表；产物在`wire-out/perf-ir-2/pixels-before/`和`pixels-after/`。
本地app binary已同步最终冻结版。

首要剩余热点是紧凑IR分发、每条边界/查表与checked访存，随后是仍需变长解码的385M fallback。
下一轮应分别量化普通fallback的guest PC/模块归属和移位等尚未AOT覆盖的形式；
模块IR需要源文件绑定、重定位、卸载/重映射和IAT hook等完整证明，不能只为了覆盖率直接启用。
FLAGS只占部分成本，继续保持极保守静态消除；图形/音频/API功能扩展仍暂停，M2仍未验收。

## 复现

冻结相应binary之后：

```sh
python3 tools/cpu_profile.py compare \
  'wire-out/game-runtime/王様恋愛【体験版】.exe' \
  --before /private/tmp/wire-perf-startup-final --after /private/tmp/wire-perf-ir-final \
  --ir wire-out/game-v3.wir --max-instr 10000000000 --stop-after-presents 8 \
  --pairs 3 --out wire-out/perf-ir-2/replay-compare

python3 tools/cpu_profile.py capture \
  'wire-out/game-runtime/王様恋愛【体験版】.exe' \
  --binary /private/tmp/wire-perf-ir-final --ir wire-out/game-v3.wir \
  --runs 1 --sample-seconds 50 --sample-interval-ms 2 \
  --max-instr 10000000000 --stop-after-presents 8 \
  --out wire-out/perf-ir-2/replay-profile

python3 tools/cpu_profile.py summarize \
  wire-out/perf-ir-2/replay-profile/run-0/sample.txt \
  --out wire-out/perf-ir-2/replay-summary
```

游戏运行副本需完整资源目录；存在时不重复clone。所有游戏资源、IR、日志、profile、帧都留在忽略目录，
不提交资产。若副本被外部清理，应先恢复APFS clone，再从头测完整AB/BA，不能将中断运行当成成功样本。
