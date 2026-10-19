# 当前状态

更新日期：2026-10-20。重构前基线为 `2e78377`；最新十二个 [不可用音频与字体入口](architecture/unavailable-endpoints.md) 接入唯一契约，保留原错误、输出、诊断和架构范围。完整 Win32 255、Win64 modules 13、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过。此前[文件版本资源](architecture/version-resources.md)、PE32 USER/MPR、[kernel endpoint](architecture/process-control.md)、[CRT](architecture/crt32.md)、[同步与等待](architecture/waits.md)等已接入。按影响范围选择验证，本批不重复性能或游戏矩阵。完整阶段与历史验收从 [重构记录](reports/refactoring-2026-10-14.md) 和 [文档入口](index.md) 查阅。

随后 [GDI+ 窄依赖](architecture/gdiplus.md)完成：矩阵、分配与启动令牌操作只借自身状态、内存和 process heap。原数学、指针与分配顺序保持，release GDI+ 4、完整 Win32 255 及 fmt/release lib/bin strict Clippy/build 通过，不跑性能矩阵；catalog 数量不变。

[WMI COM](architecture/wmi.md)随后完成同类收敛：对象 registry 不再临时 take，创建和普通方法只借 gateway table、内存、heap 和 BSTR。release com 过滤 30、完整 Win32 255 及必要构建通过，catalog 数量保持 915。

[DirectSound](architecture/directsound.md)的对象调用随后收窄到 heap 与只读 event/window/Presenter；去掉设备临时 take，保留原 PCM/native pump 和 signal 顺序。release audio 4、mixer 1、完整 Win32 255 及 fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。

最新 [D3D9 资源借用](architecture/d3d9.md)已完成：24 个分配/引用/绑定方法只借 heap，设备层再借只读 window 和 Graphics，移除后端 take。release 图形 11、library 4、显式 Metal 对照 2、完整 Win32 255 及必要构建通过，不跑性能矩阵，累计契约仍 915。

[PE32 线程创建与退出](architecture/thread-control.md)继续收敛：普通创建显式借所属资源，有序线程/进程退出归 Process 并共享清理。完整 release Win32 255、IR 31 及 fmt/release lib/bin strict Clippy/build 通过，DLL/TLS/USER 次序、worker EAX 与非局部转移规则保持，不跑性能矩阵。

最新 [USER 退休 owner](architecture/window-control.md)接入：窗口/线程清理只借 User32、Graphics、Menus 和 Sched，Process 及窗口控制直接调用。完整 release Win32 255、fmt/release lib/bin strict Clippy/build 通过，原跨线程 send 唤醒与菜单/图形释放顺序保持。

本轮累计接入对话框/窗口枚举 22、菜单 27、GDI 对象/像素/字体 31 项，共 **80 项**，累计 **675 项**。统一声明、typed 分发与领域 owner 保持双架构范围、故障提交和既有不可用策略；保留真实 RedrawWindow 网关与公开 facade 的历史差异。精确标量 SSE 已移入独立编译单元，没有近似浮点或新增 ISA 语义。

随后 **31 项契约**：5 项 paint、2 项 comctl32 初始化、10 项 PE32 资源、5 项双架构启动信息、9 项环境接口，完整正确性及累计性能复核通过，当前共 **706 项**。Unity 渲染结束后重新完成 32 条性能协议、288 次执行及三对游戏启动；原后台干扰轮次保留。实现与冻结见[五项 paint](reports/refactoring-painting-followup-2026-10-19.md)、[控件初始化](reports/refactoring-common-controls-2026-10-19.md)、[资源](reports/refactoring-resources-2026-10-19.md)、[启动信息](reports/refactoring-process-startup-2026-10-19.md)与[环境接口](reports/refactoring-environment-2026-10-19.md)。

后续各家族接入明细见下表，当前累计 **915 项**契约。普通结构迁移采用按影响范围选择验证的策略，性能不再逐批执行完整矩阵，见[工具与验收边界](architecture/validation-tools.md)。

