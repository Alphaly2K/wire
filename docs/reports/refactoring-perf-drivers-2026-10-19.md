# R7 架构测量入口与 fixture 归属

状态：已完成。三个正式测量实现分别进入 `tools/perf/pe32.py`、`amd64.py`、`v2.py`，早期 AMD64 matrix/game/capture 工具进入 `legacy_amd64.py`。四个原脚本/导入入口保留为实际模块别名。测量公共库没有改变，各架构的参数、结果校验、raw/IR、work/retired、API count 和调度协议继续独立。

PE64 image builder、基础及补充 AMD64 输入分别归入 `fixtures/pe.py`、`amd64_basic.py`、`amd64_extra.py`；PE32 builder/生成器归入 `fixtures/pe32.py`。51 份既有 fixture 仅调整 import，处理体 AST 保持；它们通过包内相对路径复用 builder/helper，不再导入命令脚本。旧 `amd64_perf.pe64/fixtures`、`interpreter_perf.additional/replay`、PE32 的 builder/helper 和 V2 bit_fixture 均保留导入。

审核了三个源文件定位调整：measurement hash 指向实际公共库，V2 默认 audit 指向新目录内的相同跟踪输入，replay hash 指向实际 `amd64.py`。内置 audit 使用包导入，其 importlib context 显式设为 fixture 包；外部 `--audit-probes` 仍按原 `audit_probes` standalone context 加载。script/replay/audit 源码 hash 因迁移改变，正式协议须保存新 manifest，不把本次 smoke 计为旧性能测量或收益。

迁移前固定四份原始源码/help 和两种 work（12K/100M）的 **226 份 PE hash/完整期望结果**。迁移后逐项相同；18 个处理函数 AST 在仅归一上述已审核路径、相对导入及内置 fixture package context 后相同。四个旧脚本 help 逐字节一致，四个旧模块和四个新包模块 help 可用。默认 V2 的六组 golden fixture 回归继续通过。

四个旧入口在迁移前/后共 **58 次真实执行、24 条 case/path 协议**。迁移后从项目外工作目录启动；PE32 raw/IR 覆盖 integer、painting、messages、GDI，AMD64 覆盖 integer/memory/SSE/REP/GUID/event/messages，V2 覆盖 FP/bit/code/data alias，早期 AMD64 matrix 覆盖五项基本输入。退出码、guest checksum、retired/work、API 次数及校验标志相同。正式三入口的 input、binary、measurement、launch manifest 字段相同；driver hash 正确改变。单次 work 为 12K 或 20K，这一批检查命令兼容和结果，不作性能结论。

另以包入口实际运行两项默认 V2 输入及一个独立外部 audit，共三次。外部 audit 显式检查 `__name__ == 'audit_probes'`、空 package context，读取原创 bit 输入并通过实际 guest 状态校验。工具 **26 项**测试通过。没有修改或构建运行时，没有执行 legacy 游戏模式，也没有复制游戏资源。

证据根 `wire-out/refactoring-2026-10-14/`：`r7-perf-drivers-before-proof.json`、`r7-perf-drivers-source-proof.json`、`r7-perf-drivers-execution-proof.json`、`r7-perf-drivers-{before,after}`、`r7-perf-drivers-custom*`、`r7-perf-drivers-package-run`、`r7-perf-drivers-tools-tests.txt`。全部输入、原始结果与计时保留；运行时正式契约仍为 675 项。
