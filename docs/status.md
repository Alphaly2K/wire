# 当前状态

更新日期：2026-10-19。重构前基线为 `2e78377`；最新已验收运行时为 [API 日志与累计契约](reports/refactoring-api-log-2026-10-19.md)，实现 `b433c63`，报告记录冻结源码和 executable SHA。完整阶段与历史验收从 [重构记录](reports/refactoring-2026-10-14.md) 和 [文档入口](index.md) 查阅。

本轮累计接入对话框/窗口枚举 22、菜单 27、GDI 对象/像素/字体 31 项，共 **80 项**，累计 **675 项**。统一声明、typed 分发与领域 owner 保持双架构范围、故障提交和既有不可用策略；保留真实 RedrawWindow 网关与公开 facade 的历史差异。精确标量 SSE 已移入独立编译单元，没有近似浮点或新增 ISA 语义。

当前代码另已接入 **22 项契约**：5 项 paint、2 项 comctl32 初始化、10 项 PE32 资源、5 项双架构启动信息，完整正确性通过，性能因后台 Unity/WindowServer 负载及明显同哈希漂移待复核；与上述 675 项分别记录。新实现、冻结及全部结果见[五项 paint](reports/refactoring-painting-followup-2026-10-19.md)、[控件初始化](reports/refactoring-common-controls-2026-10-19.md)、[资源](reports/refactoring-resources-2026-10-19.md)与[启动信息](reports/refactoring-process-startup-2026-10-19.md)。

中间 GDI 冻结出现 messages/IR 三对持续回退，未准入。实际采样后将 API 日志提取为独立 runtime 服务，并用专用整数格式化保留完整 trace、频率和最近 64 条记录。最终消息 150M IR/raw CPU 中位数相对原消息冻结 −13.210%/−12.110%，GDI 150M IR/raw −16.987%/−13.229%；33 条性能协议、207 次 fixture 执行及三对真实启动前缀通过。旧冻结、失败轮次和同哈希 control 独立保留，没有用其它负载的改善抵消持续回退。

早期绘制六项候选因持续浮点回退撤回，见[历史拒绝报告](reports/refactoring-painting-2026-10-19.md)。当前五项重试保持原 RedrawWindow 网关归属，不追认旧候选。其余 USER、COM、图形和资源 owner 继续迁移。R7 [CLI 拆分已重试接入](reports/refactoring-cli-followup-2026-10-19.md)，完整正确性及命令对照通过，性能待复核；旧候选和回滚证据保留。[离线 API inventory](reports/refactoring-inventory-2026-10-19.md)、profile、native 探针和架构性能入口已按职责整理；PE32 工具另修正未知 case 静默替换为整数的问题，最新工具检查 28 项通过，详见[工具边界](architecture/validation-tools.md)。Mono 原生化已撤回，本轮排除。

## 重构范围

当前累计性能冻结已验收[对话框](architecture/dialog.md)、[菜单](architecture/menus.md)、[GDI 对象/像素/字体](architecture/gdi-operations.md)与日志修订；独立历史证据继续可查。五项 paint 单独冻结并通过正确性，性能待复核；CLI 与剩余领域不属于累计冻结。

| 阶段 | 已交付 | 剩余范围 |
| --- | --- | --- |
| R0/R1 | 冻结输入、原创正式 fixture、领域测试 harness、GUID 唯一契约和双架构适配 | 随后续家族维护独立基线 |
| R2 | 已验收 675 个 export 的契约，另 22 项 paint/控件初始化/资源/启动信息正确性通过、性能待复核；明细见下表 | USER、COM、图形等 legacy 家族 |
| R3 | guest environment、双架构模块状态、Win64 文件、period 请求、SRW、地址等待及 APC 队列、线程标量操作与描述、双架构 apartment/BSTR、Win64 task/HSTRING/image/desktop 成员、PE32 TLS/FLS 与双架构 cursor、Win64 DC/class 私有 owner | PE32 其余资源、线程资源创建/退出及 COM 其余对象；PE32 kernel 大范围抽取曾因性能撤回 |
| R4/R5 | Process32 编排边界、owned host UI 请求、GDI 与像素状态分离 | 领域分发与资源 owner 随 R2/R3 继续收敛 |
| R6 | 按用户要求忽略 | 不重建原生 Mono 路径 |
| R7 | CLI 命令职责拆分、PE32 内核目录、领域测试、公共性能工具及 profile、离线 inventory、当前/历史文档入口 | CLI 性能待复核；其余工具和目录随领域迁移继续整理 |