中间 GDI 冻结出现 messages/IR 三对持续回退，未准入。实际采样后将 API 日志提取为独立 runtime 服务，并用专用整数格式化保留完整 trace、频率和最近 64 条记录。最终消息 150M IR/raw CPU 中位数相对原消息冻结 −13.210%/−12.110%，GDI 150M IR/raw −16.987%/−13.229%；33 条性能协议、207 次 fixture 执行及三对真实启动前缀通过。旧冻结、失败轮次和同哈希 control 独立保留，没有用其它负载的改善抵消持续回退。

早期绘制六项候选因持续浮点回退撤回，见[历史拒绝报告](reports/refactoring-painting-2026-10-19.md)。当前五项重试保持原 RedrawWindow 网关归属，不追认旧候选。其余 USER、COM、图形和资源 owner 继续迁移。R7 [CLI 拆分已重试接入](reports/refactoring-cli-followup-2026-10-19.md)，完整正确性、命令对照及累计性能通过；旧候选和回滚证据保留。[离线 API inventory](reports/refactoring-inventory-2026-10-19.md)、profile、native 探针和架构性能入口已按职责整理；PE32 工具另修正未知 case 静默替换为整数的问题，最新工具检查 28 项通过，详见[工具边界](architecture/validation-tools.md)。Mono 原生化已撤回，本轮排除。

## 重构范围

当前累计冻结包含[对话框](architecture/dialog.md)、[菜单](architecture/menus.md)、[GDI 对象/像素/字体](architecture/gdi-operations.md)、日志、五项 paint、控件初始化、资源、启动信息、环境接口与 CLI 拆分；独立历史证据继续可查。

| 阶段 | 已交付 | 剩余范围 |
| --- | --- | --- |
| R0/R1 | 冻结输入、原创正式 fixture、领域测试 harness、GUID 唯一契约和双架构适配 | 随后续家族维护独立基线 |
| R2 | 已接入 915 个 export 的契约；明细见下表 | PE32 legacy 家族与图形 COM 方法 |
| R3 | guest environment 与 Win64 变量/环境块 owner、双架构模块状态、Win64 文件、period 请求、SRW、地址等待及 APC 队列、线程标量操作与描述、双架构 apartment/BSTR、Win64 task/HSTRING/image/desktop/ETW/token/快照/注册表成员、PE32 TLS/FLS/kernel 对象/文件窄适配/scratch/CRT 与双架构 cursor、Win64 DC/class、BCrypt/SSPI/设备私有 owner | PE32 其余资源、线程资源创建/退出及 COM 其余对象；PE32 kernel 大范围抽取曾因性能撤回 |
| R4/R5 | Process32 编排边界、owned host UI 请求、GDI 与像素状态分离 | 领域分发与资源 owner 随 R2/R3 继续收敛 |
| R6 | 按用户要求忽略 | 不重建原生 Mono 路径 |
| R7 | CLI 命令职责拆分及性能复核、PE32 内核目录、领域测试、公共性能工具及 profile、离线 inventory、当前/历史文档入口 | 其余工具和目录随领域迁移继续整理 |

已迁移的 export 是契约覆盖范围；typed、ABI-only 与 absent 的架构策略分别记录，不能按数量推导完整 Windows 兼容性。

| 已验收家族 | export 数 | 契约与证据 |
| --- | ---: | --- |
| GUID | 6 | [新增 API 指引](architecture/adding-api.md)及[资源辅助验收](reports/refactoring-guid-allocation-2026-10-18.md) |
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

最新 SSPI 批次：release Win64 **264 passed**、网关单元 **3 passed**，catalog 一致性与 fmt/strict all-target/all-feature Clippy/release build 通过。此前注册表/BCrypt 与公共 catalog 整合的 release 全量 **670 passed / 7 ignored** 保留原上下文；注册表 owner 的 debug 定向 3、ETW 的 debug 定向 6/token 定向 7/工具 31，以及更早完整 debug 501、显式 native 图形 7 也未重复标为本批执行。按风险评估不重跑性能和未改动的 native 图形。各批独立基线与功能证据见对应报告。

