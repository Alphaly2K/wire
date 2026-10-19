# 当前状态

更新日期：2026-10-19。重构前基线为 `2e78377`；最新已验收运行时为[消息队列与计时器](reports/refactoring-messages-2026-10-18.md)，实现 `c07b983`，该报告记录冻结源码和 executable SHA。完整阶段与历史验收从 [重构记录](reports/refactoring-2026-10-14.md) 和 [文档入口](index.md) 查阅。

本轮追加验收消息队列、计时器、DispatchMessage 与消息元数据等 22 项，统一契约与 typed 控制。保留 PE32 的阻塞/非局部转移、队列变更的部分提交，以及 Win64 callback/resume、DWORD 与指针宽度和 absent 的差异。

后续 22 项对话框/枚举契约和[精确标量 SSE 编译隔离](reports/refactoring-scalar-codegen-2026-10-19.md)已接入并通过完整正确性，性能仍待复核；FP 初筛通过，宽矩阵若干短样本波动及扩样中的严重同哈希漂移全部保留，未增加已验收计数。[菜单 27 项契约与 owner](reports/refactoring-menus-2026-10-19.md)随后独立接入，release 619、debug 465、显式 native 7、工具 23 及完整静态检查通过，性能待验收；先固定真实 RedrawWindow 网关与公开 facade 的原始差异及不可用入口的不同 note/返回 frame，分别保存两批候选的冻结与验收。

对话框后续已补齐 35 条完整性能协议 / 234 次执行及三对 Alice 前缀；SSE/raw、messages/IR、GDI/IR 的中位数成本仍超过调查线，全部轮次和同哈希 control 独立保留。随后完成六次真实采样，消息/GDI 成本主要位于 API 路径，SSE/raw 前后热点分布接近；[PE32 分发模式候选](reports/refactoring-direct-dispatch-2026-10-19.md)把 50 个 typed guard 改为直接枚举模式，处理体与次序保持，完整 release 619、debug 465、native 7 和严格检查通过，性能验证进行中。菜单自身初筛的 raw/IR 三对全部校验，固定菜单循环 CPU −0.144%/−8.735%，宽矩阵仍待完成。累计验收维持 595。

完整正确性、38 条性能协议、237 次执行及三对游戏前缀通过，累计 **595 项**。真实 post/get 网关 Win64/PE32 IR/raw CPU −1.417%/−10.391%/−1.827%；SSE/IR 初筛回退和同哈希 control 漂移全部保留，长样本未出现持续回退，详见本批报告。失败候选与[固定 executable/cwd 协议调查](reports/refactoring-fixed-executable-slot-2026-10-18.md)继续可查。绘制区域与 paint DC 六项迁移正确性通过，但持续浮点性能回退，已撤回运行时改动，保留独立旧行为基线与原创 paint 循环，见[拒绝报告](reports/refactoring-painting-2026-10-19.md)。其余 USER、COM、图形继续迁移。Mono 原生化已撤回，本轮继续排除。

## 重构范围

最新另接入 [GDI 对象、像素与字体 31 项](reports/refactoring-gdi-operations-2026-10-19.md)：唯一 catalog 和图形操作枚举，adapter 只借 Graphics/LastError；独立旧行为基线、完整 release 623、debug 469、native 7 和严格检查通过。累计扩样中消息/IR 三对均回退超过 5%，该冻结未准入；旧轮次全部保留。重新采样后提取 [API 日志职责并优化固定整数格式化](reports/refactoring-api-log-2026-10-19.md)，完整 release 625、debug 471、native 7、工具 23 和严格检查通过，逐字节保留完整 trace/频率/64 条历史；150M 消息 IR/raw 相对正式消息冻结 CPU −13.210%/−12.110%，正在核验宽矩阵及游戏前缀。对话框、菜单、分发和 GDI 冻结独立保留，正式计数仍为 595。

| 阶段 | 已交付 | 剩余范围 |
| --- | --- | --- |
| R0/R1 | 冻结输入、原创正式 fixture、领域测试 harness、GUID 唯一契约和双架构适配 | 随后续家族维护独立基线 |
| R2 | 已验收 595 个 export 的契约；明细见下表 | USER、COM、图形等 legacy 家族 |
| R3 | guest environment、双架构模块状态、Win64 文件、period 请求、SRW、地址等待及 APC 队列、线程标量操作与描述、双架构 apartment/BSTR、Win64 task/HSTRING/image/desktop 成员、PE32 TLS/FLS 与双架构 cursor、Win64 DC/class 私有 owner | PE32 其余资源、线程资源创建/退出及 COM 其余对象；PE32 kernel 大范围抽取曾因性能撤回 |
| R4/R5 | Process32 编排边界、owned host UI 请求、GDI 与像素状态分离 | 领域分发与资源 owner 随 R2/R3 继续收敛 |
| R6 | 按用户要求忽略 | 不重建原生 Mono 路径 |
| R7 | PE32 内核目录、领域测试、公共性能工具、当前/历史文档入口 | 后续目录和工具随领域迁移继续整理 |

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

最近已验收的冻结：release **607 passed / 7 ignored**，debug Win32/AMD64/FPU **453 passed**，显式 native **7 passed**，工具 **23 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。388 个 Rust 文件和 executable 核对一致；随后绘制基线与迁移不属于该冻结。失败候选和 codegen 调查保留在[集成报告](reports/refactoring-memory-call-boundary-2026-10-18.md)。

本批 38 个 workload protocol 全部结果校验，不跨不同 work/工具版本混算。SSE/IR 扩样相对 before −0.246%、同哈希 control/before +4.965%，逐对离散和短样本回退保留。RSS 中位数最多 +272KiB，二进制增加 5,728 bytes，在 +24KiB 预算内。Alice CPU/wall −0.907%/−7.945%，保持同一预算、隐藏窗口与零 Draw/Present；未推断游戏 FPS。历史各批次成本、预算修订与漂移证据保留在对应报告；稳定启动协议见[工具边界](architecture/validation-tools.md)。

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