已迁移的 export 是契约覆盖范围；typed、ABI-only 与 absent 的架构策略分别记录，不能按数量推导完整 Windows 兼容性。

| 已验收家族 | export 数 | 契约与证据 |
| --- | ---: | --- |
| GUID | 6 | [新增 API 指引](architecture/adding-api.md)及[资源辅助验收](reports/refactoring-guid-allocation-2026-10-18.md) |
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
| 线程查询 / 设置 | 9 | [调度状态与调用边界](architecture/thread-properties.md)及[验收](reports/refactoring-thread-properties-2026-10-17.md) |
| 线程创建 / 退出 / APC / CONTEXT / description | 6 | [控制结果与资源边界](architecture/thread-control.md)及[验收](reports/refactoring-thread-control-2026-10-17.md) |
| COM / WinRT 初始化、工厂、task memory / Ole 策略 | 10 | [调用与资源边界](architecture/com-core.md)及[验收](reports/refactoring-com-core-2026-10-17.md) |
| WinRT HSTRING / activation | 11 | [字符串与资源边界](architecture/winrt.md)及[验收](reports/refactoring-winrt-2026-10-17.md) |
| BSTR | 8 | [字符串与资源边界](architecture/bstr.md)及[验收](reports/refactoring-bstr-2026-10-17.md) |
| Variant | 4 | [调用与资源边界](architecture/variants.md)及[验收](reports/refactoring-variant-2026-10-18.md) |
| 基础字符串 / 指针与 MulDiv / 进程查询 | 14 / 5 / 10 | [基础 helper 边界](architecture/basic-helpers.md)及[验收](reports/refactoring-basic-2026-10-18.md) |
| 键盘状态 / 布局 | 3 / 13 | [键盘边界](architecture/keyboard.md)及[验收](reports/refactoring-keyboard-2026-10-18.md) |
| raw input / IME | 7 / 17 | [输入能力边界](architecture/input-profile.md)及[验收](reports/refactoring-input-profile-2026-10-18.md) |
| 矩形 / 窗口边框 | 10 / 3 | [几何边界](architecture/geometry.md)及[验收](reports/refactoring-geometry-2026-10-18.md) |
| 游标 / 指针能力 | 10 | [调用与状态边界](architecture/cursor.md)及[验收](reports/refactoring-cursor-2026-10-18.md) |
| 图标 / 光标资源 | 8 | [资源边界](architecture/images.md)及[验收](reports/refactoring-images-2026-10-18.md) |
| 显示 / monitor / metrics | 16 | [显示边界](architecture/display.md)及[验收](reports/refactoring-display-2026-10-18.md) |
| GDI DC / 设备能力 | 7 | [DC 边界](architecture/gdi-dc.md)及[验收](reports/refactoring-gdi-dc-2026-10-18.md) |
| 桌面设置 / DWM | 6 | [桌面边界](architecture/desktop.md)及[验收](reports/refactoring-desktop-2026-10-18.md) |
| 窗口类 / 类信息与字段 | 17 | [类边界](architecture/classes.md)及[验收](reports/refactoring-classes-2026-10-18.md) |
| 注册名称 / 窗口属性 | 14 | [对象边界](architecture/user-objects.md)及[集成验收](reports/refactoring-memory-call-boundary-2026-10-18.md) |
| 窗口查询 / 字段与坐标 | 25 | [查询边界](architecture/window-queries.md)及[集成验收](reports/refactoring-window-queries-2026-10-18.md) |
| 窗口文本 / 焦点与 capture | 16 | [状态边界](architecture/window-state.md)及[完整验收](reports/refactoring-window-state-2026-10-18.md) |
| 窗口生命周期 / 过程控制与同步消息 | 17 | [控制边界](architecture/window-control.md)及[完整验收](reports/refactoring-window-control-2026-10-18.md) |
| 消息队列 / 过程分发 / 计时器与消息元数据 | 22 | [消息边界](architecture/messages.md)及[完整验收](reports/refactoring-messages-2026-10-18.md) |
| 对话框控件 / 窗口枚举 | 22 | [对话框边界](architecture/dialog.md)及[累计验收](reports/refactoring-api-log-2026-10-19.md) |
| 菜单资源与操作 | 27 | [菜单边界](architecture/menus.md)及[累计验收](reports/refactoring-api-log-2026-10-19.md) |
| GDI 对象 / 像素 / 字体 | 31 | [图形操作边界](architecture/gdi-operations.md)及[累计验收](reports/refactoring-api-log-2026-10-19.md) |

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

