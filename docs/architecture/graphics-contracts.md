# 图形创建与 COM 契约

七项公开创建入口由 `win32/catalog/graphics.rs` 统一声明：CreateDXGIFactory/1/2、D3D11CreateDevice、D3D11CreateDeviceAndSwapChain、D3D12CreateDevice、D3D12GetDebugInterface。PE32 全部保持 Absent/未知 ABI；Win64 保留原 typed 子集与各调用的实际失败结果，D3D12 仍无 backend，D3D11 combined 创建仍显式拒绝。

旧静态导入接受各自的图形 DLL、kernel32/kernelbase 和内部 `wire.dxgi`，其中内部 provider 区分大小写；契约用 exact-provider 元数据保留这项身份规则。fresh GetProcAddress 仅在各自图形 DLL 中发现入口，已经安装的同 provider gateway 继续可发现。D3D12 序数 #101/#102 的归一也移入契约模块。公开 DLL 的大小写规则保持。

gateway 在冷绑定阶段取得 GraphicsExportApi、arity 和 DWORD 位图，执行时直接选择创建 handler。原 DXGI/D3D11 创建处理体与 COM 方法分发分开，输出 preflight、对象父子引用、错误顺序、原生 backend 调用和 CPU/trace 提交保持。本批没有更改 shader、渲染、present 或 vtable 布局。

旧行为基线 `01b714f`。release 图形单元 10 passed / 2 ignored、PE32 未知 ABI 1、模块 12、catalog 1，fmt/release lib/bin strict Clippy/build 通过。另显式运行真实 GPU swapchain/backbuffer/resize/lifetime/deferred-present 1 项通过。创建、方法处理体和 ordinal 归一核对保持。证据见 `wire-out/refactoring-2026-10-14/r2-graphics-{source-checks,verification}.json` 及 validation 日志。普通契约迁移不执行性能矩阵。

随后以 `7435932` 为基线，将 common/factory/adapter/output/swapchain 的 70 条方法声明合为 `(name, arity, DWORD indices)`，vtable 与参数绑定读取同一条声明。D3D11 原有三元组表保持，原两份查找合为一个入口。所有方法的参数数量、截断索引与槽位顺序经旧源码逐项核对一致。

BoundMethod 在冷阶段保存静态方法声明与 DXGI/D3D11 家族路由，gateway 直接进入相应 handler，每次调用不再扫描 D3D11 方法表。handler 内仍保留原名称分支与对象类型检查；这一步没有将全部 COM 方法改为独立 enum。旧 groups 位图同时收敛为 graphics binding 与 file_search 标志，文件枚举行为保持。

该批 release 图形 10、真实 GPU swapchain 1、Win64 文件 17 项通过，fmt/release lib/bin strict Clippy/build 通过。检查中发现并收窄了 BoundImport.graphics 的可见性；最终静态检查和构建通过。源码核对与验证记录为 `r2-graphics-methods-{source-checks,verification}.json`，不重复性能矩阵或 PE32 游戏。

Win64 COM 的 vtable、对象分配、adapter 缓存及递归释放现由 Graphics 的 `dxgi/objects.rs` 拥有。分配明确借 Memory、Gateways 和共享页游标；释放只借 Memory，按原顺序处理 swapchain、映射 buffer/texture、view、context 绑定与 parent。表的 gateway 注册、map/write/seal、游标推进和引用提交顺序保持，公开 attach_graphics 继续委托。其它 D3D11/DXGI handler 暂由 Runtime64 组合资源。

该 owner 提取经五个生命周期主体及全部调用点源码对照，release Win64 269、library 61 passed / 7 ignored、严格 Clippy/build/fmt 通过；另显式通过原生 procedural draw 和 swapchain/backbuffer/resize/lifetime 两项 GPU 测试。证据 `r3-graphics-objects64-{source-checks,verification}.json`。

虚拟 output 的缓存/模式查询、D3D11 context 建立、单项 binding 引用替换以及 native draw 输入准备也直接归 Graphics。output/context 分配借 gateway 和游标，其余操作仅用 owner 状态与必要内存；public graphics_draws 保留原 facade。所有修改文件按明确资源替换对照一致，draw 的 shader/资源检查和 GPU 提交算法保持。完整 Win64 269、library 61 passed / 7 ignored、两个显式 GPU fixture 及严格构建通过，见 `r3-graphics-services64-{source-checks,verification}.json`。

D3D11 创建、COM 方法与 input-layout 解析通过临时 `d3d11::Calls` 借用 Graphics、Gateways、共享页游标和 Trace；内存仍是每次调用的明确参数。它不拥有第二份状态，不借 Runtime64、线程、USER 或 CPU。Runtime64 的创建/调用方法只组合这四项资源。递归 11.1→11.0 路由在同一借用内完成；Map 的游标推进和输出故障顺序保持。

DXGI 和 D3D11 的 private-data 字节/容量处理共用 Graphics 的原实现；对象/provider 校验仍由各入口按原顺序执行。input-layout 的名称读取使用 `win32/x64/text.rs` 中原样提取的 ASCII codec，Runtime64 的旧静态方法继续委托。

DXGI factory/D3D12 能力创建及普通 factory/adapter/COM 操作位于 Graphics 的 `dxgi/calls.rs`，明确借内存、gateway、共享游标及必要的只读 User。原对象类型检查、D3D11 QueryInterface 转发和 swapchain 控制路由仍由根协调器先执行；parent 使用检查时取得的值。窗口关联只查询 User 的现有窗口成员关系。vtable、provider、对象查询与错误输出顺序保持。

swapchain 创建/查询/resize 使用临时 Calls，只借 Graphics、gateway、共享游标、Trace 和只读 User。Graphics 拥有 backbuffer 创建、present 状态检查、帧资源快照和成功计数；窗口快照仍在原验证位置读取，resize 仍先建立新 buffer，再释放旧 buffer，最后替换记录。GetDevice 在构造 D3D11 资源借用前复制原三个参数。

Runtime64 的 present 协调器保留公共 frame/统计入口，成功时仍先更新图形计数，再 API count → unblock → record；Process/Presenter 的 GPU surface 提交和 guest 返回帧保持。连续图形迁移整合通过 release 全目标 694 passed / 7 ignored、21 suites，严格 Clippy/build/fmt 与两个显式 GPU fixture；证据 `r3-swapchain-calls64-{source-checks,verification}.json`。
