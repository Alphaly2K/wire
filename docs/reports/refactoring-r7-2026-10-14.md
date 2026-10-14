# R7 当前稳定边界整理

候选从 `e090281` 开始；这是目录、领域测试和性能工具的一个可独立回滚批次。R2/R3 的其它 API 家族及状态收拢继续推进，后续形成的边界仍会有对应目录迁移；不将此批标记为全仓重构完成。Mono/native source/第三方构建输入未迁移。

## 交付

- PE32 CPU/FPU/MMX/SIMD/IR、内存/VM、PE 与 loader 实现进入 `src/x86/`；根 `cpu/ir/mem/pe/loader` 保留公开重导出 facade。Process 的结果类型经旧 cpu 入口继续可用，CPU 的兼容 process_facade 保留原委托。
- stub 地址 `0x6fff0000`、stride 16、容量 4096 归入 `x86::address_space`，api 重导出原常量。CPU/IR 的保留地址判断直接依赖架构布局。指令、内存和 `.wir v3` 格式未改，include/build.rs/FFI 输入检查及全量构建通过。
- x64 的 130 个、Win32 的 109 个既有测试按领域拆分，保留 integration target。234 个完整名称增加领域前缀，5 个原始 guest Mono 观察测试继续留在 x64.rs。旧名 substring 过滤仍可用，exact 过滤映射见 `tests/test-name-map-2026-10-14.json`。只读逐函数比较确认每个测试/helper body 与 `e090281` 一致，cfg(unix)/cfg(macos) 条件保留；Cargo 列表确认每个新名称出现一次。
- 公共测量库仅收拢 hash、child CPU/wall/RSS、timeout、输出和 AB/BA。架构 fixture 与校验保持独立；旧字段保留，新增 timeout/测量库 hash，V2 另记录 replay hash。PE32 wall 现在不包含报告读取，Linux RSS 统一成 bytes。
- freezer 进入 tools/perf，旧命令入口委托。默认排除无关的未跟踪文档/PDF/IDE 文件，显式额外输入用 --include；symlink 保存链接，不复制目标游戏。真实冻结 manifest 已核对移入的源码及用户文件排除结果。

## 正确性

原始日志在 `wire-out/refactoring-2026-10-14/r7-*`：

- 完整 release 400 passed / 0 failed / 7 ignored；debug Win32/AMD64/FPU/Unicorn 285 passed；fmt、strict all-targets/all-features Clippy 通过。当前 Cargo 没有 native Mono features；按实际已撤回配置验证。
- Python 工具 11 passed，包括失败输出/exit/resource、timeout、AB/BA/hash、冻结输入隔离、显式输入与 symlink。
- 外部 native GPU/Metal 7 项另行显式执行，7 passed；复用 R5 的 native/GS/DXBC/MoltenVK 输入与 hash。主线程 host_ui_probe 验证上传/失败、resize/removal 和 capture，通过。
- Alice 原路径的 200M work 前缀以预算出口结束：retired 199588616、API calls 390496、隐藏窗口 client 314×211，draw/present 均 0。固定预算及可核对窗口行为保持，宿主计时/线程可影响逐指令停点。没有复制或硬链接游戏素材。

## 性能

R7 binary SHA-256：`f51717e727c78eb6ceeee875798c91c60511d3c3a4c9c6cd2bbfb724ab24ea04`；完整源快照、diff、toolchain 在 `r7-wire/manifest.json`。对照 `nls-wire`：`39ef6903fdf338d13bf81596a590756b482a0e9dd5f1398b4fcbeff0c2ec9982`。binary 从 5,864,104 到 5,866,360 bytes（+2,256）。

三套工具先执行 5K work 的全部低预算 preflight，再正式串行 100M work、三对 AB/BA，共 38 个 case/path、228 轮：PE32 18、AMD64 14、V2 6。PE32 校验 EAX/instructions；AMD64/V2 校验 RAX/retired/work，gateway 使用所属脚本的 API count 断言；全部通过。计时与 Cargo/native 执行/diagnostics 分开，没有丢弃失败轮次。

| 范围 | CPU 中位数最大回退（相对 NLS） |
| --- | ---: |
| PE32 raw/static IR | +1.50%（bit-test/IR） |
| AMD64，large-code 扩样后 | +2.41%（nls-conversion） |
| V2 FP/bit/cache alias | +1.67%（bit-test） |

large-code 初筛 +3.80%，单轮约 0.27s，各对 +2.77%/+4.11%/+2.37%。按调查门槛扩为 2B work、每轮约 5.3s，加入最初基线并保留三对九轮：相对 NLS +0.22%，各对 -0.74%/+0.22%/+0.77%；相对最初基线 +1.23%。扩大结果未出现持续回退，接受此批。全矩阵 RSS 中位数最大增加 32KiB，无实质容量增长。

全部轮次/输入 hash 在 `r7-{pe32,amd64,v2}-timing` 和 `r7-large-code-investigation`。首次性能命令把既有 nls-wire 文件误作目录，执行前失败；错误日志 `r7-pe32-timing.txt` 保留，正确命令另记 `r7-pe32-timing-fixed.txt`。这些是结构重构的确定 fixture 结果，不推断完整游戏 FPS。
