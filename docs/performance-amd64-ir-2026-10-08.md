# AMD64 定长 IR 与解释执行性能（2026-10-08）

按用户要求暂停 Alice 功能扩展，先优化 AMD64 CPU 并复测。PE32 早已使用安装期 IR，
AMD64 bring-up 此前仍逐条解码；本轮填补常见标量操作的执行 IR 缺口。
运行时只产生解释器的数据记录，没有宿主 CPU JIT；FLAGS 全部 eager。
格式、故障提交和安装期 AOT 的当前边界见 [IR设计](ir-aot-design.md)。

## 冻结基线与改动

Apple M4、release默认优化、调试符号保留、相同资产和GPU后端。
测试与采样不和计时重放并行。三份冻结文件的SHA-256：

```text
before     4be16db181bcb3ae5cc5a0e8406aa7556663ba33f45225a6c537662f6ba09f84
cache-only 941d70b989dd67a1ec19bca16bcc62031c7d8426f198afeb8a4501689978c84f
scalar-IR  a5eca4a67869fce298130eafa5bff72acb74b4c8dcf8c328b88791cf77233da6
```

文件位于 `wire-out/wire-amd64-{before,cache-only,ir}-2026-10-08`。
before 为 `1595597` 对应的窗口状态机阶段；cache-only 已加入页内批量取指、iced记录缓存、
单页checked读写一次页查找，但仍走原operand解释器并完整clone CPU。
scalar-IR 在其上加入24B typed操作、独立cold fallback与标量故障回滚，
每次执行避免通用operand解析以及复制XMM/FP80。不是只把iced记录重命名为IR。

## 采样证据

旧版真实Alice前200M work的8秒sample保存在 `before-profile/`。
带采样的51.959秒不能拿来作无profiler AB基线。
新版 `tools/cpu_profile.py summarize` 支持 Process64::run，并汇总同函数的不同PC与递归栈：
6,742个运行期样本中，Cpu64::decode inclusive **5,206 / 77.22%**；
read_access 43.95%、check 23.05%、iced decode_out_ptr 23.05%。inclusive项互相重叠，不能相加。
最初只计两个最大的decode栈得到约64%，完整汇总后修正为77.22%。
旧实现每增加一个字节都重新建立iced decoder，普通长指令被反复解析。

## 确定性负载矩阵

`tools/amd64_perf.py` 生成原创PE32+，每类约50M退休指令，三轮交替
before/cache/IR、IR/cache/before、before/cache/IR；统计与profiler关闭。
45次计时共约22.5亿退休指令，另做各类一次不参与计时的IR覆盖诊断。
所有版本的返回校验值与精确退休数一致。MIPS以下使用全进程wall，含装载和结束报告。

| 负载 | before MIPS | cache-only MIPS | scalar-IR MIPS | IR / cache |
| --- | ---: | ---: | ---: | ---: |
| 寄存器/FLAGS/CMOV | 7.42 | 28.59 | 77.67 | 2.72× |
| checked内存 | 6.59 | 24.05 | 47.30 | 1.97× |
| SSE与标量混合 | 6.06 | 22.62 | 30.08 | 1.33× |
| 16,384条指令/约53KiB代码 | 7.57 | 22.40 | 32.80 | 1.46× |
| 间接CALL/RET/栈 | 7.87 | 29.15 | 68.03 | 2.33× |

SSE混合负载约一半仍走cold执行器；大代码负载刻意超过直接映射缓存的无冲突足迹，
出现大量重复翻译，结果没有只挑紧凑循环。原始报告、计时与checksum在
`wire-out/perf-amd64-2026-10-08/matrix/compare.json` 和各子目录。
这组结果用于分离CPU热路径收益，不能替代真实游戏启动结果。

## 真实 Alice 启动，前200M work

相同隔离资产、--startup、真实MoltenVK后端，窗口呈现关闭；CPU统计和采样关闭。
三轮相反顺序，每次都到相同200M预算终点，没有添加或补齐API以改变启动功能。

