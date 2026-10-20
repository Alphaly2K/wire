# 当前状态

更新日期：2026-10-20。按 [重构计划](refactoring-plan-2026-10-11.md) 推进的代码重构主体和阶段整合已交付，重构前基线为 `2e78377`，当前累计 **919 项 export 契约**。R5 的真实音频验收仍待确认；R6 按用户要求排除。分阶段核对与保留项见 [收尾审计](reports/refactoring-closeout-2026-10-20.md)，历史提交与失败记录见 [重构记录](reports/refactoring-2026-10-14.md)。

最新阶段整合基于 `37b2c9c` 加本批 Win64 input/processor 窄资源提取：release 全目标 **695 passed / 8 ignored / 21 suites**，工具 **31 项**、all-targets/all-features strict Clippy、release build 和 fmt 通过。此前原生窗口、真实 Metal/Vulkan、CoreText 的证据按实际执行批次保留；新增音频 fixture 在打开输出时失败，原始 C 主线程探针也失败，具体原因未确定。

PE32 [Process](architecture/callbacks32.md) 已拥有 API 控制、USER callback、DLL/TLS 生命周期、线程退出、SEH 与 CRT 编排，各自恢复策略保留。CPU 单步/IR 不依赖这些服务，公开 Cpu::run/call_guest 委托仍兼容。USER 与 Modules 的领域 owner 已归入 win32；根模块保留兼容入口。

Win64 普通 [文件/endpoint/section](architecture/files.md)、[Environment](architecture/environment.md)、[TLS/FLS](architecture/locals.md)、[USER](architecture/window-queries.md)、[输入](architecture/input-profile.md)、COM/字符串及图形调用已按实际资源收窄；Runtime 保留跨 owner 的 close/duplicate/wait、装载、callback 和 present 提交协调。

[host](architecture/host-graphics.md) 现拥有 UI、AudioQueue、CoreText 与原生 Vulkan/Metal 的 Rust FFI 和资源，guest adapter 负责内存布局、消息和错误映射。[Windows printf](architecture/windows-printf.md) 四项契约明确 cdecl/stdcall 与按需变参，PE32 ABI 显式 legacy 例外剩余 23 项。

普通契约和资源边界提取按影响选择行为回归及必要构建，不逐批跑性能或游戏矩阵，见 [验收策略](architecture/validation-tools.md)。历史性能拒绝、回滚、[后台 Unity 结束后的复核](reports/refactoring-post-unity-performance-2026-10-19.md)和 [CLI 验收](reports/refactoring-cli-followup-2026-10-19.md)保留，当前结构重构不宣称新的性能提升。

## 重构范围

当前累计冻结包含[对话框](architecture/dialog.md)、[菜单](architecture/menus.md)、[GDI 对象/像素/字体](architecture/gdi-operations.md)、日志、五项 paint、控件初始化、资源、启动信息、环境接口与 CLI 拆分；独立历史证据继续可查。

| 阶段 | 已交付 | 保留项或待验 |
| --- | --- | --- |
| R0/R1 | 冻结输入、原创正式 fixture、领域测试 harness、GUID 唯一契约与双架构适配 | 历史基线和外部输入要求保留 |
| R2 | 919 项 export 契约、冷绑定、架构 ABI/执行策略；Win64 普通 export 元数据已收敛 | PE32 CRT/专用对象方法和 23 项 ABI 例外明确走 legacy，见收尾清单 |
| R3 | environment、modules、files、同步/线程、USER、COM/字符串、图形等资源 owner；普通 handler 仅借实际资源 | 跨 owner 的生命周期协调与 PE32 KernelObjects32 混合句柄表保留，不统一 arena |
| R4 | Process32 拥有执行/调度、DLL/TLS、USER callback、SEH 与 CRT 编排 | Cpu 公开兼容委托和各家族独立恢复策略保留 |
| R5 | owned host UI、事件消费、GDI/像素边界，audio/font/native graphics Rust owner | 原生窗口/GPU/字体已有证据；AudioQueue 输出打开失败，真实音频验收未完成 |
| R6 | 按用户要求排除 | 不重建原生 Mono 路径 |
| R7 | 稳定领域目录、测试 harness、CLI、perf/inventory/profile/probe 工具及当前/历史文档入口 | 公开 facade 保留；第三方/native 构建路径保持 |