最近已验收的冻结：release **625 passed / 7 ignored**，debug Win32/AMD64/FPU 与日志单元 **471 passed**，显式 native **7 passed**，工具 **23 passed**；fmt、strict all-target/all-feature Clippy、SIMD crate 静态检查与 release build 通过。402 个 Rust 文件和 executable 核对一致。

当前五项 paint 冻结：release **628 passed / 7 ignored**、debug **474 passed**、native **7 passed**、工具 **26 passed**，上述静态检查和 build 通过。290 个 src Rust 文件与冻结一致，executable 仅增加 48 bytes。20 条 12K 功能协议共 120 次执行通过；37 个长样本结果校验后因同哈希明显漂移停止长计时，另保留 1 个主动中断样本。小规模功能耗时不作性能准入。

该候选三对 Alice 200M 功能前缀也到相同预算/隐藏窗口/零 Draw/Present 出口；RSS before 307–378MiB、after 379–397MiB，保留差异待复核，不与前述已验收启动计时混算。

随后两项控件初始化冻结：release **631 passed / 7 ignored**、debug **477 passed**、native **7 passed**、工具 **26 passed**，静态检查和 build 通过；290 个 src Rust 文件核对一致，executable 较 paint 冻结增加 1,344 bytes。9 条 12K 功能协议共 54 次执行，以及一对 Alice 200M 功能前缀通过，长性能对照待复核。

最新 CLI 拆分冻结：其余 289 个 src Rust 文件与控件冻结相同，只改变 main 和四个命令模块；完整 release **631 passed / 7 ignored**、fmt、严格 Clippy 与 build 复测通过。**21 组命令/42 次执行**、IR 文件及 API 调用/频率内容、一对 Alice 功能前缀通过；新增仓库内检查工具自行生成输入后，21 组对照再次全部通过，工具 **26** 项复测通过。debug/native 沿用上述同源 library 证据。294 个 src Rust 文件与冻结一致，executable 增加 5,472 bytes，性能待复核。

随后资源冻结：完整 release **636 passed / 7 ignored**、debug **482 passed**、native **7 passed**、工具 **26 passed**，静态检查与 build 通过；296 个 src Rust 文件核对一致，executable 较 CLI 减少 3,616 bytes。八种实际输入、16 条 raw/IR 功能协议共 96 次执行、六对完整资源 trace 和一对 Alice 200M 功能前缀通过；性能待复核。

最新启动信息冻结：完整 release **640 passed / 7 ignored**、debug **486 passed**、native **7 passed**、工具 **28 passed**，静态检查与 build 通过；299 个 src Rust 文件核对一致，executable 较资源增加 19,024 bytes。PE32 16 条协议/96 次执行、AMD64 4 条协议/24 次执行、六对启动 API trace 和一对 Alice 功能前缀通过；性能待复核。

33 条 workload protocol 全部结果校验，不跨不同 work/工具版本混算；GUID/raw 首轮离散后，150M 加同哈希 control 三对均改善，CPU 中位数 −14.007%。15 项计算路径的 CPU 中位数变化为 −4.408% 至 +2.111%，fixture RSS 中位数最多 +48KiB。二进制较直接 GDI 候选增加 32 bytes，满足 +24KiB 预算。Alice CPU/wall +1.272%/+1.650%，RSS 中位数约 +5.91MiB，原生启动 before 的 RSS 范围约 319–398MiB；保持同一预算、隐藏窗口与零 Draw/Present。所有离散轮次保留，没有推断游戏 FPS。

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
