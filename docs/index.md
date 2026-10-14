# Wire 文档入口

## 当前实现

- [当前状态与验证矩阵](status.md)：注明代码基线、运行入口与外部测试条件。
- [架构和依赖方向](architecture/runtime.md)：当前边界、保留 facade 与迁移目标。
- [宿主与图形边界](architecture/host-graphics.md)：owned 请求、事件消费顺序、GDI 适配及 native 验证入口。
- [新增 API 与 GUID 契约](architecture/adding-api.md)：唯一声明、冷绑定、双架构错误和提交顺序。
- [Windows version 契约](architecture/version.md)：架构 provider 范围、ABI-only 策略与故障提交。
- [本次渐进重构记录](reports/refactoring-2026-10-14.md)：阶段交付、行为差异与验收结果。
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