七项 Toolhelp 契约完成 17 个计时协议、162 次执行和三对 Alice 前缀。Win64 copyout CPU −12.007%，PE32 ABI-only 快捷路径 IR/raw −82.834%/−77.516%，完整诊断日志逐字节不变。code-alias 900M 保留 +4.286% 的局部成本，执行计数、实际调用栈及热区操作数已核对，具体机制尚未确证；raw 六对 900M 汇总 −0.745%。Alice CPU/wall −1.008%/−0.404%，RSS −752KiB，保持同预算、隐藏窗口及零 Draw/Present。详见[契约验收](reports/refactoring-snapshot-contract-2026-10-19.md)。

快照 owner 完成 19 条协议、180 次执行及三对 Alice 前缀，copyout 循环 CPU +0.864%。AMD64 integer/memory 初筛波动经长样本收敛；data-alias 保留 +2.959% 的局部成本，raw 两轮长样本的六对汇总 +2.391%，全部大幅漂移轮次继续保留。针对 token 阶段的 IR 成本，另将最终冻结直接与 ETW 作三对 900M 比较，累计 −0.061%。Alice CPU/wall −0.247%/−0.598%，RSS +400KiB；同预算、隐藏窗口与零 Draw/Present。详见[快照验收](reports/refactoring-snapshot-owner-2026-10-19.md)。

token owner 追加 17 条协议、162 次执行和三对 Alice 前缀，查询循环 CPU −0.639%。SSE/thread-id 的初筛波动经 900M 扩样收敛；PE32 integer/IR 六对长样本保留 **+3.320%** 的局部成本，已采样并核对执行计数与代码生成，具体机制尚未确证。没有三对均超过 5% 的持续回退。Alice CPU/wall −3.611%/−3.871%，RSS +48KiB，保持同预算、隐藏窗口和零 Draw/Present。详见[token 验收](reports/refactoring-token-owner-2026-10-19.md)。

ETW owner 追加 12 条性能协议、108 次执行与三对 Alice 前缀全部通过；主路径最大 CPU 中位数回退 +2.110%，保留 memory 同哈希漂移。游戏 CPU/wall −0.168%/−0.519%，RSS 中位数约 +1.03MiB，仍为相同预算/隐藏窗口/零 Draw/Present 前缀。完整结果见[ETW 验收](reports/refactoring-etw-owner-2026-10-19.md)。

最新累计复核共 **32 条协议、288 次执行**全部校验；三标签轮换中穿插同一旧 binary 的 control。五个新接口家族的 CPU 中位数 −51.455% 至 +1.099%；PE32 四项计算的 raw/IR 与 AMD64 计算/别名/线程 ID 为 −4.098% 至 +2.574%。保留 AMD64 SSE after/control +3.440% 的局部成本、GDI/IR 同哈希漂移及全部离散轮次，没有持续超过 5% 的回退。fixture RSS 中位数最大增加 112KiB。相对 675 项日志冻结，累计 executable +44,272 bytes，各批独立增量仍在 +24KiB 预算内，没有扩大缓存或资源限额。

三对 Alice 200M 启动前缀 CPU/wall 中位数 **−1.543%/−0.328%**，RSS 中位数 **−256KiB**；保持同一预算、隐藏窗口与零 Draw/Present。首次沙箱无法访问 Metal 的失败独立保留，正式六次使用可访问设备的宿主环境；复用原游戏资源。完整证据见[累计复核](reports/refactoring-post-unity-performance-2026-10-19.md)。旧 Unity/WindowServer 干扰、37 个已校验样本及 1 个主动中断维持历史记录，不与新协议样本混算。

后续是否测性能先按改动风险评估，依据用户 2026-10-19 的要求减少验收粒度。普通契约迁移和 owner 封装优先定向正确性及必要构建，不逐批重跑性能矩阵或无关游戏/native 图形。涉及执行器、memory/cache、热网关、热点布局和实际绘制/启动路径时再选择针对性比较，较大阶段整合后决定累计复核；原 3%/5% 门槛不再作为普通结构改动的准入标准。详见[验证选择](architecture/validation-tools.md)。需要正式计时时继续使用显式 [稳定启动协议](reports/refactoring-launch-protocol-2026-10-16.md)，保留哈希、全部轮次与输入校验，编译、正式计时、采样和 diagnostics 串行，不跨协议或不同 work 汇总。

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
