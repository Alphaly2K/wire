# 当前状态

更新日期：2026-10-17。重构前基线为 `2e78377`；最新已验收运行时为 [地址等待契约与 owner](reports/refactoring-address-waits-2026-10-17.md)，该报告记录冻结源码和 executable SHA。完整阶段与历史验收从 [重构记录](reports/refactoring-2026-10-14.md) 和 [文档入口](index.md) 查阅。

本轮完成 SRW/条件变量/InitOnce、critical section 与地址等待三批，共 23 个 export。SRW map 封装为私有 owner，critical section 使用双架构 codec，地址等待的队列操作与序号收进线程 owner；CPU 与调度提交保持。后续继续处理线程、USER、COM、图形等 legacy 家族。Mono 原生化已撤回，本轮继续排除。

## 重构范围

| 阶段 | 已交付 | 剩余范围 |
| --- | --- | --- |
| R0/R1 | 冻结输入、原创正式 fixture、领域测试 harness、GUID 唯一契约和双架构适配 | 随后续家族维护独立基线 |
| R2 | 已验收 304 个 export 的契约；明细见下表 | 线程、USER、COM、图形等 legacy 家族 |
| R3 | guest environment、双架构模块状态、Win64 文件、period 请求、SRW 与地址等待队列、PE32 TLS/FLS 私有 owner | PE32 其余资源、线程/COM 等；PE32 kernel 大范围抽取曾因性能撤回 |
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
| TLS / FLS | 8 | [local-slot 边界](architecture/locals.md)及[验收](reports/refactoring-locals-2026-10-16.md) |
| event / semaphore | 16 | [同步对象边界](architecture/sync-objects.md)及[验收](reports/refactoring-sync-objects-2026-10-16.md) |
| mutex | 7 | [所有权与释放边界](architecture/mutexes.md)及[验收](reports/refactoring-mutexes-2026-10-16.md) |
| WaitFor* | 4 | [进程控制与等待边界](architecture/waits.md)及[验收](reports/refactoring-waits-2026-10-16.md) |
| Sleep / 线程让出 | 3 | [调度与返回帧边界](architecture/delays.md)及[验收](reports/refactoring-delays-2026-10-16.md) |
| waitable timer | 9 | [状态与调用边界](architecture/timers.md)及[验收](reports/refactoring-timers-2026-10-16.md) |
| virtual memory | 8 | [内存与返回槽边界](architecture/virtual-memory.md)及[验收](reports/refactoring-virtual-memory-2026-10-16.md) |
| heap | 13 | [资源与调用边界](architecture/heaps.md)及[验收](reports/refactoring-heaps-2026-10-16.md) |
| Local / Global memory | 14 | [共享资源与调用边界](architecture/local-memory.md)及[验收](reports/refactoring-local-memory-2026-10-16.md) |
| 系统信息 / 内存状态 | 4 | [布局与调用边界](architecture/system-profile.md)及[验收](reports/refactoring-system-profile-2026-10-16.md) |
| 日历 / 时区 / CPU 计时 | 11 | [调用与宿主边界](architecture/calendar.md)及[验收](reports/refactoring-calendar-2026-10-16.md) |
| 多媒体计时器 | 3 | [调用与资源边界](architecture/multimedia.md)及[验收](reports/refactoring-multimedia-2026-10-16.md) |
| SRW / 条件变量 / InitOnce | 12 | [状态与调度边界](architecture/inline-sync.md)及[验收](reports/refactoring-inline-sync-2026-10-16.md) |
| Critical section | 8 | [调用与调度边界](architecture/critical-sections.md)及[验收](reports/refactoring-critical-sections-2026-10-16.md) |
| 地址等待 | 3 | [队列与线程状态边界](architecture/address-waits.md)及[验收](reports/refactoring-address-waits-2026-10-17.md) |

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

最近已验收的地址等待批次：release **488 passed / 7 ignored**，debug Win32/AMD64/FPU **335 passed**，显式 native **7 passed**，工具 **20 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。

地址等待的 40 条性能路径全部结果校验；新网关 CPU −0.351%，最大回退 +1.907%，RSS 最多增加 80KiB；游戏前缀 CPU −0.408%、wall +0.460%，启动终点保持。本轮 critical section 网关 CPU Win64 −3.853%、PE32 IR/raw −1.418%/−0.813%。各批次使用自己的基线，日历批次的 Local/Global 局部成本与其它历史离散轮次继续保留在对应报告，未将结构调整解释为游戏 FPS 提升。AMD64 event fixture 的重复导入已独立修正；旧验收保持冻结输入，后续按新哈希配对，详见[工具边界](architecture/validation-tools.md)。

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