已迁移的 export 是契约覆盖范围；typed、ABI-only 与 absent 的架构策略分别记录，不能按数量推导完整 Windows 兼容性。

| 已验收家族 | export 数 | 契约与证据 |
| --- | ---: | --- |
| GUID | 6 | [新增 API 指引](architecture/adding-api.md)及[资源辅助验收](reports/refactoring-guid-allocation-2026-10-18.md) |
| Windows printf | 4 | [cdecl/stdcall 固定前缀与按需变参](architecture/windows-printf.md) |
| 异常 / 展开 / function-table | 11 | [架构策略、控制结果与注册 owner](architecture/exceptions.md) |
| Shell / FormatMessage | 6 / 2 | [目录、命令行、消息准备与分配协调](architecture/shell-and-messages.md) |
| 进程创建 / 退出 / 终止 | 3 | [控制结果与 child preflight](architecture/process-control.md) |
| DXGI / D3D 创建 | 7 | [provider 差异、标量 ABI 与 typed 创建](architecture/graphics-contracts.md) |
| 文件版本资源 | 6 | [PE 字节解析与 guest 输出边界](architecture/version-resources.md) |
| 不可用 MCI / wave 与字体资源 | 6 / 6 | [错误、输出与架构契约](architecture/unavailable-endpoints.md) |
| identity / version / NLS | 6 / 8 / 42 | [环境与架构](architecture/runtime.md)、[版本](architecture/version.md)、[NLS](architecture/nls.md) |
| module | 31 | [模块](architecture/modules.md) |
| file | 58 | [文件及标准句柄](architecture/files.md)；包含 metadata、目录/变更、枚举、section、ANSI profile 与 endpoint |
| Interlocked / SLIST | 6 | [同步原语边界](architecture/interlocked.md) |
| 路径辅助 / 通用句柄 | 15 / 7 | [路径与句柄](architecture/paths-handles.md) |
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
| 系统信息 / 内存状态 / processor topology | 5 | [布局与调用边界](architecture/system-profile.md)及[补齐验收](reports/refactoring-scalar-followup-2026-10-20.md) |
| 日历 / 时区 / CPU 计时 | 11 | [调用与宿主边界](architecture/calendar.md)及[验收](reports/refactoring-calendar-2026-10-16.md) |
| 多媒体计时器 | 3 | [调用与资源边界](architecture/multimedia.md)及[验收](reports/refactoring-multimedia-2026-10-16.md) |
| SRW / 条件变量 / InitOnce | 12 | [状态与调度边界](architecture/inline-sync.md)及[验收](reports/refactoring-inline-sync-2026-10-16.md) |
| Critical section | 8 | [调用与调度边界](architecture/critical-sections.md)及[验收](reports/refactoring-critical-sections-2026-10-16.md) |
| 地址等待 | 3 | [队列与线程状态边界](architecture/address-waits.md)及[验收](reports/refactoring-address-waits-2026-10-17.md) |
| 线程查询 / 设置 | 10 | [调度状态与调用边界](architecture/thread-properties.md)及[验收](reports/refactoring-thread-properties-2026-10-17.md) |
| 线程创建 / 退出 / APC / CONTEXT / description | 6 | [控制结果与资源边界](architecture/thread-control.md)及[验收](reports/refactoring-thread-control-2026-10-17.md) |
| COM / WinRT 初始化、工厂、task memory / Ole 策略 | 10 | [调用与资源边界](architecture/com-core.md)及[验收](reports/refactoring-com-core-2026-10-17.md) |
| WinRT HSTRING / activation | 11 | [字符串与资源边界](architecture/winrt.md)及[验收](reports/refactoring-winrt-2026-10-17.md) |
| BSTR | 8 | [字符串与资源边界](architecture/bstr.md)及[验收](reports/refactoring-bstr-2026-10-17.md) |
| Variant | 4 | [调用与资源边界](architecture/variants.md)及[验收](reports/refactoring-variant-2026-10-18.md) |
| 基础字符串 / 指针与 MulDiv / 进程查询 | 14 / 7 / 11 | [基础 helper 边界](architecture/basic-helpers.md)及[验收](reports/refactoring-basic-2026-10-18.md) |
| 键盘状态 / 布局 | 3 / 13 | [键盘边界](architecture/keyboard.md)及[验收](reports/refactoring-keyboard-2026-10-18.md) |
| raw input / IME | 7 / 17 | [输入能力边界](architecture/input-profile.md)及[验收](reports/refactoring-input-profile-2026-10-18.md) |
| 矩形 / 窗口边框 | 10 / 3 | [几何边界](architecture/geometry.md)及[验收](reports/refactoring-geometry-2026-10-18.md) |
| 游标 / 指针能力 | 10 | [调用与状态边界](architecture/cursor.md)及[验收](reports/refactoring-cursor-2026-10-18.md) |
| 图标 / 光标资源 | 8 | [资源边界](architecture/images.md)及[验收](reports/refactoring-images-2026-10-18.md) |
| 显示 / monitor / metrics | 16 | [显示边界](architecture/display.md)及[验收](reports/refactoring-display-2026-10-18.md) |
| GDI DC / 设备能力 | 7 | [DC 边界](architecture/gdi-dc.md)及[验收](reports/refactoring-gdi-dc-2026-10-18.md) |
| 桌面设置 / DWM | 8 | [桌面边界](architecture/desktop.md)及[验收](reports/refactoring-desktop-2026-10-18.md) |
| 窗口类 / 类信息与字段 | 17 | [类边界](architecture/classes.md)及[验收](reports/refactoring-classes-2026-10-18.md) |
| 注册名称 / 窗口属性 | 14 | [对象边界](architecture/user-objects.md)及[集成验收](reports/refactoring-memory-call-boundary-2026-10-18.md) |
| 窗口查询 / 字段与坐标 | 25 | [查询边界](architecture/window-queries.md)及[集成验收](reports/refactoring-window-queries-2026-10-18.md) |
| 窗口文本 / 焦点与 capture | 16 | [状态边界](architecture/window-state.md)及[完整验收](reports/refactoring-window-state-2026-10-18.md) |
| 窗口生命周期 / 过程控制与同步消息 | 17 | [控制边界](architecture/window-control.md)及[完整验收](reports/refactoring-window-control-2026-10-18.md) |
| 消息队列 / 过程分发 / 计时器与消息元数据 | 22 | [消息边界](architecture/messages.md)及[完整验收](reports/refactoring-messages-2026-10-18.md) |
| 对话框控件 / 窗口枚举 / MessageBox | 24 | [对话框边界](architecture/dialog.md)及[累计验收](reports/refactoring-api-log-2026-10-19.md) |
| 菜单资源与操作 | 27 | [菜单边界](architecture/menus.md)及[累计验收](reports/refactoring-api-log-2026-10-19.md) |
| GDI 对象 / 像素 / 字体 | 31 | [图形操作边界](architecture/gdi-operations.md)及[累计验收](reports/refactoring-api-log-2026-10-19.md) |
| paint | 5 | [绘制边界](architecture/painting.md)及[累计复核](reports/refactoring-post-unity-performance-2026-10-19.md) |
| comctl32 初始化 | 2 | [窗口类边界](architecture/classes.md)及[累计复核](reports/refactoring-post-unity-performance-2026-10-19.md) |
| PE32 资源 | 10 | [资源边界](architecture/resources.md)及[累计复核](reports/refactoring-post-unity-performance-2026-10-19.md) |
| 启动信息 | 5 | [进程信息边界](architecture/process-startup.md)及[累计复核](reports/refactoring-post-unity-performance-2026-10-19.md) |
| 环境接口 | 9 | [环境边界](architecture/environment.md)及[累计复核](reports/refactoring-post-unity-performance-2026-10-19.md) |
| Toolhelp 快照 | 7 | [快照边界](architecture/snapshots.md)及[契约验收](reports/refactoring-snapshot-contract-2026-10-19.md) |
| token 查询 | 3 | [token 边界](architecture/tokens.md)及[契约验收](reports/refactoring-token-contract-2026-10-19.md) |
| SID | 9 | [适配器边界](architecture/sids.md)及[验收](reports/refactoring-sid-contract-2026-10-20.md) |
| ETW | 8 | [provider 边界](architecture/tracing.md)及[契约验收](reports/refactoring-etw-contract-2026-10-20.md) |
| 注册表 | 17 | [领域边界](architecture/registry.md)及[契约验收](reports/refactoring-registry-contract-2026-10-20.md) |
| BCrypt RNG | 3 | [领域边界](architecture/bcrypt.md)及[验收](reports/refactoring-bcrypt-2026-10-20.md) |
| SSPI | 34 | [领域边界](architecture/sspi.md)及[验收](reports/refactoring-sspi-2026-10-20.md) |
| WinHTTP | 5 | [调用边界及验收](architecture/winhttp.md) |
| HID / SetupAPI / 设备通知 | 24 | [契约与资源边界及验收](architecture/devices.md) |
| Winsock 启动 / 地址值 / 事件 | 20 | [契约、资源及调度边界](architecture/winsock.md) |
| 进程错误模式 / 启动属性 | 5 | [资源与线程继承边界](architecture/process-options.md) |
| 系统 / 临时目录查询 | 10 | [环境默认值与查询边界](architecture/system-paths.md) |

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

