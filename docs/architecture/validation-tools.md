# 测试与性能工具边界

2026-10-19 用户调整验收要求：性能测试先按改动风险评估，不再把完整矩阵作为每批重构的前置条件。普通 API 声明迁移、低耦合 owner 封装、工具和文档整理，以所属领域的行为/ABI/故障回归及必要构建检查为主；不因编译后地址变化推测回退并反复扩样。改动 CPU 执行器、memory/cache、热网关、热点数据布局或实际绘制/启动路径时，按受影响负载选择少量有针对性的性能比较；较大阶段整合后再决定是否需要累计复核。保留历史严格验收及失败证据，但不将原 3%/5% 数值门槛继续作为普通结构改动的准入标准。已有定向检查通过且未产生新问题时直接继续；完整正确性矩阵和真实 native 检查也按实际影响范围执行。

2026-10-14：保留 `x64`、`win32` integration target，把已有测试按 CPU/SIMD、memory、kernel、files、modules、sync、USER、COM 和图形等实际领域组织到子模块。共享小型 PE/VM/调用 harness 位于 `tests/support/`，跨领域的 PE32 DLL builder 与 guest COM 调用也在此处；领域私有 helper 和测试一起移动。

完整函数名新增领域前缀，例如原 `amd64_integer_flags_and_rex_register_aliases_match_unicorn` 现在为 `cpu::amd64_integer_flags_and_rex_register_aliases_match_unicorn`。旧的函数名 substring 过滤继续可用，`--exact` 过滤需要新名称。完整映射保存在 `tests/test-name-map-2026-10-14.json`；R7 迁移时的 130 个 x64 测试与 109 个 Win32 测试逐名保持，后续新增用例不属于名称迁移项。5 个原始 guest Mono 观察测试留在 `x64.rs`，不涉及 native Mono。

```sh
cargo test --release --test x64 -- cpu::
cargo test --release --test win32 -- user::
cargo test --release --test x64 -- cpu::amd64_integer_flags_and_rex_register_aliases_match_unicorn --exact
```

`tools/perf/measurement.py` 只负责 child 的 wall/CPU/RSS、timeout、report、hash 和 AB/BA 顺序。架构脚本继续拥有 fixture、work、retired、结果和 API count 校验；不会把不同架构或模式混成一个吞吐量。旧报告字段保留，新增 `timed_out` 和测量库 hash；V2 另记录使用的 replay 源 hash。PE32 wall 窗口现在与公共库一致，不包含结果文本读取；macOS RSS 仍为 bytes，Linux 的 KiB 转成 bytes。

架构测量实现位于 `tools/perf/{pe32,amd64,v2}.py`，早期 AMD64 工具位于 `legacy_amd64.py`；原 `interpreter32_perf.py`、`interpreter_perf.py`、`interpreter_v2_perf.py`、`amd64_perf.py` 保留 CLI 与导入。builder/输入生成器集中在 fixture 包，内部采用相对导入，旧命令模块仍公开原 helper。三个正式协议的 input、measurement、launch、binary 字段保持；script/replay/audit hash 标识迁移后的实际源码。226 份生成输入、58 次命令兼容执行及包/自定义 audit 检查见 [测量入口验收](../reports/refactoring-perf-drivers-2026-10-19.md)。

`tools/perf/freeze.py` 保存一次已构建 binary、source diff、toolchain 和输入 hash。旧 `tools/freeze_interpreter.py` 委托此入口；默认包含跟踪文件及 src/tests/tools/examples/native 下的新源码，额外输入须用 `--include` 指定仓库相对文件。未跟踪的 docs 设计稿/PDF、IDE 元数据不自动进入快照。符号链接保存链接及其目标文本 hash，不复制目标素材；复现者需要另行提供该链接引用的外部输入。

```sh
python3 -B tools/freeze_interpreter.py --binary target/release/wire --out wire-out/new-frozen-interpreter
python3 -B -m unittest discover -s tools -p 'test_*.py'
```

PE32、AMD64 和 V2 的公开脚本入口保持不变，正式 fixture 位于 `tools/perf/fixtures/`。native/probe 和 inventory 的兼容入口仍保留；native source、FFI 和 third_party 构建路径没有搬动。本页描述当前工具边界，后续领域迁移仍需独立验收。

