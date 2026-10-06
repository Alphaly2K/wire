# 纯寄存器静态序列的第一阶段优化（2026-10-06）

用户要求开始上一轮提出的IR优化，同时提供了64位Unity样本。
本轮继续以解释器为主路径，暂停API/图形功能扩展；64位样本检查见
[x86-64记录](x86-64-sample-2026-10-06.md)。没有引入JIT或外部源码。

## 实现与边界

在绑定已经安装期AOT预解码的v3记录后，用静态逆序扫描规划2–8条连续纯寄存器操作。
相同4KiB guest页、全部源字节/执行权限已验证，不包含访存、栈访问、CALL/native gateway、
中间分支、ESP/SP写入或fallback；直接Jcc/Jmp可以作为最后一条。
入口generation检查适用于整个序列；单host CPU协作调度和这些操作的无副作用性质，
保证内部没有代码写入、保护变更、线程切换或callback栈越界。
跨页、访存、API、失效和任意未识别入口仍进入原checked路径。

长度打包在16B热条目的next_slot高4位，低28位保留slot，不新增side table；
磁盘v3、64B冷记录和8Btyped操作不变，旧 `game-v3.wir`直接参与对照。
每条guest入口、GPR/subregister行为和完整FLAGS恢复信息仍独立保留。
预算/精确分时剩余量不足整个序列时拆回单条执行；trace/op-stats也逐条执行。
退休数量按原始guest计数，fault只计成功前缀；API/stub和callback继续在外层处理。

序列内算术不急于求所有FLAGS，但始终保存完整恢复recipe。
INC/DEC保留CF，ADC/SBB读取有效CF，所有预算/异常/线程保存/API/raw边界可恢复完整值。
静态dead-FLAGS集合没有扩大，也没有因为Jcc只读某几个位就丢掉其他位。
实现细节和正确性条件见[IR设计](ir-aot-design.md)。

这是块内批量执行的第一步：减少重复查表、页代数检查和公共收尾，
**每条热操作的opcode分发仍在**。没有完成专用CMP/TEST+Jcc融合opcode、
常量传播、复制传播、guest指令删除、紧凑持久文件或完整512B vreg bank。
优化规划发生在绑定时；不能称为已实现持久IR上的全套AOT优化器。

## 三对完整启动复测

基线为 `2eea900`，冻结文件 `/private/tmp/wire-perf-ir-final`，SHA-256：
`ee684d0eaa8727bcd6a0bfbc65fab2d278e8588c39b01189b29118f4153262bb`。

新版冻结文件 `/private/tmp/wire-perf-scalar-lazy`，SHA-256：
`03194072f1d1b819a3bde7531c06b37b9b1aebe47b8805fa132be352e4dce1ed`。

同一游戏运行副本、同一v3 IR、release默认构建、完整API日志与AppKit开启；
没有sample、trace、op-stats、截图或并发build/test，顺序AB / BA / AB。
每次成功运行至提示页及STAFF画面，10 Draw / 9 Present / 8上传。
不把此前更慢基线、短循环或带profiler的时间放入收益计算。

| 对 | 基线首Present/s | 新版首Present/s | 基线测量段/s | 新版测量段/s | 基线进程CPU/s | 新版进程CPU/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 18.079 | 17.425 | 27.918 | 26.760 | 24.978 | 23.899 |
| 2 | 17.950 | 17.369 | 27.709 | 26.759 | 24.784 | 23.901 |
| 3 | 17.984 | 17.678 | 27.754 | 27.086 | 24.871 | 24.161 |

中位数首Present **17.984→17.425秒（−3.11%）**，测量段 **27.754→26.760秒（−3.58%）**，
全进程CPU **24.871→23.901秒（−3.90%）**，吞吐 **82.36→85.42 MIPS（+3.72%）**。
六次GPR/EIP/EFLAGS全部相同，均为正常的presentation benchmark终点。
退休数2,285,941,712…2,285,965,780，API3,519,315…3,519,471；
guest计时/调度略有变化，不能称为严格同指令序列或持续帧率结果。

第一轮只批量执行、仍按原eager mask计算FLAGS的一对试验为82.41→85.25 MIPS。
没有独立多对拆分FLAGS试验，因此不能把最终收益全部归因于FLAGS延迟计算。
最终第一对新版共297,257,275个run、749,412,434条guest参与批量执行，平均约2.52条/run；
约32.8%的退休指令被覆盖，减少约452M次重复查表/公共收尾。
总吞吐仍只增加约4%，说明这种批量执行不是大幅提速的证据；当前性能问题仍未解决。

复现产物：`wire-out/perf-scalar-runs/final-compare/compare.json`及六份report/API日志。

## 回归与接续

增加三组IR回归：CMP/TEST配全部16种Jcc、溢出/借位/完整FLAGS、所有预算和中间入口；
trace/stats精确计数；同host页内跨guest页的execute guard和后续内存fault；
绑定前后对run中间字节的修改。既有callback ESP、协作分时、原始fallback、
IAT hook、窄寄存器、访存故障/失效和x87回归继续保留。

完整release **136项通过**（3 unit、36 Unicorn差分、5 FPU、25 IR、1 Wasm、66 Win32）；
Clippy release all-targets、fmt、diff检查通过。
25项IR另跑debug通过，覆盖GPR字段范围和run长度断言。
另一次完整启动捕获的frame-000001/000002/000005/000008与上一轮最终版逐字节相同，
分别覆盖黑场、提示页和STAFF；捕获时间不用于性能收益。
记录在 `wire-out/perf-scalar-runs/pixels-after/`，对照为 `wire-out/perf-ir-2/pixels-after/frames/`。

下一步应按动态模式选择真正减少opcode分发次数的融合，再评估常量/复制/寻址简化。
任何删除guest操作的优化必须保留逐条恢复点和内部入口，并在预算/单步/故障时拆开执行。
还要按guest PC/模块归因raw fallback，避免只优化主EXE已有的IR而忽略DLL解释成本。
后续测量使用本轮冻结版作基线，继续完整启动多对对照，不以新的小样本夸大收益。

本轮收尾时用户将优先级改为修复三个32位应用（Rewrite、koikake、totsulover），
下一步先推进这些真实兼容阻塞；x86-64继续暂停，后续性能路线保留为接续方向。
