# 真实游戏长启动性能优化（2026-10-06）

按用户要求，真实 D3D9 图像与 macOS 呈现检查点完成后，暂停功能扩展，
优先降低启动延迟。解释执行仍为主路径，没有启用 Wasm/JSC 或 runtime JIT。
本轮使用完整游戏、相同 AOT 文件和真实资源解码/绘制，测量到提示页及 STAFF 画面。

## 测量范围

Apple M4、macOS 26.5.1，release 默认编译配置，完整 API 日志开启；
CPU trace/opcode stats 关闭，AppKit 呈现开启。游戏运行副本与资产不改动。
同一 EXE 和 `wire-out/game-v3.wir`，最大预算100亿条，成功 Present 阈值8。
`--stop-after-presents` 只在外层、guest callback 返回之后停下，不能中断宿主回调栈；
因此此游戏每次实际完成 **10 Draw / 9 Present / 8 AppKit 帧上传**。
终点有独立 RunOutcome，真实 guest 退出/错误保留其含义，不能算测量成功。

每次执行约 **22.86亿 guest 指令、352万 API**，比之前插件失败阶段的约24M样本大约两阶数量级。
各次 guest 计时/线程轮询略有差异，不能声称严格同指令序列。
首 Draw/Present 从 PE entry 开始计时；进程 wall/CPU 包含 PE/IR 装载、绑定、结束报告。
Present 是 CPU 真值图像提交，AppKit 上传与实际屏幕扫描输出仍是不同边界。
没有把空白 Present、错误退出、资源跳过或降低画质算作收益。

## 完整启动 profile

基线是 `dbfcb0b` 加测量计时/终点的冻结 binary：`/private/tmp/wire-perf-baseline`。
使用 `/usr/bin/sample PID 50 2 -mayDie`，从进程启动采集50秒、间隔2ms。
该次最终运行63.641秒、22.892亿指令，首 Present47.083秒；带采样器的时间不用于加速比。

只保留最外层 `Cpu::run_loop` 及其后代，排除装载、IR绑定和报告，
按叶权重去除递归 guest callback 的重复累计，并以最内层 run_loop 分类。
宿主 API 外层调用 callback 的调度开销不能全部计入 USER32 API 本身。
分母20,377个运行期主线程样本，包括阻塞/睡眠样本，不能当作纯 CPU cycles。

| 运行期分类 | 样本 | 占比 |
| --- | ---: | ---: |
| IR execution/cache | 10,874 | 53.36% |
| 原始 x86 fallback（除x87） | 3,333 | 16.36% |
| 软件 x87 | 190 | 0.93% |
| CPU stepping 外的 API | 3,648 | 17.90% |
| 外层调度及其他 | 2,332 | 11.44% |

`ApiLogger::log_call` inclusive 为1,341样本（6.58%），
`wire_ui_poll` 为1,691（8.30%，包含等待），不能与分区表相加。
大量初始化由 WndProc callback 执行；旧实现 callback 强制每条指令返回 run_loop。
软件 x87 已不是本阶段的主要瓶颈，不能继续用旧的短阶段浮点热点推断此次启动。

## 实施

- Callback typed IR 可连续执行；每条 guest 指令后检查所属线程的 callback ESP ceiling，
  即使下一条会降低ESP也必须先报告非局部转移。返回 sentinel/API/cold/fallback、
  精确预算、1,048,576条的协作分时和代码页 generation 边界全部保留。
- Bind 时将已验证 v3 的常见32位 PUSH/POP、立即数 PUSH、直接/寄存器 CALL、
  寄存器 JMP、RET、LEAVE 降低为8个进程内 typed opcode。
  减少64B通用记录复制/operand分发及外层往返，调用和返回可继续紧凑链。
  磁盘v3、8B指令/16B热条目、旧文件兼容性与总cache向量尺寸不变。
- 通用冷分发只解析一次操作枚举。CALL_NATIVE 的真实IAT读取、hook判断、
  FLAGS恢复和统计仍走原路径。
- 完整 API trace 直接写入复用的最近64条字符串，按stub索引缓存名称；
  保留实时频率、UNIMPLEMENTED标记、参数/返回/备注及崩溃上下文。
  移除每次参数的 Vec<String>/join、名称clone及备注拼接分配，输出缓冲64KiB。
- AppKit poll 保留 NSEvent、CFRunLoop ready sources 和 updateWindows，
  将每次CFRunLoop的1ms等待改成非阻塞；guest wait仍按原deadline工作。
  可见游戏在主线程使用公开 pthread QoS 请求 `USER_INITIATED`，匹配前台启动工作。
  这是调度请求，不能声称固定在P core；没有绑核或调整系统全局设置。

FLAGS 静态证明没有放宽。所有恢复边界、checked memory、代码写失效、
x87 binary128/FP80精度、RC和异常逻辑保持原要求。
这不是纯 IR 分发单项收益：最终版本同时包括日志、UI polling 与调度优化。

## 无 profiler 的最终长启动对照

最终冻结文件 `/private/tmp/wire-perf-startup-final`，AB / BA / AB，
不与编译、测试、采样或画面捕获并发。完整数据：
`wire-out/perf-startup/final-validated/compare.json` 与逐次 report/api.log。

| 对（顺序） | 基线首Present秒 | 新版首Present秒 | 基线测量段秒 | 新版测量段秒 | 基线进程CPU秒 | 新版进程CPU秒 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1（AB） | 25.458 | 19.119 | 37.688 | 29.675 | 32.283 | 26.795 |
| 2（BA） | 24.869 | 18.996 | 36.915 | 29.513 | 31.935 | 26.587 |
| 3（AB） | 25.031 | 18.965 | 37.191 | 29.534 | 32.160 | 26.708 |

