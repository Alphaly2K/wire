# Runtime 边界

基线：2026-10-14，`2e78377`。维持一个 Cargo crate，先抽职责，再按形成的边界调整目录。

| 当前模块 | 职责和迁移方向 |
| --- | --- |
| `x86::process` | PE32 进程组合根，拥有装载状态、CPU、内存、API、调度器和日志；执行 DLL 通知、API trap、callback 与分时编排 |
| `x86::{cpu,ir,memory,pe,loader}` | PE32 ISA/IR/内存与装载；根 `cpu/ir/mem/pe/loader` 仅重导出旧公开入口。`cpu::process_facade` 委托旧 run/call_guest，单步/段执行保持专用静态路径 |
| `x86::address_space` | PE32 gateway 保留地址、stride 与容量；执行内核直接引用架构布局，`api` 继续公开重导出原常量 |
| `x64::{cpu,memory,pe,process}` | AMD64 指令、内存、装载与进程边界，保持现有 typed 热路径 |
| `api` | PE32 stub 与 ABI 调用 facade；领域契约将集中在 `win32::catalog`，领域语义逐步拥有独立状态 |
| `x64::runtime` | Win64 gateway 与服务组合根；已有 BoundImport 冷绑定和 HotImport 热路由，迁移中保留 |
| `fs`、`codepage`、`simd` | 已共享的纯语义或公共服务，继续复用 |
| `runtime::environment` | 不可变 guest 身份/版本/locale/路径；私有可变环境变量模型，架构 allocator 生命周期独立 |
| `win32::{catalog,kernel,x86,x64}` | GUID、identity、version、NLS 的唯一契约、纯算法及架构适配；locale 回调通过专用控制路由，由进程执行 |
| `host::calendar` | 提供原有宿主 localtime 值；Windows 日期格式和 guest 输出布局由领域/架构层负责 |
| `host::ui`、`win32::{x86::host,gdi::x86}` | host 接收 owned 窗口/帧请求并产出事件；guest adapter 负责 USER 入队、GDI 语义映射及 LastError，公开 platform 路径只作 facade |
| `graphics`、`d3d9`、`gpu` | 像素/对象核心不依赖 ApiState；DIB/font 仍有 PE32 codec，D3D9/DXGI 的其它 guest COM 适配器后续按领域迁移 |
| `main` | CLI、IR 文件读取和报表；PE32 状态构造与执行委托 `Process32` |

目标依赖方向：CLI → Process → Loader/CPU/ABI adapter；ABI adapter → Win32 领域状态 → 纯算法/host 服务。共享 ISA 语义不依赖 Win32 或 host；host 产出事件，由 Process/USER 消费，host callback 不运行 guest CPU。

Win32 契约在装载/lookup 冷阶段解析 DLL/export/alias，产生紧凑领域 ID 和架构 ABI 计划；执行阶段继续静态分发。ABI 已知、路由存在、backend 可用和真实行为已验证分别记录。未迁移条目明确保留 legacy 策略：PE32 未知 ABI 的诊断/零清栈行为与 Win64 拒绝行为不在结构迁移中修改。

CPU 热循环、FLAGS/fault 提交、Memory/cache 所有权、stub 地址和 `.wir v3` 格式保持当前契约。PE32 与 AMD64 内存/CPU 不强制泛型统一；复杂 callback、varargs 和 sret 使用明确的架构适配器。

本轮忽略已撤回的原生 Mono 实验；原始 guest Mono 的普通解释执行和诊断仍保留。

PE32 callback 仍允许领域 handler 经旧 Cpu facade 重入 Process。这一兼容环在迁移控制请求时逐家族解除；facade 的删除条件是内部调用者和外部使用者都已迁到 Process。IR 链引用 `x86::address_space::STUB_BASE` 作为保留地址边界；不能将它误当作可继续链执行的普通指令地址。根目录重导出 facade 的删除条件是公开使用者迁入架构入口；当前不要求 tests/examples 一起换路径。

host/graphics 的调用顺序、所有权与原生验证见 [宿主与图形边界](host-graphics.md)。native 源码和 build.rs 未迁移，保留的第三方构建输入不变。