2026-10-15：PE32 和 AMD64 测量入口均接受可选 `--qos-clamp utility|background|maintenance`，通过 macOS taskpolicy 对每个被测 child 施加同样的调度上限；默认不改变 QoS。manifest 明确记录模式与 taskpolicy hash，逐轮 command 保留 wrapper，不把受控调度结果混成默认前台吞吐量。

PE32 headless 整数 fixture 的同二进制 700M 对照曾出现 2.31–7.28s 两档，而 IR/fallback/scalar-run 计数完全相同。开启采样后没有复现慢档；原生 UI 初始化另有显式 USER_INITIATED QoS。因此调查必须保持 headless/UI、采样和调度条件一致，不能沿用 capture 默认 UI 的结果替代 headless 计时，也不能直接认定为代码回退。utility 同二进制三对的差异约 +0.70%/−1.88%/+2.49%，该受控条件可用于后续成对调查；默认条件的失败、离散轮次仍全部保留。

`tools/perf/summarize.py` 汇总上述三个正式入口的 manifest/compare.json，兼容 `binary` 与 `version` 标签。只合并架构、raw/IR、输入及预期 work/retired、QoS、日志/cache 协议和测量源码 hash 一致的样本；不同规模分别出行。每轮保留原 source/pair，输出样本数、min/median/max、成对变化、CPU/RSS 差异和同二进制 control 的漂移；没有跨负载总分，也不自动作接入决定。

工具拒绝重复目录/轮次、不完整 pair、未验证或 timeout 的样本、同标签不同 binary hash 和非 timing 报告；`--out` 必须是新文件，已有证据不会覆盖。它核对记录的哈希与校验标志，原 fixture 仍负责 guest 结果验证，冻结工具仍负责保存源码与 binary。汇总无需启动 guest 或重跑性能测试。

2026-10-16：三个测量入口新增可选 `--stable-launch`。通过固定的 zsh exec 程序把解释器 argv0 设为 `wire`，启动参数作为独立 argv 原样转交；每次 child 的 host cwd 为本轮输出目录，PE32 的日志参数固定为 `logs`，fixture 与 IR 使用同一绝对路径。QoS wrapper 继续在外层，语义、计数、默认 case、API trace 格式化和旧默认启动方式保持。测量包含小型 shell 的启动成本，before/after/control 使用同一协议。

新增模式源于同一机器码哈希副本在旧协议中出现约 2.3s/7.1s 的重复差异；固定 argv/log 参数后，在同一 utility QoS 中两副本接近。启动参数与其引发的分配布局是调查线索，尚未证明具体 allocator/cache 因果关系。使用该模式需重新测量，不能将旧协议的离散样本静默移除或混入新协议。manifest 保存 launch 模式、argv0、cwd/log 策略与 shell hash；离线汇总按完整 launch 字段分组。调查、工具测试与跨架构预检见 [记录](../reports/refactoring-launch-protocol-2026-10-16.md)。

2026-10-18：增加可选 `--fixed-executable-slot`，要求显式 stable launch。每个 case/path 的不同标签轮次使用同一 `execution/wire` 和 cwd，启动前核对冻结程序 SHA；只临时硬链接小型解释器，退出后移除链接并把该轮生成文件归档到 `workspace/`。不复制或链接游戏资产，下一轮重建相同路径的空工作目录。manifest 独立记录 `fixed-executable-slot-v1`，run 的 slot SHA 也须通过汇总校验。默认和旧 stable 协议保持；两种协议的结果不合并。动机、失败日志与真实输入预检见[固定路径调查](../reports/refactoring-fixed-executable-slot-2026-10-18.md)。

```sh
python3 -B tools/perf/summarize.py wire-out/batch-a wire-out/batch-b --out wire-out/summary.json
```

AMD64 event fixture 的三项导入现在使用独立 OFT `0x1140`，IAT 仍为 `0x1190`。原 OFT `0x1180` 与 IAT 重叠，会多解析 ResetEvent/CreateEventW 两个重复导入。循环机器码与 RAX/work/retired/API 预期保持；两架构的独立 PE inventory 检查要求恰好三项导入及原 IAT 地址。20 项工具测试与两个已冻结运行时的 12K smoke 通过，日志前缀 `r7-event-fixture-`。