中位数：首 Present **25.031→18.996秒（−24.1%）**，
整个测量段 **37.191→29.534秒（−20.6%）**，
全进程 CPU **32.160→26.708秒（−17.0%）**，
guest MIPS **61.47→77.40（+25.9%）**。
三组每组都提速，六次的全部GPR/EIP/EFLAGS终态一致；
实际退休数2,285,993,341…2,287,695,156、API数3,519,594…3,523,597。
六次均到同一回调返回后的测量边界、10 Draw / 9 Present / 8上传。
在本机这个长启动负载上已得到稳定收益，但仍需近19秒首提交，性能问题没有解决。
此结果不代表持续游戏帧率，也没有外推到iOS或其他游戏。

冻结 binary SHA-256：

```text
before 5e35dd09f0d987d9259fc5c70aab0586d58190ef7fbbd12aec30007e77836841
after  24d6df21b66d33a4e7a21c28c99efcf7fc6e27de32851ab99f268aa9b21bf899
```

初步只改 callback chaining 和日志时，两对中位数没有稳定收益，
同一基线首 Present 出现39.816和25.235秒的明显波动。
因此不将第一组约14%的差异当成已证明的稳定优化，也不直接使用用户观察的1分钟计算加速比。

## 正确性与后续

补抓了两份覆盖相同9-Present终点的完整启动sample，基线15,438、新版13,216个运行期样本：

| 运行期分类 | 完整基线 | 完整新版 |
| --- | ---: | ---: |
| IR execution/cache | 47.34% | 53.44% |
| 原始 x86 fallback（除x87） | 21.09% | 25.17% |
| 软件 x87 | 未采到 | 0.79% |
| CPU stepping 外的 API | 20.75% | 15.87% |
| 外层调度及其他 | 10.81% | 4.73% |

`log_call` inclusive占比4.75%→1.51%，`wire_ui_poll` 7.14%→0.89%，
通用 `execute_ir` 11.61%→8.61%。IR/cache与fallback的相对占比上升，
主要反映其他成本被减少，不能据此称它们变慢；inclusive数值不能相加。
新版这份profile同时捕获CPU帧并尝试窗口读取，不参与前面的计时对照。
剩余主要开销仍在解释器，下一轮应分解 `steps_cached` self和decode/checked访问，
继续扩大窄算术/内存与fallback的紧凑执行覆盖，而不是恢复功能扩展。

像素复验：新版捕获 `after-profile/frames/frame-000002.ppm`（提示页）与
`frame-000008.ppm`（STAFF页），两者和渲染检查点 `game-graphics/d3d-z/frames`
的同名文件 **逐字节相同**；另外两张黑场也一致。已通过CUA读取到原生
“王様恋愛 Ver1.00”窗口截图，看到真实提示页，确认非阻塞run-loop仍能显示图像并响应AX读取。
PNG只由原始PPM转码，未生成或替换游戏图片。
单独延长的可见窗口复验运行59.941秒、47.862亿指令，
17 Draw / 16 Present / 15 AppKit上传，首Present19.198秒，正常到达测量终点。
这次用于观察窗口，未纳入三对启动加速比；记录在 `visible-check-report.txt`。

完整 release **128项**通过：3 unit、36 Unicorn CPU differential、5 FPU、17 IR、1 Wasm、66 Win32。
新增逐预算 raw/IR 对照覆盖 callback 正常/嵌套返回、ESP跨边界后立即降低、
PUSH/POP/CALL/RET/LEAVE fault与ESP别名；测量终点必须等callback返回、恢复FLAGS，
真实退出优先于测量成功。原自修改、权限、IAT hook、线程、SEH和逐像素图形回归保留。
release Clippy all-targets、fmt、diff检查与Python语法检查通过。

后续继续性能路线，重点分解剩余 IR/cache self time、通用内存/窄算术和 raw fallback。
先在真实初始化/资源解码负载证明收益，再考虑扩大 lowering 或融合，
不能放宽 fault/FLAGS/线程边界。音频、IME、额外图形功能暂停；M2仍未验收。

## 复现

冻结前后binary后：

```sh
python3 tools/cpu_profile.py compare \
  'wire-out/game-runtime/王様恋愛【体験版】.exe' \
  --before /private/tmp/wire-perf-baseline --after /private/tmp/wire-perf-startup-final \
  --ir wire-out/game-v3.wir --max-instr 10000000000 --stop-after-presents 8 \
  --pairs 3 --out wire-out/perf-startup/replay-compare

python3 tools/cpu_profile.py capture \
  'wire-out/game-runtime/王様恋愛【体験版】.exe' \
  --binary /private/tmp/wire-perf-startup-final --ir wire-out/game-v3.wir \
  --runs 1 --sample-seconds 50 --sample-interval-ms 2 \
  --max-instr 10000000000 --stop-after-presents 8 \
  --out wire-out/perf-startup/replay-profile

python3 tools/cpu_profile.py summarize \
  wire-out/perf-startup/replay-profile/run-0/sample.txt \
  --out wire-out/perf-startup/replay-summary
```

本机 logs、CPU帧、采样与游戏副本均在忽略目录，资产不提交。
完整重采样：`wire-out/perf-startup/baseline-full-profile/run-0/sample.txt`、
`after-profile/sample.txt`；汇总/火焰图在 `full-profile-summary/`。
