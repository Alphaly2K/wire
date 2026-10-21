# 当前状态

更新日期：2026-10-21。按 [重构计划](refactoring-plan-2026-10-11.md) 的 R0–R5 与 R7 交付及验收已完成，重构前基线为 `2e78377`，重构交付 **919 项 export 契约**；[Alice 接续](reports/alice-execution-2026-10-20.md)新增 SleepConditionVariableSRW、OpenThread、FreeLibraryAndExitThread、GetPointerType、GetPointerInfo 和 GetPointerDevices 后，当前累计 **925 项**。R6 按用户要求排除。分阶段核对与保留项见 [收尾审计](reports/refactoring-closeout-2026-10-20.md)，历史提交与失败记录见 [重构记录](reports/refactoring-2026-10-14.md)。

重构交付基线为 `85ced07`。随后完成条件变量、文件定位/截断与大读取、大纹理/内存容量、OpenThread、FPREM、shader varying 子集，以及 [PE32/Win64 宿主键鼠事件](architecture/input-events.md)、AMD64 内存屏障及 Win64 前台 Raw Input 逻辑键鼠设备和显式启用的鼠标指针消息。输入加入后的完整 release 全目标为 **726 passed / 0 failed / 9 ignored / 23 suites**、工具 **31 项**，all-targets/all-features strict Clippy、release build 和 fmt 通过。真实大纹理、DXGI、shader 接口与原生输入回调的定向验证也通过，详见[执行接续报告](reports/alice-execution-2026-10-20.md)。

Alice 已在 `varying-subset` 重放显示标题菜单，约 **15.56B work** 后遇到 MFENCE。补齐该指令后的 `input-events` 重放持续至 600 秒时间上限，最后周期采样为 **24.05B work**，没有记录新的执行错误；当时菜单交互、完整保存和可玩性尚未验收。 人工试用随后发现 Retina 裁剪和 HeapReAlloc(0x10) 退出，已补齐并通过实际 backing 尺寸/GPU 探针及原地址扩容回归；该次试运行随后在 FreeLibraryAndExitThread 停止，现已补齐。另补齐 native 激活/焦点通知，并通过原创窗口的真实 OS 点击→guest WNDPROC 验证；显式虚拟桌面配置后，用户已确认裁切消失。本轮定位到新输入后端要求 WM_POINTER，现已补齐鼠标指针消息及查询，并通过原创窗口的真实点击验证。实际程序的按钮位已随点击改变，但随后因缺少 Win64 GetDoubleClickTime 闪退；现已启用两端共用的 500ms 查询，用户确认按钮有响应且可以继续推进，但存在卡顿。[同一实例的 20 秒采样](reports/alice-profile-2026-10-21.md)显示解释执行/调度/内存约占主线程样本 76.0%，诊断启动器约 14.6%，原生图形约 8.0%；后续优化需先建立无高频探针基线，再聚焦执行器与浮点、管线缓存和提交同步。

本轮[执行器优化](reports/executor-optimization-2026-10-21.md)已接入 4–32 条混合 checked block、精确 ARM64 单精度快路径、精简标量回滚，以及禁止内联的混合块预算前缀提交。完整 release **731 passed / 0 failed / 9 ignored / 23 suites**，独立 `wire-simd` **2 项**、strict Clippy、fmt 和 build 通过。最终三版轮换对照中，600M 混合 SSE/访存 **1.967×**、访存 **1.076×**、间接调用 **0.991×**；相对中间精简版，目标负载再快约 **5.24%**，五个控制负载约在 2% 内。回退候选及所有离散轮次保留，不推导游戏 FPS 倍数。最终正常入口的一段绘制采样为约 **8.04 Present/s**（140.2 秒计数窗口），20 秒主采样的解释器/调度/内存约 **68.4%**、图形约 **29.7%**；已识别的 APFloat 运算调用约 **1.15%**，含回退和快路径的数学包装入口约 **4.87%**。不同运行阶段不能作游戏 AB，仍未确认可玩性。44.456B work 遇到的 `OMGetRenderTargets` 已补齐。最终实例 `interactive-executor-nscpv87h` 使用正常入口，关闭高频 watch、输入 trace 和 API inventory，沿用原 Users。

[音频复查](reports/refactoring-audio-validation-2026-10-20.md)确认当前工具受限执行时 AudioQueueStart 返回 InvalidDevice（−66680），同一个 C 探针在沙箱外成功；加强后的 Rust fixture 实际消费全部 2048 个静音样本，并验证重复关闭和重开。此前失败日志保留，音频待验项已关闭。

PE32 [Process](architecture/callbacks32.md) 已拥有 API 控制、USER callback、DLL/TLS 生命周期、线程退出、SEH 与 CRT 编排，各自恢复策略保留。CPU 单步/IR 不依赖这些服务，公开 Cpu::run/call_guest 委托仍兼容。USER 与 Modules 的领域 owner 已归入 win32；根模块保留兼容入口。

