# 当前状态

更新日期：2026-10-16。重构前基线为 `2e78377`；最近已验收的运行时提交为 `e2839d5`（进程身份/LastError）。完整阶段与历史验收从 [重构记录](reports/refactoring-2026-10-14.md) 和 [文档入口](index.md) 查阅。

当前在验收八项 TLS/FLS 契约候选：正确性已通过，28 条性能路径及异常项扩样尚未完成，尚未提交运行时代码。独立旧行为基线已提交为 `2b65653`。Mono 原生化已撤回，本轮继续排除。

## 重构范围

| 阶段 | 已交付 | 剩余范围 |
| --- | --- | --- |
| R0/R1 | 冻结输入、原创正式 fixture、领域测试 harness、GUID 唯一契约和双架构适配 | 随后续家族维护独立基线 |
| R2 | 已验收 181 个 export 的契约；明细见下表 | TLS/FLS 候选；其余内存、同步、USER、COM、图形等 legacy 家族 |
| R3 | guest environment、双架构模块状态、Win64 文件状态 owner | PE32 其余资源、线程/COM 等；PE32 kernel 大范围抽取曾因性能撤回 |
| R4/R5 | Process32 编排边界、owned host UI 请求、GDI 与像素状态分离 | 领域分发与资源 owner 随 R2/R3 继续收敛 |
| R6 | 按用户要求忽略 | 不重建原生 Mono 路径 |
| R7 | PE32 内核目录、领域测试、公共性能工具、当前/历史文档入口 | 后续目录和工具随领域迁移继续整理 |

已迁移的 export 是契约覆盖范围；typed、ABI-only 与 absent 的架构策略分别记录，不能按数量推导完整 Windows 兼容性。

| 已验收家族 | export 数 | 契约与证据 |
| --- | ---: | --- |
| GUID | 3 | [新增 API 指引](architecture/adding-api.md) |
| identity / version / NLS | 6 / 8 / 42 | [环境与架构](architecture/runtime.md)、[版本](architecture/version.md)、[NLS](architecture/nls.md) |
| module | 31 | [模块](architecture/modules.md) |
| file | 53 | [文件及标准句柄](architecture/files.md)；包含 metadata、目录/变更、枚举和 section |
| Interlocked / SLIST | 6 | [同步原语边界](architecture/interlocked.md) |
| 路径辅助 / 通用句柄 | 13 / 6 | [路径与句柄](architecture/paths-handles.md) |
| 时钟 | 6 | [时钟](architecture/clocks.md)及[验收](reports/refactoring-clocks-2026-10-15.md) |
| 进程身份 / LastError | 7 | [上下文](architecture/context.md)及[验收](reports/refactoring-context-2026-10-16.md) |

## 当前执行路径与能力

| 范围 | 入口与边界 |
| --- | --- |
| PE32 | `wire run <exe>`；`wire prepare <exe> --out <wir>` 后可用静态 IR。raw/IR 均解释执行，`.wir v3` 持久格式保持 |
| AMD64 | `wire probe64 <exe> --startup`；受检稀疏内存、typed IR、DLL/TLS、合作线程、异常与 Win64 服务；仍为实验入口 |
| 图形 | PE32 GDI/D3D9、AMD64 DXGI/D3D11 子集；Vulkan/MoltenVK 加速需显式提供已构建的 native graphics 库 |
| Mono | 执行游戏自带的原始 guest Mono；[原生 ARM64 桥接撤回记录](mono-native-rollback-2026-10-14.md) |
| Wasm | JSC 可选加速研究与原创循环 fixture；主程序默认走解释器 |
| 文件/设备 | 受限 guest 文件系统；虚拟身份、单 CPU 和设备/transport 子集按所属 API 契约处理 |

最近已验收的 Alice smoke 使用现有 runtime 目录，三对均到 200M work 预算出口，窗口隐藏且 client 314×211，Draw/Present 为 0。该启动前缀未进入实际绘制；游戏 logo/加载/渲染的历史推进结果应按报告中的提交与输入判断。

## 验证与性能

最近已验收的上下文批次：release **442 passed / 7 ignored**，debug Win32/AMD64/FPU **290 passed**，显式 native **7 passed**，工具 **19 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。TLS/FLS 候选另有 release 448 / debug 296 的正确性结果，性能验收尚未完成，见 [候选记录](reports/refactoring-locals-2026-10-16.md)。

上下文的 25 条性能路径全部结果校验。AMD64 SSE 初筛 +3.006% 后扩为 2.1B 三标签轮换，中位数仍有 **+2.358%** 局部成本，其余最大回退 +2.505%，RSS 中位数最多增加 160KiB。各批次保留自己的基线、失败轮次和局部成本，未将结构调整解释为游戏 FPS 提升。

后续正式比较使用显式 [稳定启动协议](reports/refactoring-launch-protocol-2026-10-16.md)，固定 argv0、参数和 child cwd，并记录 shell SHA。默认工具行为保持；旧协议数据独立保留，汇总不跨协议或不同 work 合并。编译、正式计时、采样与 runtime diagnostics 串行执行。每个结构候选先完成正确性，再按至少三对轮换检查结果、CPU/wall/RSS 与输入哈希；超过门槛先调查，不把失败候选扩大到其它领域。

必需检查：

```sh
cargo fmt --all -- --check
cargo test --release --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
python3 -B -m unittest discover -s tools -p 'test_*.py'
```

七个默认 ignored 均需外部条件：两项 Metal/software 像素对照需真实 Metal device；三项 native GPU 资源/Shader/owner 及两项 Win64 DXGI draw/swapchain 需 native graphics、MoltenVK 与原创 DXBC/GS fixture。涉及这些边界时显式提供环境执行，不能将 ignored 计为通过。Unicorn 参照测试在受限 macOS 沙箱可能触发 SIGILL，已验收批次在允许参照执行的环境验证；发布运行时不使用 Unicorn。

## 证据存储

本轮冻结 executable、source/input manifest、完整测试日志和性能轮次位于 `wire-out/refactoring-2026-10-14/`。该目录日期是证据批次标识，后续日期继续复用。只冻结小型运行时与源码，游戏素材通过原路径/软链接复用，不复制或硬链接。历史候选拒绝、布局/调度调查及全部性能报告统一从 [文档入口](index.md) 查阅。
