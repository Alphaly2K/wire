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
- [日历、时区与 CPU 计时调用边界](architecture/calendar.md)及[契约验收](reports/refactoring-calendar-2026-10-16.md)：11 项接口、时区独立模块、宿主 CPU 计时、32 条性能路径及局部成本调查。
- [多媒体计时器调用与资源边界](architecture/multimedia.md)及[契约验收](reports/refactoring-multimedia-2026-10-16.md)：三项接口、私有 period owner、静态/动态 provider 差异、35 条性能路径与同哈希扩样。
- [release 编译单元候选](reports/refactoring-release-codegen-2026-10-15.md)：独立构建配置、同二进制对照及长样本调查。
- [网关边界候选](reports/refactoring-gateway-boundary-2026-10-15.md)、[页缓存内联候选](reports/refactoring-page-cache-inline-2026-10-15.md)、[helper 布局候选](reports/refactoring-memory-helper-layout-2026-10-15.md)：已撤回实现和保留的测量证据。
- [本次渐进重构记录](reports/refactoring-2026-10-14.md)：阶段交付、行为差异与验收结果。
- [R7 稳定边界整理](reports/refactoring-r7-2026-10-14.md)：PE32 目录、领域测试、freeze/replay 工具及完整对照。
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
