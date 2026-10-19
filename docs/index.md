# Wire 文档入口

## 当前实现

- [当前状态与验证矩阵](status.md)：注明代码基线、运行入口与外部测试条件。
- [架构和依赖方向](architecture/runtime.md)：当前边界、保留 facade 与迁移目标。
- [宿主与图形边界](architecture/host-graphics.md)：owned 请求、事件消费顺序、GDI 适配及 native 验证入口。
- [新增 API 与 GUID 契约](architecture/adding-api.md)：唯一声明、冷绑定、双架构错误和提交顺序。
- [Windows version 契约](architecture/version.md)：架构 provider 范围、ABI-only 策略与故障提交。
- [NLS/locale 契约](architecture/nls.md)：转换、字符分类、日历与进程回调的边界。
- [测试与性能工具](architecture/validation-tools.md)：领域测试名映射、公共测量库、freeze 输入和兼容入口。
- [稳定启动测量协议](reports/refactoring-launch-protocol-2026-10-16.md)：相同哈希副本的偏差调查、可选 argv/cwd 协议和跨模式证据隔离。
- [模块身份与生命周期](architecture/modules.md)：31 个 export 的冷绑定契约、动态查询差异、私有 owner 与引用/pin 提交。
- [PE32 资源查询与字符串](architecture/resources.md)及[迁移记录](reports/refactoring-resources-2026-10-19.md)：十项 ResourceApi、只读 image 视图、编码/故障/返回槽边界；正确性及累计性能通过。
- [模块 API 契约验收](reports/refactoring-module-catalog-2026-10-14.md)：模块 catalog、控制请求与独立性能调查。
- [文件状态与标准句柄](architecture/files.md)：Win64 filesystem、属性元数据、endpoint/alias 寿命和输出提交顺序。
- [核心文件 API 契约与验收](reports/refactoring-file-catalog-2026-10-15.md)：行为基线、失败候选、布局/调度调查与最终接入。
- [路径与属性 API 契约验收](reports/refactoring-file-path-catalog-2026-10-15.md)：10 个追加 export、独立故障基线、31 条路径及整数长样本调查。
- [文件元数据 API 契约验收](reports/refactoring-file-metadata-catalog-2026-10-15.md)：五个追加 export、ABI-only 冷绑定与完整性能对照。
- [目录与文件变更 API 契约验收](reports/refactoring-file-mutation-catalog-2026-10-15.md)：12 个追加 export、原架构能力范围与错误顺序、34 条性能路径及事件扩样。
- [目录枚举 API 契约验收](reports/refactoring-file-search-catalog-2026-10-15.md)：7 个追加 export、句柄类型、游标提交、返回槽别名与独立 FindClose fixture。
- [路径与句柄边界](architecture/paths-handles.md)与[完整验收](reports/refactoring-paths-handles-2026-10-15.md)：19 个 export、故障/资源提交、历史拒绝候选及新基线上的 46 条性能路径。
- [文件 section 契约验收](reports/refactoring-file-section-catalog-2026-10-15.md)：八项映射 API、首轮暂缓、顺序平衡补样和保留的 raw 局部成本。
- [Interlocked/SLIST 边界](architecture/interlocked.md)与[契约验收](reports/refactoring-interlocked-catalog-2026-10-15.md)：六项契约、故障提交、25 条性能路径和 SSE/thread-ID 调查。
- [时钟调用边界](architecture/clocks.md)与[契约验收](reports/refactoring-clocks-2026-10-15.md)：六项契约、架构返回/故障顺序、稳定启动协议下的 49 条路径与长样本。
- [进程身份与 LastError 边界](architecture/context.md)及[契约验收](reports/refactoring-context-2026-10-16.md)：七项契约、伪句柄/TEB/PEB、25 条性能路径及 SSE 长样本。
- [TLS/FLS 边界](architecture/locals.md)及[契约验收](reports/refactoring-locals-2026-10-16.md)：八项接口、架构槽位/回调差异、28 条性能路径与 memory/data-alias 成本调查。
- [PE32 TLS/FLS 状态归属](reports/refactoring-locals32-owner-2026-10-16.md)：私有 map/索引组件、28 条性能路径、context 顺序扩样与启动前缀。
- [事件与信号量边界](architecture/sync-objects.md)及[契约验收](reports/refactoring-sync-objects-2026-10-16.md)：16 项接口、架构命名/权限/故障差异、31 条性能路径和 module 长样本。
- [Mutex 所有权与释放边界](architecture/mutexes.md)及[契约验收](reports/refactoring-mutexes-2026-10-16.md)：七项接口、递归 owner/错误/唤醒顺序、33 条性能路径和启动前缀。
- [WaitFor* 进程控制边界](architecture/waits.md)及[契约验收](reports/refactoring-waits-2026-10-16.md)：四项接口、阻塞/APC/数组及返回槽故障、36 条性能路径和 local 长样本。
- [Sleep/线程让出边界](architecture/delays.md)及[契约验收](reports/refactoring-delays-2026-10-16.md)：三项接口、返回帧与调度/APC 顺序、24 条性能路径和启动前缀。
- [Waitable timer 边界](architecture/timers.md)及[契约验收](reports/refactoring-timers-2026-10-16.md)：九项接口、命名/access/deadline 与故障顺序、动态查询收敛、25 条性能路径。
- [虚拟内存调用边界](architecture/virtual-memory.md)及[契约验收](reports/refactoring-virtual-memory-2026-10-16.md)：八项接口、架构位宽/查询布局/返回槽差异、31 条性能路径与同哈希对照。
- [Heap 资源与调用边界](architecture/heaps.md)及[契约验收](reports/refactoring-heaps-2026-10-16.md)：13 项接口、私有堆/slab 所有权、ABI-only/absent 策略及 29 条性能路径。
- [Local/Global 共享资源与调用边界](architecture/local-memory.md)及[契约验收](reports/refactoring-local-memory-2026-10-16.md)：14 项接口、成对资源共享、错误及返回顺序、28 条性能路径。
- [系统信息与内存状态调用边界](architecture/system-profile.md)及[契约验收](reports/refactoring-system-profile-2026-10-16.md)：四项接口、双架构布局、public facade、31 条性能路径与路由顺序调查。
- [进程启动信息](architecture/process-startup.md)及[迁移记录](reports/refactoring-process-startup-2026-10-19.md)：五项命令行/STARTUPINFO/feature 契约，保留双架构布局与返回帧差异；正确性及累计性能通过。
- [环境变量与环境块](architecture/environment.md)及[迁移记录](reports/refactoring-environment-2026-10-19.md)：九项契约、Win64 私有 owner 与两种分配寿命，正确性及累计性能通过。
- [Win64 ETW provider 所有权](architecture/tracing.md)及[完整验收](reports/refactoring-etw-owner-2026-10-19.md)：注册、退休、disabled header 边界与私有 owner；三项独立旧行为、12 条性能协议、108 次执行及三对游戏前缀通过。
- [Win64 token 所有权](architecture/tokens.md)及[完整验收](reports/refactoring-token-owner-2026-10-19.md)：query access、故障顺序、保护关闭与单调句柄；17 条协议、162 次执行、PE32 局部成本调查及三对启动前缀。
- [token 三项统一契约](reports/refactoring-token-contract-2026-10-19.md)：静态/动态 provider 差异、PE32 ABI-only；累计 716 exports，按影响执行正确性和构建，不重复性能矩阵。
- [SID 九项契约与适配器](architecture/sids.md)及[验收](reports/refactoring-sid-contract-2026-10-20.md)：显式内存/错误槽、BYTE/DWORD、guard 与提交顺序；累计 725 exports。
- [ETW 八项统一契约](reports/refactoring-etw-contract-2026-10-20.md)：PE32 U64 参数槽、Win64 typed ID 与 provider 范围；累计 733 exports，三批整合 release 664 项通过。
- [Win64 注册表所有权](architecture/registry.md)及[验收](reports/refactoring-registry-owner-2026-10-20.md)：独立资源与处理函数、共用名称解码、生命周期及输出顺序；按影响范围验证，无性能矩阵。
- [注册表 17 项统一契约](reports/refactoring-registry-contract-2026-10-20.md)：两架构受限能力、provider/ABI 与 typed ID，累计 750 exports；领域正确性及构建检查通过。
- [BCrypt RNG 契约与所有权](architecture/bcrypt.md)及[验收](reports/refactoring-bcrypt-2026-10-20.md)：三项契约、私有 RNG 句柄集合与原 OS entropy；catalog 改为固定类型深度的分组迭代，累计 753 exports。
- [SSPI 契约与函数表所有权](architecture/sspi.md)及[验收](reports/refactoring-sspi-2026-10-20.md)：34 项契约、独立 A/W 表与冷阶段网关注册；保留空安全包能力范围，累计 787 exports。
- [WinHTTP 调用边界及验收](architecture/winhttp.md)：五项契约、无状态受检适配器，保持 proxy profile 和 transport 不可用语义；累计 792 exports。
- [Win64 Toolhelp 快照](architecture/snapshots.md)及[完整验收](reports/refactoring-snapshot-owner-2026-10-19.md)：冻结线程值、游标/输出故障、保护关闭；19 条协议、180 次执行及独立累计 IR 复核。
- [Toolhelp 七项统一契约](reports/refactoring-snapshot-contract-2026-10-19.md)：双架构 provider/ABI 和 typed ID；累计 713 exports，17 个计时协议、162 次执行及三对启动前缀，保留代码别名局部成本。
- [日历、时区与 CPU 计时调用边界](architecture/calendar.md)及[契约验收](reports/refactoring-calendar-2026-10-16.md)：11 项接口、时区独立模块、宿主 CPU 计时、32 条性能路径及局部成本调查。
- [多媒体计时器调用与资源边界](architecture/multimedia.md)及[契约验收](reports/refactoring-multimedia-2026-10-16.md)：三项接口、私有 period owner、静态/动态 provider 差异、35 条性能路径与同哈希扩样。
- [SRW、条件变量与 InitOnce 边界](architecture/inline-sync.md)及[契约验收](reports/refactoring-inline-sync-2026-10-16.md)：十二项接口、私有 SRW owner、真实线程阻塞重试、36 条性能路径。
- [Critical section 调用与调度边界](architecture/critical-sections.md)及[契约验收](reports/refactoring-critical-sections-2026-10-16.md)：八项接口、双架构部分写入/返回帧差异、真实线程重试与 39 条性能路径。
- [地址等待与线程状态边界](architecture/address-waits.md)及[契约验收](reports/refactoring-address-waits-2026-10-17.md)：三项接口、线程 owner、重试/到达顺序与 PE32 未知 ABI，40 条性能路径。
- [线程查询、设置与状态边界](architecture/thread-properties.md)及[契约验收](reports/refactoring-thread-properties-2026-10-17.md)：九项接口、架构挂起/优先级差异、私有 mode、43 条性能路径与 HeapSize 局部成本调查。
- [线程控制与资源边界](architecture/thread-control.md)及[契约验收](reports/refactoring-thread-control-2026-10-17.md)：六项接口、私有 description/APC queue、退出/CONTEXT 显式结果、46 条性能路径与 SSE 局部成本调查。
- [COM 基础调用与资源边界](architecture/com-core.md)及[契约验收](reports/refactoring-com-core-2026-10-17.md)：十项接口、线程独占 apartment、task 成员归属、公开 facade、49 条性能路径与四项离散扩样。
- [WinRT 字符串与资源边界](architecture/winrt.md)及[契约验收](reports/refactoring-winrt-2026-10-17.md)：十一项接口、私有 HSTRING owner、reference header 重读、静态/动态 provider 差异、50 条性能路径与两项离散扩样。
- [BSTR 字符串与资源边界](architecture/bstr.md)及[契约验收](reports/refactoring-bstr-2026-10-17.md)：八项接口、ordinal alias、双架构私有 owner、Variant/WMI 共享分配、53 条性能路径与 SSE/raw 局部成本。
- [Variant 调用与资源边界](architecture/variants.md)及[契约验收](reports/refactoring-variant-2026-10-18.md)：四项 PE32 接口、BSTR 深拷贝回滚、BYREF 生命周期、Win64 absent、55 条性能路径与五项离散扩样。
- [GUID 资源辅助验收](reports/refactoring-guid-allocation-2026-10-18.md)：三个追加 export、allocator/TaskMemory、entropy 与返回槽别名、63 条初筛路径和保留的 multimedia/VirtualQuery IR 局部成本。
- [基础 helper 边界](architecture/basic-helpers.md)及[完整验收](reports/refactoring-basic-2026-10-18.md)：29 项接口、common_api 移除、ABI spec 数据共享与缓存恢复生成代码；75 个性能协议与游戏前缀验证，保留 memory/SSE 局部成本及历史拒绝候选。
- [键盘边界](architecture/keyboard.md)及[完整验收](reports/refactoring-keyboard-2026-10-18.md)：16 项状态/布局契约，保留静态与动态 provider 差异；32 个性能协议与游戏前缀验证，保留整数 IR 局部成本和离散轮次。
- [Raw input / IME 边界](architecture/input-profile.md)及[完整验收](reports/refactoring-input-profile-2026-10-18.md)：24 项契约、34 个性能协议，保留 FP 和启动前缀局部成本及 SSE 离散复核。
- [矩形与窗口边框](architecture/geometry.md)及[完整验收](reports/refactoring-geometry-2026-10-18.md)：13 项契约、纯算法与架构 codec；30 个性能协议及游戏前缀通过，保留 packed POINT、DPI 错误和返回帧顺序。
- [游标与指针边界](architecture/cursor.md)及[完整验收](reports/refactoring-cursor-2026-10-18.md)：10 项契约、私有游标状态、虚拟裁剪与 tracking 适配；31 个性能协议和游戏前缀通过。
- [图标与光标资源](architecture/images.md)及[完整验收](reports/refactoring-images-2026-10-18.md)：8 项契约、私有资源/句柄 owner 与纯 icon 校验；34 个性能协议及游戏前缀通过。
- [虚拟桌面与显示查询](architecture/display.md)及[完整验收](reports/refactoring-display-2026-10-18.md)：16 项契约、共享 profile/编码与明确 monitor callback 控制；37 个性能协议及游戏前缀通过。
- [GDI DC 与设备能力](architecture/gdi-dc.md)及[完整验收](reports/refactoring-gdi-dc-2026-10-18.md)：七项契约、真实 backing 和私有逻辑 DC owner；40 个性能协议及游戏前缀通过。
- [桌面设置与 DWM](architecture/desktop.md)及[完整验收](reports/refactoring-desktop-2026-10-18.md)：六项契约、进程设置 owner；43 个性能协议、bit-test 补证及游戏前缀通过。
- [窗口类与元数据](architecture/classes.md)及[完整验收](reports/refactoring-classes-2026-10-18.md)：十七项契约、私有 Win64 类 owner 与 PE32 兼容 facade；46 个性能协议、两组 control 补证及游戏前缀通过。
- [USER 名称与属性](reports/refactoring-user-objects-2026-10-18.md)：十四项首版/布局候选拒绝，trace 与窗口查询最终修订一起验收，历史轮次保留。
- [窗口查询边界](architecture/window-queries.md)及[验收记录](reports/refactoring-window-queries-2026-10-18.md)：二十五项范围、六项旧行为回归；与名称/属性共 39 项计入累计。
- [访存调用边界调查与最终验收](reports/refactoring-memory-call-boundary-2026-10-18.md)：修正实际 CLI caller 审计，消除 checked_access 内联增加的栈操作；55 条性能协议及游戏前缀通过，累计 540 项。
- [窗口文本与输入所有权](architecture/window-state.md)及[完整验收](reports/refactoring-window-state-2026-10-18.md)：十六项契约、59 个性能协议和游戏前缀通过，累计 556 项。
- [窗口生命周期与过程控制](architecture/window-control.md)及[完整验收](reports/refactoring-window-control-2026-10-18.md)：十七项契约、69 个性能协议和游戏前缀通过，累计 573 项。
- [消息队列与计时器](architecture/messages.md)及[完整验收](reports/refactoring-messages-2026-10-18.md)：22 项契约、38 个性能协议和游戏前缀通过，累计 595 项。
- [绘制区域与 paint DC](architecture/painting.md)及[拒绝记录](reports/refactoring-painting-2026-10-19.md)：六项迁移因持续浮点性能回退撤回，保留旧行为基线和真实 paint 循环。
- [对话框控件与枚举](architecture/dialog.md)及[独立记录](reports/refactoring-dialog-2026-10-19.md)：22 项随日志修订累计验收，旧冻结证据保留。
- [精确标量 SSE 边界](architecture/scalar-simd.md)及[编译隔离记录](reports/refactoring-scalar-codegen-2026-10-19.md)：隔离纯运算的生成代码，保留原事务与舍入语义，随日志修订累计验收。
- [菜单契约与 owner](architecture/menus.md)及[候选记录](reports/refactoring-menus-2026-10-19.md)：27 项已接入；保留双输出故障、共享资源与 child ID 生命周期，以及真实 RedrawWindow 网关优先级。
- [PE32 typed 分发模式](reports/refactoring-direct-dispatch-2026-10-19.md)：直接枚举模式保持原分支覆盖与处理体，随日志修订累计验收。
- [GDI 对象、像素与字体](architecture/gdi-operations.md)及[独立记录](reports/refactoring-gdi-operations-2026-10-19.md)：31 项唯一契约，图形核心拥有操作枚举，adapter 只借 Graphics/错误槽；随日志修订累计验收，失败冻结不追认。
- [API 日志职责与累计验收](reports/refactoring-api-log-2026-10-19.md)：完整 trace 逐字节保持，33 条性能协议、207 次 fixture 和三对游戏前缀通过，累计 675 项。
- [Unity 渲染结束后的累计复核](reports/refactoring-post-unity-performance-2026-10-19.md)：675 项日志冻结与当前版本的 32 条协议、288 次循环及三对游戏启动通过，新增 31 项及 CLI 完成性能验收，累计 706 项。
- [固定执行路径的测量协议](reports/refactoring-fixed-executable-slot-2026-10-18.md)：调查同哈希副本的数倍耗时差异，新增显式固定 executable/cwd 模式、逐轮 SHA 检查与临时链接回收。
- [release 编译单元候选](reports/refactoring-release-codegen-2026-10-15.md)：独立构建配置、同二进制对照及长样本调查。
- [网关边界候选](reports/refactoring-gateway-boundary-2026-10-15.md)、[页缓存内联候选](reports/refactoring-page-cache-inline-2026-10-15.md)、[helper 布局候选](reports/refactoring-memory-helper-layout-2026-10-15.md)：已撤回实现和保留的测量证据。
- [本次渐进重构记录](reports/refactoring-2026-10-14.md)：阶段交付、行为差异与验收结果。
- [R7 稳定边界整理](reports/refactoring-r7-2026-10-14.md)：PE32 目录、领域测试、freeze/replay 工具及完整对照。
- [R7 CLI 候选](reports/refactoring-cli-2026-10-19.md)：六个函数逐体迁移和 19 组命令兼容通过；性能对照大幅漂移，源码已精确撤回，候选冻结保留。
- [R7 CLI 重试](reports/refactoring-cli-followup-2026-10-19.md)：四个命令模块已接入，七份函数文本等价、21 组命令及启动功能通过；library 保持，累计性能复核通过。
- [R7 离线 inventory](reports/refactoring-inventory-2026-10-19.md)：PE/CLR 解析、运行轨迹和扫描报表各归其职，旧入口保留，24 项工具检查及机器报告对照通过。
- [R7 CPU profile 工具](reports/refactoring-profile-tools-2026-10-19.md)：实现归入 perf 包，旧入口与 mock 保持，真实采样的 JSON/折叠栈/火焰图逐字节对照通过。
- [R7 native 图形探针](reports/refactoring-native-probes-2026-10-19.md)：七个旧入口保留，13 个 C ABI 布局集中；三项真实 native 执行与 11 个输出对照通过。
- [R7 架构测量入口](reports/refactoring-perf-drivers-2026-10-19.md)：三个正式入口与早期 AMD64 工具归入 perf；226 份生成输入及 58 次兼容执行保持。
- [五项 paint 迁移与后台负载](reports/refactoring-painting-followup-2026-10-19.md)：typed 契约接入，完整正确性与 120 次功能对照通过；累计性能复核通过，保留旧宿主控制和主动中断。
- [comctl32 初始化](reports/refactoring-common-controls-2026-10-19.md)：两项签名与注册循环归回 class 领域，完整正确性、54 次功能对照、启动前缀及累计性能通过。
- [PE32 kernel 候选拒绝记录](reports/refactoring-pe32-kernel-rejected-2026-10-14.md)：独立资源故障基线、性能回退调查及运行时代码撤回。
- [IR 执行器布局隔离](reports/refactoring-ir-layout-2026-10-14.md)：macOS 固定对齐、机器码证据与独立性能验收。
- [原生 Mono 撤回](mono-native-rollback-2026-10-14.md)：保留通用优化，继续执行原始 guest Mono。

## 设计与接口

- [IR/AOT 设计](ir-aot-design.md)
- [Win32/Hangover 语义来源](hangover-win32-design.md)
- [Win64 D3D11/DXVK/MoltenVK 边界](d3d11-dxvk-moltenvk-design.md)
- [图形渲染设计](graphics-rendering-design.md)

## 历史实现与测量

带日期报告保留当时的代码、测试计数与终点，不能作为当前 HEAD 的验证结果。
本轮先分类链接，保持旧文档路径可用。

- [解释器 V1](interpreter-implementation-2026-10-09.md)、[解释器 V2](interpreter-implementation-v2-2026-10-09.md)
- [32 位游戏推进](games32-progress-2026-10-07.md)、[AMD64/Unity 推进](amd64-runtime-progress-2026-10-07.md)
- [Unity 资源加载](unity-resource-loading-2026-10-08.md)、[资源加载性能](performance-unity-resource-2026-10-09.md)
- [API inventory](api-inventory-2026-10-08.md)、[图形实现](graphics-progress-2026-10-06.md)
- [测试目录存储约定](test-storage-2026-10-12.md)