| 轮次 | before 秒 | cache-only 秒 | scalar-IR 秒 |
| --- | ---: | ---: | ---: |
| 1，before/cache/IR | 51.137 | 13.535 | 10.541 |
| 2，IR/cache/before | 50.775 | 13.675 | 10.544 |
| 3，before/cache/IR | 51.164 | 13.485 | 10.516 |
| 中位数 | 51.137 | 13.535 | 10.541 |

退休数约199.6M，全进程MIPS中位数 **3.90 → 14.75 → 18.94**；
总延迟降低79.4%，其中IR相对cache-only另降低22.1%。这不是仅IR单项带来4.85倍：
最主要的第一段收益来自停止反复解码，IR额外减少operand分发及CPU上下文复制。
进程CPU中位数51.006 / 13.448 / 10.480秒，与wall趋势一致。

真实计时器、worker和API轮询使路径不同：before约331.6K已完成API，cache/IR约380.8–381.6K，
退休数和终点RIP略有不同。因此不宣称同一指令序列或寄存器终态完全相同。
本段主要覆盖Unity/Mono早期初始化，尚未覆盖程序集完整加载或正式窗口；不能把它当首帧时延。
数据在 `alice-200m/compare.json`。单独coverage重放中 IR=180,720,111、fallback=18,897,148，
即完成的CPU work约 **90.53%** 走标量IR；翻译17,125,855次、全局失效22,660次。
这也暴露了代码缓存仍有直接映射冲突与保守全局失效的开销。

重新8秒采样、共6,038个运行期样本：decode inclusive **12.42%**，
Memory::check self/inclusive **27.16%**，ScalarIr::step self13.05%，read_access self10.52%，
Process64::run self11.21%。不同版本相同时间窗口会覆盖不同guest阶段，比例不能直接当每条指令成本。
重抓结果保存在 `ir-profile/`、`ir-summary/`；仍不参与前述无profiler计时。

## 同代码页的EXECUTE证明缓存

依据新profile再缓存最近一个已成功检查的代码页，只用于已验证的单页translation hit。
完整PC/epoch仍逐条匹配；任何保护、映射、代码修改都会清除该证明，guard重置也在此路径内。
跨页指令仍完整检查两页，数据读写检查不省略，没有裸宿主页指针。
同页已知范围加上这些失效规则证明权限有效；这不同于把checked memory整个关闭。
最终版本SHA-256 `f8a5f16a3455bf0c8a40f7d66920f4224f00c53e42b7600636eb84e95aa1b005`，
冻结为 `wire-out/wire-amd64-ir-final-2026-10-08`。

独立三轮200M work对照，以前一版scalar-IR作基线：

| 轮次 | scalar-IR 秒 | 最终版 秒 |
| --- | ---: | ---: |
| 1，前/后 | 10.482 | 8.542 |
| 2，后/前 | 10.418 | 8.069 |
| 3，前/后 | 10.519 | 8.124 |
| 中位数 | 10.482 | 8.124 |

延迟降低22.49%，全进程MIPS **19.04 → 24.57**；进程CPU **10.416 → 8.059秒**。
最终版约383.1–383.3K API，前版约381.5K，仍存在真实计时器差异。
这些是另一组对照，不能把两组中位数拼成同一对严格同路径的计时。
结果在 `execute-page-200m/compare.json`，先前的矩阵表明确属于scalar-IR阶段。

最终版再采8秒（重放预算400M，避免进程提前结束）：6,665个运行期样本中，
ScalarIr::step self18.15%、read_access self16.62%、Process64::run self13.61%、
cached_ir self8.31%，Memory::check self/inclusive从27.16%降到1.55%。decode inclusive15.41%，
仍有cold miss和全局失效重译；相比上轮的占比升高不能直接推断绝对变慢。
后续优先测代码缓存冲突/失效、checked数据访问和外层调度，FLAGS丢弃规则继续保持保守。
证据在 `final-profile/`、`final-summary/`，不计入AB时间。

## 完整启动到已知 CoCreateGuid 边界

再用一组完整AB重放覆盖程序集加载、domain重置、PhysX选择、x87和窗口定位后的路径。
预算2.5B，但终点按**精确的既有CoCreateGuid未实现错误**验证，不是预算耗尽，
不添加API实现，不忽略错误。两版都停在RIP=`0x7ffe00001670`、EXE entry阶段；
HWND=`0x7ffd00630000`、314×211、visible=false，仍没有guest Draw/Present或正式窗口。

