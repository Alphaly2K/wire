# 图形创建与 COM 契约

七项公开创建入口由 `win32/catalog/graphics.rs` 统一声明：CreateDXGIFactory/1/2、D3D11CreateDevice、D3D11CreateDeviceAndSwapChain、D3D12CreateDevice、D3D12GetDebugInterface。PE32 全部保持 Absent/未知 ABI；Win64 保留原 typed 子集与各调用的实际失败结果，D3D12 仍无 backend，D3D11 combined 创建仍显式拒绝。

旧静态导入接受各自的图形 DLL、kernel32/kernelbase 和内部 `wire.dxgi`，其中内部 provider 区分大小写；契约用 exact-provider 元数据保留这项身份规则。fresh GetProcAddress 仅在各自图形 DLL 中发现入口，已经安装的同 provider gateway 继续可发现。D3D12 序数 #101/#102 的归一也移入契约模块。公开 DLL 的大小写规则保持。

gateway 在冷绑定阶段取得 GraphicsExportApi、arity 和 DWORD 位图，执行时直接选择创建 handler。原 DXGI/D3D11 创建处理体与 COM 方法分发分开，输出 preflight、对象父子引用、错误顺序、原生 backend 调用和 CPU/trace 提交保持。本批没有更改 shader、渲染、present 或 vtable 布局。

旧行为基线 `01b714f`。release 图形单元 10 passed / 2 ignored、PE32 未知 ABI 1、模块 12、catalog 1，fmt/release lib/bin strict Clippy/build 通过。另显式运行真实 GPU swapchain/backbuffer/resize/lifetime/deferred-present 1 项通过。创建、方法处理体和 ordinal 归一核对保持。证据见 `wire-out/refactoring-2026-10-14/r2-graphics-{source-checks,verification}.json` 及 validation 日志。普通契约迁移不执行性能矩阵。

随后以 `7435932` 为基线，将 common/factory/adapter/output/swapchain 的 70 条方法声明合为 `(name, arity, DWORD indices)`，vtable 与参数绑定读取同一条声明。D3D11 原有三元组表保持，原两份查找合为一个入口。所有方法的参数数量、截断索引与槽位顺序经旧源码逐项核对一致。

BoundMethod 在冷阶段保存静态方法声明与 DXGI/D3D11 家族路由，gateway 直接进入相应 handler，每次调用不再扫描 D3D11 方法表。handler 内仍保留原名称分支与对象类型检查；这一步没有将全部 COM 方法改为独立 enum。旧 groups 位图同时收敛为 graphics binding 与 file_search 标志，文件枚举行为保持。

该批 release 图形 10、真实 GPU swapchain 1、Win64 文件 17 项通过，fmt/release lib/bin strict Clippy/build 通过。检查中发现并收窄了 BoundImport.graphics 的可见性；最终静态检查和构建通过。源码核对与验证记录为 `r2-graphics-methods-{source-checks,verification}.json`，不重复性能矩阵或 PE32 游戏。
