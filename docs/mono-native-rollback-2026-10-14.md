# 撤回原生 Mono 实验路径

2026-10-14 按用户指示撤回 ARM64 原生 Mono 替换与桥接实验。游戏继续使用原始 PE Mono，通过现有 x86/AMD64 IR 解释器执行；GPU 渲染接入保持现状。

原生 Mono 桥接跨越对象布局、GC、managed 引用、Unity native 对象和 guest 地址空间，当前方案的跨游戏与跨版本维护成本过高。停止该路线，后续工作回到解释器性能与通用 Win32/图形兼容。

## 撤回范围

- 移除 `native-mono-memory`、`native-mono-backend` 两个 Cargo feature，以及 `--native-mono-*` CLI 参数。
- 移除原生 runtime provider、Mono C API/Unity InternalCall 桥接、回调调度、根与共享 backing 管理、专用测试和 `tools/mono` 构建/补丁/probe 工具。
- 移除实验路径的接续文档。完整历史和撤回前实现保留在 Git，撤回前 main 为 `c5b37bb`。
- 不改写历史，不按时间段整体回滚，不删除游戏素材或 `wire-out` 中的既有测试输出。

## 保留的通用改动

| 范围 | 保留内容与代表提交 |
| --- | --- |
| AMD64 IR/缓存 | 有界段执行、REP、预绑定 Win32 gateway、按代码页 generation 校验（`cc76ecb`），victim store（`ee24896`） |
| SSE/浮点 | 定宽 packed lanes（`07a44b9`）、精确标量 SSE（`078bde8`）、PE32 raw descriptor 复用（`d6c9772`）、混合段执行（`370457f`） |
| FLAGS/段所有权 | PE32 dead FLAGS（`87f0e5c`）、条件 FLAGS 精确 read mask（`8598896`）、checked segment 精确 fault 提交（`61e0dc5`）、owned/Arc 段所有权（`5739d9d`、`ac13119`） |
| 通用兼容 | Win64 注册表句柄修复（`40c581d`）、混在 Mono 提交 `5555c49` 中的 IME 参数和无效句柄行为修复 |
| 诊断/验证 | 翻译请求与段计数、解释器性能工具、原始 guest Mono 导出跟踪与相关 fault/alias 测试、独立 fallback oracle 与字符串比较回归（`d55fa05`） |
| 图形/其他 | 既有 D3D11/DXGI/MoltenVK、Win32/VFS/线程兼容、所有此前游戏推进成果 |

`8495b3c` 的改动仅修正实验性 shared backing 的标量更新路由，随 shared backing 一并移除；普通 guest 页的单次 lookup 标量 RMW 优化完整保留。`module_image_bounds` 与临时 native token slot 检查只被移除的桥接使用，亦一并撤回。

原始 Mono 诊断仍可用：`wire probe64 <exe> --startup --mono-trace <entries.tsv>`。该模式观察 guest 执行，不替换 VM；正常运行不启用跟踪。`--api-trace`、`--cpu-stats`、`--cpu-diagnostics` 与翻译缓存实验参数保持可用。

通用优化的已有性能证据见 [V1 实现记录](interpreter-implementation-2026-10-09.md) 与 [V2 实现记录](interpreter-implementation-v2-2026-10-09.md)。这些是原测试数据，本次撤回不据此报告新的性能收益。

## 本次验证

- `cargo test --release --all-targets --locked`：382 passed、0 failed、7 ignored，包括 Unicorn 对照、缓存/代码页失效、FLAGS、SIMD、线程、原始 Mono 跟踪与 Win32/图形回归。移除的是原生 Mono 专用测试，通用测试文件保持原样。
- `cargo clippy --all-targets --all-features --locked -- -D warnings`、`cargo build --release --locked`、`cargo fmt --all -- --check`、`git diff --check` 全部通过。
- 对照撤回前 `c5b37bb`，PE32 CPU/IR/SIMD、AMD64 IR/FP/vector/segment、翻译诊断/page cache、gateway/trace、IME/注册表、GPU/图形和通用性能工具文件内容未变。混合文件的 diff 已检查，普通 guest 页 RMW、generation 校验、victim store 和段分派全部保留。
- Alice 复用已有隔离目录，未复制素材。首次未配置 graphics 的启动在已完成原始 Mono DLL/TLS 后按预期拒绝 DXGI；随后显式加载既有 graphics/MoltenVK 库，执行至 200,000,000 work 预算，退休 199,588,740 条指令后正常按预算停止，没有新 API/访存错误。此启动前缀未进入 Draw/Present，本次不报告整局运行或性能 AB。

日志保存在 `wire-out/mono-rollback-2026-10-14/`：`release-tests.txt`、`clippy.txt`、`build.txt`、`alice-guest-smoke.txt` 与 `alice-guest-graphics-smoke.txt`。