Win64 普通 [文件/endpoint/section](architecture/files.md)、[Environment](architecture/environment.md)、[TLS/FLS](architecture/locals.md)、[USER](architecture/window-queries.md)、[输入](architecture/input-profile.md)、COM/字符串及图形调用已按实际资源收窄；Runtime 保留跨 owner 的 close/duplicate/wait、装载、callback 和 present 提交协调。

[host](architecture/host-graphics.md) 现拥有 UI、AudioQueue、CoreText 与原生 Vulkan/Metal 的 Rust FFI 和资源，guest adapter 负责内存布局、消息和错误映射。[Windows printf](architecture/windows-printf.md) 四项契约明确 cdecl/stdcall 与按需变参，PE32 ABI 显式 legacy 例外剩余 23 项。

普通契约和资源边界提取按影响选择行为回归及必要构建，不逐批跑性能或游戏矩阵，见 [验收策略](architecture/validation-tools.md)。历史性能拒绝、回滚、[后台 Unity 结束后的复核](reports/refactoring-post-unity-performance-2026-10-19.md)和 [CLI 验收](reports/refactoring-cli-followup-2026-10-19.md)保留，当前结构重构不宣称新的性能提升。

## 重构范围

当前累计冻结包含[对话框](architecture/dialog.md)、[菜单](architecture/menus.md)、[GDI 对象/像素/字体](architecture/gdi-operations.md)、日志、五项 paint、控件初始化、资源、启动信息、环境接口与 CLI 拆分；独立历史证据继续可查。

| 阶段 | 已交付 | 保留项 |
| --- | --- | --- |
| R0/R1 | 冻结输入、原创正式 fixture、领域测试 harness、GUID 唯一契约与双架构适配 | 历史基线和外部输入要求保留 |
| R2 | 919 项 export 契约、冷绑定、架构 ABI/执行策略；Win64 普通 export 元数据已收敛 | PE32 CRT/专用对象方法和 23 项 ABI 例外明确走 legacy，见收尾清单 |
| R3 | environment、modules、files、同步/线程、USER、COM/字符串、图形等资源 owner；普通 handler 仅借实际资源 | 跨 owner 的生命周期协调与 PE32 KernelObjects32 混合句柄表保留，不统一 arena |
| R4 | Process32 拥有执行/调度、DLL/TLS、USER callback、SEH 与 CRT 编排 | Cpu 公开兼容委托和各家族独立恢复策略保留 |
| R5 | owned host UI、事件消费、GDI/像素边界，audio/font/native graphics Rust owner；窗口/GPU/字体及真实 PCM 消费均有 native 证据 | 依赖实际设备的 fixture 默认 ignored，按需在可访问设备的宿主条件下显式执行 |
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
| SRW / 条件变量 / InitOnce | 13 | [状态与调度边界](architecture/inline-sync.md)、[重构验收](reports/refactoring-inline-sync-2026-10-16.md)及[Alice 功能补齐](reports/alice-execution-2026-10-20.md) |
| Critical section | 8 | [调用与调度边界](architecture/critical-sections.md)及[验收](reports/refactoring-critical-sections-2026-10-16.md) |
| 地址等待 | 3 | [队列与线程状态边界](architecture/address-waits.md)及[验收](reports/refactoring-address-waits-2026-10-17.md) |
| 线程查询 / 设置 | 11 | [调度状态与调用边界](architecture/thread-properties.md)及[验收](reports/refactoring-thread-properties-2026-10-17.md) |
| 线程创建 / 退出 / APC / CONTEXT / description | 7 | [控制结果与资源边界](architecture/thread-control.md)及[验收](reports/refactoring-thread-control-2026-10-17.md) |
| COM / WinRT 初始化、工厂、task memory / Ole 策略 | 10 | [调用与资源边界](architecture/com-core.md)及[验收](reports/refactoring-com-core-2026-10-17.md) |
| WinRT HSTRING / activation | 11 | [字符串与资源边界](architecture/winrt.md)及[验收](reports/refactoring-winrt-2026-10-17.md) |
| BSTR | 8 | [字符串与资源边界](architecture/bstr.md)及[验收](reports/refactoring-bstr-2026-10-17.md) |
| Variant | 4 | [调用与资源边界](architecture/variants.md)及[验收](reports/refactoring-variant-2026-10-18.md) |
| 基础字符串 / 指针与 MulDiv / 进程查询 | 14 / 7 / 11 | [基础 helper 边界](architecture/basic-helpers.md)及[验收](reports/refactoring-basic-2026-10-18.md) |
| 键盘状态 / 布局 | 3 / 13 | [键盘边界](architecture/keyboard.md)及[验收](reports/refactoring-keyboard-2026-10-18.md) |
| raw input / IME | 7 / 17 | [输入能力边界](architecture/input-profile.md)及[验收](reports/refactoring-input-profile-2026-10-18.md) |
| 矩形 / 窗口边框 | 10 / 3 | [几何边界](architecture/geometry.md)及[验收](reports/refactoring-geometry-2026-10-18.md) |
| 游标 / 指针能力 | 13 | [调用与状态边界](architecture/cursor.md)、[输入事件](architecture/input-events.md)及[验收](reports/refactoring-cursor-2026-10-18.md) |
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