当前阶段整合为 release **695 passed / 0 failed / 8 ignored / 21 suites**，包含既有 Unicorn 参照测试；Python 工具 **31 项**、fmt、release strict all-targets/all-features Clippy 与 build 通过。当前 Cargo 无 native Mono feature，不重建已撤回的原生桥接。额外 debug、真实 native 和游戏前缀的历史结果仍按各批报告解释，不重复记为本次执行。

最近一次累计性能复核为 2026-10-19 的 [32 条协议、288 次执行与三对 Alice 前缀](reports/refactoring-post-unity-performance-2026-10-19.md)；token、快照、CLI、后台负载和失败候选的原始结果仍由 [文档入口](index.md)定位。当前代码提取未改执行循环、memory/cache 或 native 算法，依据用户要求不重复长性能矩阵。

后续性能测试先按风险评估：普通契约迁移和 owner 封装优先定向正确性及必要构建；涉及执行器、memory/cache、热网关、热点布局或实际绘制/启动路径时，选择受影响负载比较。原 3%/5% 门槛不再作为普通结构改动的准入标准。正式计时仍保持稳定启动协议、完整哈希和全部轮次，编译、采样与计时串行，见 [验证选择](architecture/validation-tools.md)。

必需检查：

```sh
cargo fmt --all -- --check
CARGO_INCREMENTAL=0 cargo test --release --all-targets --locked
CARGO_INCREMENTAL=0 cargo clippy --release --all-targets --all-features --locked -- -D warnings
python3 -B -m unittest discover -s tools -p 'test_*.py'
```

八个默认 ignored 均有外部条件：两项 Metal/software 像素对照需真实 Metal device；三项 native GPU 资源/Shader/owner 及两项 Win64 DXGI draw/swapchain 需 native graphics、MoltenVK 与原创 DXBC/GS fixture；新增一项 PCM 输出需可打开的 AudioQueue。音频 fixture 在 open 时失败，后续计数/重复关闭断言未执行，原始 C 主线程探针也无法打开，验收仍待确认。其它 native 的历史显式通过不代表本次全部重跑，ignored 不计为通过。Unicorn 参照测试在受限 macOS 沙箱可能触发 SIGILL，本批全目标在允许参照执行的环境通过；发布运行时不使用 Unicorn。

## 证据存储

本轮冻结 executable、source/input manifest、完整测试日志和性能轮次位于 `wire-out/refactoring-2026-10-14/`。该目录日期是证据批次标识，后续日期继续复用。只冻结小型运行时与源码，游戏素材通过原路径/软链接复用，不复制或硬链接。历史候选拒绝、布局/调度调查及全部性能报告统一从 [文档入口](index.md) 查阅。
