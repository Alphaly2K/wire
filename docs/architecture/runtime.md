# Runtime 边界

基线：2026-10-14，`2e78377`。维持一个 Cargo crate，先抽职责，再按形成的边界调整目录。

| 当前模块 | 职责和迁移方向 |
| --- | --- |
| `x86::process` | PE32 进程组合根，拥有装载状态、CPU、内存、API、调度器和日志；执行 DLL 通知、API trap、callback 与分时编排 |
| `x86::process::termination`、`win32::x86::thread_creation` | Process 统一有序线程/进程退出；普通线程创建只借对象、模块、heap、错误与 scheduler，late ID fault 的部分提交保持 |
| `x86::{cpu,ir,memory,pe,loader}` | PE32 ISA/IR/内存与装载；根 `cpu/ir/mem/pe/loader` 仅重导出旧公开入口。`cpu::process_facade` 委托旧 run/call_guest，单步/段执行保持专用静态路径 |
| `x86::address_space` | PE32 gateway 保留地址、stride 与容量；执行内核直接引用架构布局，`api` 继续公开重导出原常量 |
| `x64::{cpu,memory,pe,process}` | AMD64 指令、内存、装载与进程边界，保持现有 typed 热路径 |
| `api` | PE32 stub 与 ABI 调用 facade；领域契约将集中在 `win32::catalog`，领域语义逐步拥有独立状态 |
| `x64::runtime` | Win64 gateway 与服务组合根；已有 BoundImport 冷绑定和 HotImport 热路由，迁移中保留 |
| `fs`、`codepage`、`simd` | 已共享的纯语义或公共服务，继续复用 |
| `runtime::environment` | 不可变 guest 身份/版本/locale/路径；私有可变环境变量模型，架构 allocator 生命周期独立 |
| `x64::runtime::environment::Environment` | 私有拥有 Win64 变量和环境块集合，typed adapter 保持页分配、专用释放及故障顺序；PE32 空环境继续使用原 scratch allocator |
| `runtime::api_log` | PE32 完整 API trace、调用频率与最近 64 条诊断；不依赖 CPU、catalog 或 ApiState，`api::ApiLogger` 保留原公开入口 |
| `win32::{catalog,kernel,x86,x64}` | GUID、identity、version、NLS 的唯一契约、纯算法及架构适配；locale 回调通过专用控制路由，由进程执行 |
| `modules::Modules`、`win32::{x86,x64}::modules` | PE32 guest image/TLS 与 provider 身份；Win64 module/export/path/ref/pin/search 的私有 owner。Root 注册入口委托，ABI/control adapters 分别保留 |
| `resource`、`win32::x86::resources` | PE32 映射 image 的受检资源视图与字符串输出；只借内存、EXE 基址和错误槽，不拥有模块寿命 |
| `x64::runtime::files::Files` | Win64 文件、标准 endpoint/alias、受限 filesystem 与属性元数据的私有 owner；路径 mutation 和资源寿命由 owner 执行，ABI/guest 输出由 adapter 提交 |
| `host::calendar` | 提供原有宿主 localtime 值；Windows 日期格式和 guest 输出布局由领域/架构层负责 |
| `host::ui`、`win32::{x86::host,gdi::x86}` | host 接收 owned 窗口/帧请求并产出事件；guest adapter 负责 USER 入队、GDI 语义映射及 LastError，公开 platform 路径只作 facade |
| `graphics`、`d3d9`、`gpu` | 像素/对象核心不依赖 ApiState；D3D9 生命周期只借 heap，设备调用再借 windows/Graphics，DIB/font 仍有 PE32 codec；DXGI 其余适配器继续迁移 |
| `gdiplus`、`gdiplus::calls` | GdiPlus 私有拥有 matrix/allocation/token；普通调用只借内存和 process heap，公开 dispatch 保留兼容签名 |
| `wmi`、`wmi::calls` | Wmi 私有拥有 COM 对象、引用计数、游标和 vtable；调用只借内存、gateway table、heap 与 BSTR，provider 和 HRESULT 保持 |
| `dsound`、`dsound::calls` | DSound 私有拥有对象/buffer、通知和 sink；方法只借内存、gateway、heap 及只读事件/窗口/Presenter，pump 与 signal 次序保持 |
| `main`、`cli` | main 只收集参数并分发；四个命令模块负责参数、静态 image/IR 操作和架构报表，guest 构造与执行委托 Process32/Process64。结构与命令兼容通过，性能待复核，见 [CLI 边界](cli.md) |

目标依赖方向：CLI → Process → Loader/CPU/ABI adapter；ABI adapter → Win32 领域状态 → 纯算法/host 服务。共享 ISA 语义不依赖 Win32 或 host；host 产出事件，由 Process/USER 消费，host callback 不运行 guest CPU。

Win32 契约在装载/lookup 冷阶段解析 DLL/export/alias，产生紧凑领域 ID 和架构 ABI 计划；执行阶段继续静态分发。ABI 已知、路由存在、backend 可用和真实行为已验证分别记录。未迁移条目明确保留 legacy 策略：PE32 未知 ABI 的诊断/零清栈行为与 Win64 拒绝行为不在结构迁移中修改。

CPU 热循环、FLAGS/fault 提交、Memory/cache 所有权、stub 地址和 `.wir v3` 格式保持当前契约。PE32 与 AMD64 内存/CPU 不强制泛型统一；复杂 callback、varargs 和 sret 使用明确的架构适配器。

本轮忽略已撤回的原生 Mono 实验；原始 guest Mono 的普通解释执行和诊断仍保留。

PE32 callback 仍允许领域 handler 经旧 Cpu facade 重入 Process。这一兼容环在迁移控制请求时逐家族解除；facade 的删除条件是内部调用者和外部使用者都已迁到 Process。IR 链引用 `x86::address_space::STUB_BASE` 作为保留地址边界；不能将它误当作可继续链执行的普通指令地址。根目录重导出 facade 的删除条件是公开使用者迁入架构入口；当前不要求 tests/examples 一起换路径。

host/graphics 的调用顺序、所有权与原生验证见 [宿主与图形边界](host-graphics.md)。native 源码和 build.rs 未迁移，保留的第三方构建输入不变。