Alice 接续复用现有 runtime 和原始 guest Mono。在可访问 AppKit/Metal 的宿主条件下，`varying-subset` 已显示 **1024×720** 标题菜单，完成 **12540 Draw / 1122 Present**。MFENCE 修复后，`input-events` 最后周期采样达到 **24,050,000,000 work**，未记录新执行错误；600 秒时间上限触发 SIGTERM，没有最终绘制/退出汇总。Settings/GlobalSave 仍为零字节，未验收完整保存或菜单交互，见[本轮证据](reports/alice-execution-2026-10-20.md)。新增宿主输入支持物理键码/文本、左右修饰键、五键鼠标、拖动及双轴滚轮，raw input/IME 的范围见[输入边界](architecture/input-events.md)。重构期间三对 200M smoke 的 hidden 314×211、Draw/Present 0 是更早的短启动前缀，不能代表长跑终点。

## 验证与性能

当前阶段整合为 release **731 passed / 0 failed / 9 ignored / 23 suites**，包含既有 Unicorn 参照测试；独立 `wire-simd` **2 项**、fmt、release strict all-targets/all-features Clippy 与 build 通过。Python 工具此前 **31 项**通过，本轮未改工具实现，未重复该矩阵。当前 Cargo 无 native Mono feature，不重建已撤回的原生桥接。外部设备测试和游戏重放按各批实际日志解释，不把 ignored 计为通过。

最近一次累计性能复核为 2026-10-19 的 [32 条协议、288 次执行与三对 Alice 前缀](reports/refactoring-post-unity-performance-2026-10-19.md)；token、快照、CLI、后台负载和失败候选的原始结果仍由 [文档入口](index.md)定位。本轮执行器优化完成最终六类原创负载的三版轮换、54 次执行，其中访存、间接调用与目标混合负载采用 600M work；早期两版和被撤回候选的独立轮次也保留，详见[执行器报告](reports/executor-optimization-2026-10-21.md)；真实窗口/GPU 重放的 work、wall time 或绘制次数不作为受控 FPS 或性能提升结论。

后续性能测试先按风险评估：普通契约迁移和 owner 封装优先定向正确性及必要构建；涉及执行器、memory/cache、热网关、热点布局或实际绘制/启动路径时，选择受影响负载比较。原 3%/5% 门槛不再作为普通结构改动的准入标准。正式计时仍保持稳定启动协议、完整哈希和全部轮次，编译、采样与计时串行，见 [验证选择](architecture/validation-tools.md)。

必需检查：

```sh
cargo fmt --all -- --check
CARGO_INCREMENTAL=0 cargo test --release --all-targets --locked
CARGO_INCREMENTAL=0 cargo clippy --release --all-targets --all-features --locked -- -D warnings
python3 -B -m unittest discover -s tools -p 'test_*.py'
```

九个默认 ignored 均有外部条件：两项 Metal/software 像素对照需真实 Metal device；四项 native GPU 资源/Shader/owner/大纹理及两项 Win64 DXGI draw/swapchain 需 native graphics、MoltenVK 与原创 DXBC/GS fixture；一项 PCM 输出需可打开的 AudioQueue。音频已在允许访问输出设备的宿主条件下显式通过；最终 link 图形库的大纹理与两个 DXGI fixture、本轮原创 varying 和原生输入探针也通过。其它 native 的历史显式通过不代表本次全部重跑。Unicorn 参照测试在受限 macOS 沙箱可能触发 SIGILL，本批全目标在允许参照执行的环境通过；发布运行时不使用 Unicorn。

## 证据存储

重构冻结 executable、source/input manifest、完整测试日志和性能轮次位于 `wire-out/refactoring-2026-10-14/`；功能接续使用 `wire-out/alice-resume-2026-10-20/`。目录日期是证据批次标识。只冻结小型运行时与源码，游戏素材通过原路径/软链接复用，不复制或硬链接。各次游戏重放独立保留 Users 前后大小/hash，不静默重置保存文件。历史候选拒绝、布局/调度调查及全部性能报告统一从 [文档入口](index.md) 查阅。