离线 API inventory 的实际职责位于 `tools/inventory/`：`pe.py` 读取 PE/CLR 声明，`traces.py` 读取调用/查询/失败证据，`api.py` 编排目录扫描、可选 ABI coverage helper 和报表。原 `tools/api_inventory.py` CLI 与 fixture 使用的 PE/scan_game/trace_records 导入保留；源码候选仍从项目 src 读取，工作目录不改变这一根路径。候选字符串、导入声明、实际轨迹和 ABI 登记保持不同证据，目录整理不扩大能力结论。迁移与外部工作目录回归见 [工具验收](../reports/refactoring-inventory-2026-10-19.md)。

CPU 采样/比较和火焰图实现位于 `tools/perf/profile.py`，原 `tools/cpu_profile.py` 保留脚本及模块入口；import 时使用同一模块对象，既有调用与对 demangle 的替换仍作用于实际实现。纯迁移提交的实现逐字节保持，三份真实采样的 JSON/折叠栈/SVG 与旧工具逐字节一致，见 [profile 工具验收](../reports/refactoring-profile-tools-2026-10-19.md)。

capture/compare 可显式传 `--headless --discard-api-log`，匹配 headless/null-log 计时条件。空设备日志仍经过原格式化、频率统计与最近调用记录；轮次内已有 `api.log` 时拒绝覆盖。capture 记录实际 command，compare 记录两种模式；默认保持 UI/file-log 行为。`--headless` 与正数 `--stop-after-presents` 不能组合。采样会引入额外开销，其 wall/MIPS 不纳入无采样的正式性能对照。

```sh
python3 -B tools/cpu_profile.py capture fixture.exe --binary frozen/wire --ir fixture.wir --out wire-out/new-profile --headless --discard-api-log
python3 -B tools/cpu_profile.py summarize wire-out/new-profile/run-0/sample.txt --out wire-out/new-profile-summary
```

七个独立 native 图形探针实现归入 `tools/probes/`，共享 C ABI 布局在 `abi.py`，各探针保留自己的输入、失败断言及清理顺序。旧 `tools/probe_*.py` 脚本及导入继续委托同一实际模块；新入口可用 `python3 -B -m tools.probes.present --help` 等命令。Present 的 AppKit 源码按仓库根定位，MoltenVK 的 C 探针仍在 tools 根，不受调用工作目录影响。13 个 ABI 布局、三项真实 native 报告与 11 个 shader/像素输出对照通过；缺失外部 SDK/validator 的未重跑项目见 [native 探针验收](../reports/refactoring-native-probes-2026-10-19.md)。当前工具测试 26 项。

`tools/probe_cli.py` 委托 `tools/probes/cli.py`，自行生成三个原创小型 PE，比较两个冻结 executable 的 21 组命令。固定 cwd/argv0/解释器软链接，记录输入、工具和 binary SHA；退出删除链接，输出目录必须不存在。退出码、stderr、IR 文件和 API 调用顺序严格比较；stdout 仅规范化三个既有时间/速率位置，API 频率 footer 按内容排序。检查用于命令兼容，不作性能准入；[CLI 验收](../reports/refactoring-cli-followup-2026-10-19.md)保留原始输出和初版误判。

```sh
python3 -B tools/probe_cli.py --before frozen-before/wire --after frozen-after/wire --out wire-out/new-cli-comparison
```

PE32 测量入口现于参数解析后先生成并验证全部 case，再解析 binary 或创建输出目录。未知名称明确报错，`fixture()` 直接调用也拒绝未知名称；原先任意未知名称会静默生成 integer，资源迁移中误写 `gdi-gateway` 已暴露此问题。合法名字和默认四项保持，59 个 case 在 1/12K/100M 三种规模下的 177 份 PE 字节和预期值与修正前完全一致。独立 CLI 检查要求错误退出码 2、无 traceback、无输出目录；工具 **28** 项及四次实际 raw/IR 执行通过，证据 `r7-pe32-case-validation-*`。未修改运行时或重跑长性能计时。

100M 的 AMD64 event 输入 SHA 从 `e95a32dce6313767263432f4df1ab8106745bf24526597ad643d7ceb0b961441` 变为 `f538f2f83f97fbc7152cf9f3bb8628f8663efea3306aaa937374ae194ffaf1b2`。旧正式数据与校验保留，后续使用修正输入独立配对，不跨哈希合并。本次仅修工具，没有修改运行时。