| 指标 | before | 最终IR版 |
| --- | ---: | ---: |
| 全进程wall秒 | 536.798 | 54.766 |
| 全进程CPU秒 | 536.044 | 54.605 |
| 退休guest指令 | 2,068,704,475 | 1,361,446,725 |
| work | 2,072,496,128 | 1,364,600,916 |
| 已完成API | 3,666,512 | 3,075,194 |
| 全进程MIPS | 3.85 | 24.86 |
| Player.log程序集加载秒 | 183.887 | 18.898 |
| Player.log domain重置秒 | 3.611 | 0.417 |

这组实际启动到相同兼容边界的时间降低89.8%，约9.8倍；但退休数也减少34.2%，
不是同一指令序列，更不能称为纯IR分发提速9.8倍。真实计时器和worker的guest轮询
随执行速度改变；没有删除任何guest操作或用宿主等待替换这些循环。
按退休数/时间得到的整段吞吐约6.45倍。单组完整AB提供长路径方向性证据，
稳定性证据来自前述多轮确定性矩阵及真实200M段；此终点仍不是首帧或完整可玩。

完整数据、原命令、report和分别保存的Player.log在 `alice-full/compare.json` 与各子目录，
后续短profile覆盖runtime的Player.log不会丢失这两份完整证据。
优化后当前功能边界仍为CoCreateGuid，按本轮范围暂停继续补API。

## 正确性

完整release **290 passed、5 ignored**，Clippy all-targets `-D warnings`、fmt和diff检查通过。
新测试覆盖高指针、四种整数宽度的内存ALU、16种Jcc/CMOV/SETcc×64组FLAGS，
共3,272次独立Unicorn逐指令对照；另覆盖false CMOV fault、POP [RSP]时序、跨页写故障原子性、
execute-only取指、代码写/保护/同页guard重新启用、remap与decommit/recommit失效。
SSE/x87/REP、Win64线程/context、回调和既有PE32回归均通过，ignored GPU测试未重复运行，
本轮未修改图形后端。日志为 `/private/tmp/wire-amd64-ir-final-{tests,clippy}.txt`。

## 复现

```sh
python3 tools/amd64_perf.py matrix \
  --before wire-out/wire-amd64-before-2026-10-08 \
  --cached wire-out/wire-amd64-cache-only-2026-10-08 \
  --after wire-out/wire-amd64-ir-2026-10-08 \
  --out wire-out/perf-amd64-2026-10-08/matrix --work 50000000 --pairs 3

python3 tools/amd64_perf.py game \
  --before wire-out/wire-amd64-before-2026-10-08 \
  --cached wire-out/wire-amd64-cache-only-2026-10-08 \
  --after wire-out/wire-amd64-ir-2026-10-08 \
  --out wire-out/perf-amd64-2026-10-08/alice-200m --work 200000000 --pairs 3 \
  --cwd wire-out/games64/alice/runtime \
  --graphics-backend wire-out/libwire_graphics.dylib \
  --moltenvk /private/tmp/wire-moltenvk-1.4.2/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib

python3 tools/amd64_perf.py game \
  --before wire-out/wire-amd64-before-2026-10-08 \
  --after wire-out/wire-amd64-ir-final-2026-10-08 \
  --out wire-out/perf-amd64-2026-10-08/alice-full --work 2500000000 --pairs 1 \
  --skip-coverage \
  --expected-error 'ole32.dll!CoCreateGuid: unimplemented Win64 import: ole32.dll!CoCreateGuid' \
  --player-log Users/wire/AppData/LocalLow/TukiMahiru/AliceToPapaADV_1-0-0/Player.log \
  --cwd wire-out/games64/alice/runtime \
  --graphics-backend wire-out/libwire_graphics.dylib \
  --moltenvk /private/tmp/wire-moltenvk-1.4.2/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib
```

`--cpu-stats` 只用于单独覆盖重放；它统计完成的IR/fallback work、翻译与失效次数，
不含 Process64 的API/idle调度work。startup 的 retired/work/MIPS 另有输出，
MIPS按退休guest指令而不是IR数量计算。Wasm/JSC路径没有启用。
