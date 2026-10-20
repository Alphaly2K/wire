# Runtime 边界

基线：2026-10-14，`2e78377`。维持一个 Cargo crate，先抽职责，再按形成的边界调整目录。

| 当前模块 | 职责和迁移方向 |
| --- | --- |
| `x86::process` | PE32 进程组合根，拥有装载状态、CPU、内存、API、调度器和日志；执行 DLL 通知、API trap、callback 与分时编排 |
| `x86::process::termination`、`win32::x86::thread_creation` | Process 统一有序线程/进程退出；普通线程创建只借对象、模块、heap、错误与 scheduler，late ID fault 的部分提交保持 |
| `win32::user::pe32`（公开 `user32` facade）、`x86::process::{window,window_geometry,window_state,dialog,messages,painting,display}` | USER 准备状态、资源与 callback/等待请求；Process 执行 guest 与阻塞并完成后置输出。USER 领域文件不依赖 CPU/执行器，创建/销毁、重入重查和部分提交策略各自保持 |
| `x86::process::{crt,crt_sort}`、`win32::x86::{crt,crt_memory}` | Process 负责 CRT CPU/FPU 返回、初始化器与排序 callback；领域负责私有指针状态与普通内存算法。caller-context 与 SEH initializer 恢复策略分别保留 |
| `x86::process::exceptions`、`win32::x86::context` | Process 负责 SEH 搜索/展开与普通 filter；受检 CONTEXT codec 不借进程状态，公开 seh 入口保持兼容 |
| `x86::process::synchronization` | Process 执行 critical-section 阻塞/唤醒、Sleep/让出及对象/消息等待；明确借资源并返回控制结果，API facade 在返回帧提交后才执行让出 |
| `x86::process::module_notifications`、`modules::Modules` | Process 执行 DLL/TLS、线程与退出通知；Modules 提供私有 metadata 的原快照和重查，不暴露 image 表 |
| `x86::process::module_lifecycle` | Process 执行依赖初始化、sidecar IR 与卸载回调；Modules 准备/提交 phase、引用与映射退休，顺序保持 |
| `x86::process::module_loading` | typed 模块 API、失败 callback/清理与 CPU IR 失效编排；Modules 处理私有元数据和资源退休，公开 modules::dispatch 保留兼容 |
| `x86::{cpu,ir,memory,pe,loader}` | PE32 ISA/IR/内存与装载；根 `cpu/ir/mem/pe/loader` 仅重导出旧公开入口。`cpu::process_facade` 委托旧 run/call_guest，单步/段执行保持专用静态路径 |
| `x86::address_space` | PE32 gateway 保留地址、stride 与容量；执行内核直接引用架构布局，`api` 继续公开重导出原常量 |
| `x64::{cpu,memory,pe,process}` | AMD64 指令、内存、装载与进程边界，保持现有 typed 热路径 |
| `api` | PE32 stub 与 ABI 调用 facade；领域契约将集中在 `win32::catalog`，领域语义逐步拥有独立状态 |
| `x64::runtime` | Win64 gateway 与服务组合根；已有 BoundImport 冷绑定和 HotImport 热路由，迁移中保留 |
| `x64::runtime::user::{message,host}` | User 处理消息、队列就绪与宿主事件；只借明确资源。Runtime 协调 LastError 和实际阻塞，窗口 callback frame 保持独立 |
| `x64::runtime::display` | User 执行 Win64 显示 scalar 查询，只借 Memory/只读 Threads；Runtime 保留 monitor callback 的真实资源和执行编排 |
| `x64::runtime::{heap,local_memory,automation,winrt}` | Heap、LocalMemory、Bstrings、Strings 执行各自分配/调用，只借内存与 Pages 的原页记录、游标、错误槽和诊断；owner 保留独立归属与失败提交顺序 |
| `fs`、`codepage`、`simd` | 已共享的纯语义或公共服务，继续复用 |
| `runtime::environment` | 不可变 guest 身份/版本/locale/路径；私有可变环境变量模型，架构 allocator 生命周期独立 |
| `x64::runtime::environment::Environment` | 私有拥有 Win64 变量和环境块集合，typed adapter 保持页分配、专用释放及故障顺序；PE32 空环境继续使用原 scratch allocator |
| `runtime::api_log` | PE32 完整 API trace、调用频率与最近 64 条诊断；不依赖 CPU、catalog 或 ApiState，`api::ApiLogger` 保留原公开入口 |
| `win32::{catalog,kernel,x86,x64}` | GUID、identity、version、NLS 的唯一契约、纯算法及架构适配；locale 回调通过专用控制路由，由进程执行 |
| `modules::Modules`、`win32::{x86,x64}::modules` | PE32 guest image/TLS 与 provider 身份；Win64 module/export/path/ref/pin/search 的私有 owner。Root 注册入口委托，ABI/control adapters 分别保留 |
| `modules::linker::Linker` | 在映射/解析期间明确借 Modules、ABI、USER/scratch/error 和只读路径/命令行资源；不借 ApiState，不执行 CPU，回调前释放借用 |
| `win32::user::pe32::classes` | User32 执行类注册/查询/字段及基本控件初始化，只借内存、gateway、Modules、scratch 和错误槽；公开 find_class 兼容 facade 保留 |
| `win32::user::pe32::queries` | User32 执行 scalar 和桌面查询；桌面首次初始化明确借 Modules、gateway、scratch 和错误槽，ApiState 只组合资源 |
| `resource`、`win32::x86::resources` | PE32 映射 image 的受检资源视图与字符串输出；只借内存、EXE 基址和错误槽，不拥有模块寿命 |
| `x64::runtime::files::Files` | Win64 文件、标准 endpoint/alias、受限 filesystem 与属性元数据的私有 owner；路径 mutation 和资源寿命由 owner 执行，ABI/guest 输出由 adapter 提交 |
| `x64::runtime::{find::Searches,files::directory}` | Searches 执行 FindFirst/Next/Close，Files 编码 metadata 和执行目录 batch 输出；只读借 Threads 在实际错误分支写 LastError，原游标提交顺序保持 |
| `x64::runtime::{events::Events,sync_objects,timers}` | Events 执行事件、信号量和 waitable timer 的普通调用，只借 Memory 与只读 Threads；原命名、权限、计数、deadline 和等待语义保持 |
| `host::calendar` | 提供原有宿主 localtime 值；Windows 日期格式和 guest 输出布局由领域/架构层负责 |
| `host::ui`、`win32::{x86::host,gdi::x86}` | host 接收 owned 窗口/帧请求并产出事件；guest adapter 负责 USER 入队、GDI 语义映射及 LastError，公开 platform 路径只作 facade |
| `win32::user::pe32::host` | User32 准备 PE32 owned 窗口快照并消费宿主事件；adapter 直接借 Presenter 与各资源，删除 Presenter 的临时 take/还原 |
| `graphics`、`d3d9`、`gpu` | 像素/对象核心不依赖 ApiState；D3D9 生命周期只借 heap，设备调用再借 windows/Graphics，DIB/font 仍有 PE32 codec；DXGI 其余适配器继续迁移 |
| `x64::runtime::dxgi::{Graphics,objects}` | Graphics 拥有 vtable、COM 对象分配/adapter 缓存及递归释放；分配明确借内存、Gateways 和共享游标，释放只借内存 |
| `x64::runtime::dxgi::{output,d3d11::draw}` | Graphics 执行虚拟 output、context/binding 引用与 draw 输入准备；分配借 gateway/游标，其余按需只借内存，根保留公开统计 facade |
| `x64::runtime::dxgi::d3d11::Calls` | 创建/COM/input-layout adapter 临时借 Graphics、Gateways、共享游标和 Trace；不借 Runtime64/线程/USER，内存按次传入，private-data 由 Graphics 共用实现 |
| `x64::runtime::dxgi::calls` | Graphics 执行 factory/D3D12 能力创建及普通 factory/adapter/COM；明确借 gateway/游标与必要的只读 User，根保留对象 preflight 和 swapchain 控制路由 |
| `x64::runtime::dxgi::swapchain` | 临时 Calls 借图形资源、分配/trace 和只读 User；Graphics 拥有 buffer/status/frame/计数，根继续协调 present 的 API count、unblock 和 record |
| `gdiplus`、`gdiplus::calls` | GdiPlus 私有拥有 matrix/allocation/token；普通调用只借内存和 process heap，公开 dispatch 保留兼容签名 |
| `wmi`、`wmi::calls` | Wmi 私有拥有 COM 对象、引用计数、游标和 vtable；调用只借内存、gateway table、heap 与 BSTR，provider 和 HRESULT 保持 |
| `dsound`、`dsound::calls` | DSound 私有拥有对象/buffer、通知和 sink；方法只借内存、gateway、heap 及只读事件/窗口/Presenter，pump 与 signal 次序保持 |
| `main`、`cli` | main 只收集参数并分发；四个命令模块负责参数、静态 image/IR 操作和架构报表，guest 构造与执行委托 Process32/Process64。结构、命令兼容及累计性能复核通过，见 [CLI 边界](cli.md) |

