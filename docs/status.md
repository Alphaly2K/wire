# 当前状态

更新日期：2026-10-20。按 [重构计划](refactoring-plan-2026-10-11.md) 持续推进，重构前基线为 `2e78377`，当前累计 **915 项 export 契约**。家族明细见下表，完整提交与历史验收见 [重构记录](reports/refactoring-2026-10-14.md)。Mono 原生化已撤回，R6 按用户要求排除。

近期完成 [GDI+](architecture/gdiplus.md)、[WMI](architecture/wmi.md)、[DirectSound](architecture/directsound.md) 与 [D3D9](architecture/d3d9.md) 的资源借用收敛，移除对象调用对整个 ApiState 的依赖及 registry/backend 临时 take；[PE32 线程创建和退出](architecture/thread-control.md)、[USER 资源退休](architecture/window-control.md)、[callback 执行](architecture/callbacks32.md) 与 [消息队列](architecture/messages.md) 也已归入各自 owner。Win64 [TLS/FLS](architecture/locals.md) 私有拥有固定槽和 callback identity，保留 TEB 与析构顺序。

最新阶段整合覆盖至 Win64 消息与 host USER owner（上一提交 `c551314`）：release 全目标 **694 passed / 7 ignored / 21 suites**，all-targets/all-features strict Clippy、build 及 fmt 通过。工具未改，沿用前次 **31 项工具测试**；真实 Metal 两项已在 D3D9 批次显式通过。按改动影响选择验证，普通契约和资源边界提取不逐批重跑性能或游戏矩阵；执行循环、ISA 算法和 callback frame 保持。见 [验收策略](architecture/validation-tools.md)。

历史性能拒绝、回滚与后续复核证据继续保留，见 [API 日志累计验收](reports/refactoring-api-log-2026-10-19.md)、[后台 Unity 渲染结束后的复核](reports/refactoring-post-unity-performance-2026-10-19.md) 和 [CLI 重试](reports/refactoring-cli-followup-2026-10-19.md)。当前结构重构不宣称新的性能提升。

Win64 [Heap](architecture/heaps.md)、[Local/Global](architecture/local-memory.md)、[BSTR](architecture/bstr.md)、[HSTRING](architecture/winrt.md)、[COM](architecture/com-core.md) 与[窗口查询/状态](architecture/window-queries.md)普通调用已收窄到各自 owner 与明确资源借用，保留原分配、输出、LastError 和诊断顺序。

PE32 [窗口状态](architecture/window-state.md)、[对话框/枚举](architecture/dialog.md)、[消息](architecture/messages.md)、[monitor](architecture/display.md)和[绘制](architecture/painting.md)均由领域准备请求、Process 执行 callback 或阻塞；原重入重查、临时资源释放、输出 fault 与返回帧策略保持。新增的通知重入和 monitor fault 释放 fixture 已纳入完整 **257 项 Win32** 回归。

随后[窗口几何/更新](architecture/window-control.md)也完成准备→回调→提交拆分，默认 WM_MOVE/WM_SIZE 之间保留重查。新增独立过程替换/最小化 fixture 后，完整 Win32 **258**、IR 31 和必要构建通过。

窗口创建/销毁、显示、同步发送与默认过程也已完成请求边界，`src/user32/` 内直接 CPU/guest 执行依赖清零；Process 保留原 callback/cleanup 循环，完整 Win32 258、IR 31 和必要构建通过。其余 DLL/SEH/CRT 恢复边界继续按原策略迁移。

[CRT 编排](architecture/crt32.md)随后迁入 Process，API 不再处理 FPU/初始化器执行循环；qsort 与 initializer 保留各自恢复策略。新增函数表变更/首错误 fixture，完整 Win32 **259**、IR 31 和必要构建通过。

Win64 的 cursor/DC/desktop 调用也归 User owner，明确借 tid、内存与错误槽；完整 Win64 269、library 61 passed / 7 ignored、fmt/strict Clippy/build 通过。

Win64 class/image 调用随后归 User，PE 资源读取只借内存/Modules；原遍历和资源验证顺序保持，完整 Win64 269、library 61 passed / 7 ignored 及必要构建通过。

Win64 键盘状态/布局与默认过程标量调用也归 User，只读借用 Threads，并保留错误分支才读取 TEB；callback frame 和 start/resume 主体保持。完整 Win64 269、library 61 passed / 7 ignored、fmt/strict Clippy/build 通过。

PE32 SEH 搜索/展开和普通 filter 现归 Process，CONTEXT codec 独立借内存/ThreadCtx；原恢复策略与公开 seh 入口保持，完整 Win32 259、IR 31 和必要构建通过。

## 重构范围

当前累计冻结包含[对话框](architecture/dialog.md)、[菜单](architecture/menus.md)、[GDI 对象/像素/字体](architecture/gdi-operations.md)、日志、五项 paint、控件初始化、资源、启动信息、环境接口与 CLI 拆分；独立历史证据继续可查。

| 阶段 | 已交付 | 剩余范围 |
| --- | --- | --- |
| R0/R1 | 冻结输入、原创正式 fixture、领域测试 harness、GUID 唯一契约和双架构适配 | 随后续家族维护独立基线 |
| R2 | 已接入 915 个 export 的契约；明细见下表 | PE32 legacy 家族与图形 COM 方法 |
| R3 | environment、双架构 modules/apartment/BSTR/cursor、Win64 files/同步队列/TLS-FLS/task/HSTRING/USER/设备等 owner；PE32 kernel/scratch/CRT、线程创建/退出、USER 队列/退休及 GDI+/WMI/DirectSound/D3D9 窄资源调用 | 其余领域资源与 COM 对象；PE32 kernel 大范围抽取曾因性能撤回 |
| R4/R5 | Process32 编排、线程退出与 callback 执行服务、owned host UI 请求、GDI 与像素状态分离 | 领域 callback 请求与其余分发/owner 继续收敛 |
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
