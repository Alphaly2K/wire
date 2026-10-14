# Runtime 边界

基线：2026-10-14，`2e78377`。维持一个 Cargo crate，先抽职责，再按形成的边界调整目录。

| 当前模块 | 职责和迁移方向 |
| --- | --- |
| `x86::process` | PE32 进程组合根，拥有装载状态、CPU、内存、API、调度器和日志；执行 DLL 通知、API trap、callback 与分时编排 |
| `cpu`、`ir`、`mem` | PE32 ISA/IR/内存；`cpu::process_facade` 仅委托旧的公开 run/call_guest 入口，单步/段执行保持专用静态路径 |
| `x64::{cpu,memory,pe,process}` | AMD64 指令、内存、装载与进程边界，保持现有 typed 热路径 |
| `api` | PE32 stub 与 ABI 调用 facade；领域契约将集中在 `win32::catalog`，领域语义逐步拥有独立状态 |
| `x64::runtime` | Win64 gateway 与服务组合根；已有 BoundImport 冷绑定和 HotImport 热路由，迁移中保留 |
| `fs`、`codepage`、`simd` | 已共享的纯语义或公共服务，继续复用 |
| `runtime::environment` | 不可变 guest 身份/版本/locale/路径；私有可变环境变量模型，架构 allocator 生命周期独立 |
| `graphics`、`d3d9`、`gpu`、`platform` | guest ABI、像素/资源核心、native FFI 与 host UI 尚有交叉，按资源所有权渐进分离 |
| `main` | CLI、IR 文件读取和报表；PE32 状态构造与执行委托 `Process32` |

目标依赖方向：CLI → Process → Loader/CPU/ABI adapter；ABI adapter → Win32 领域状态 → 纯算法/host 服务。共享 ISA 语义不依赖 Win32 或 host；host 产出事件，由 Process/USER 消费，host callback 不运行 guest CPU。

Win32 契约在装载/lookup 冷阶段解析 DLL/export/alias，产生紧凑领域 ID 和架构 ABI 计划；执行阶段继续静态分发。ABI 已知、路由存在、backend 可用和真实行为已验证分别记录。未迁移条目明确保留 legacy 策略：PE32 未知 ABI 的诊断/零清栈行为与 Win64 拒绝行为不在结构迁移中修改。

CPU 热循环、FLAGS/fault 提交、Memory/cache 所有权、stub 地址和 `.wir v3` 格式保持当前契约。PE32 与 AMD64 内存/CPU 不强制泛型统一；复杂 callback、varargs 和 sret 使用明确的架构适配器。

本轮忽略已撤回的原生 Mono 实验；原始 guest Mono 的普通解释执行和诊断仍保留。

PE32 callback 仍允许领域 handler 经旧 Cpu facade 重入 Process。这一兼容环在迁移控制请求时逐家族解除；facade 的删除条件是内部调用者和外部使用者都已迁到 Process。IR 链目前仍引用 `api::STUB_BASE` 作为保留地址边界，后续移到架构地址布局；不能将它误当作可继续链执行的普通指令地址。