目标依赖方向：CLI → Process → Loader/CPU/ABI adapter；ABI adapter → Win32 领域状态 → 纯算法/host 服务。共享 ISA 语义不依赖 Win32 或 host；host 产出事件，由 Process/USER 消费，host callback 不运行 guest CPU。

Win32 契约在装载/lookup 冷阶段解析 DLL/export/alias，产生紧凑领域 ID 和架构 ABI 计划；执行阶段继续静态分发。ABI 已知、路由存在、backend 可用和真实行为已验证分别记录。未迁移条目明确保留 legacy 策略：PE32 未知 ABI 的诊断/零清栈行为与 Win64 拒绝行为不在结构迁移中修改。

CPU 热循环、FLAGS/fault 提交、Memory/cache 所有权、stub 地址和 `.wir v3` 格式保持当前契约。PE32 与 AMD64 内存/CPU 不强制泛型统一；复杂 callback、varargs 和 sret 使用明确的架构适配器。

本轮忽略已撤回的原生 Mono 实验；原始 guest Mono 的普通解释执行和诊断仍保留。

PE32 领域 callback 已直接调用 Process；[回调服务](callbacks32.md)统一原 USER 的 context 恢复策略，DLL/SEH/CRT initializer 继续使用原独立策略。Cpu 的 run/call_guest 兼容 facade 保留给外部使用者，普通 CPU/IR 语义不经过它。领域控制请求仍按家族继续收敛。IR 链引用 `x86::address_space::STUB_BASE` 作为保留地址边界；不能将它误当作可继续链执行的普通指令地址。根目录重导出 facade 的删除条件是公开使用者迁入架构入口；当前不要求 tests/examples 一起换路径。

host/graphics 的调用顺序、所有权与原生验证见 [宿主与图形边界](host-graphics.md)。native 源码和 build.rs 未迁移，保留的第三方构建输入不变。

R7 已将稳定的 PE32 USER owner、请求及文本 codec 迁到 `win32/user/pe32.rs` 与其子目录。根 `user32` 只重导出原公开类型/内部请求，并保留 find_class、host_message 和名称分发兼容函数；CPU/ApiState 借用限于这些兼容函数。原私有 user32_common/user32_geometry 入口删除，几何调用直接使用既有纯核，来源和许可标注保留。没有新增公开模块或要求外部调用方换路径。
