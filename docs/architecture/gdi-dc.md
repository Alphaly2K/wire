# GDI DC 与设备能力边界

`win32/catalog/gdi_dc.rs` 统一七项 export 的 ABI、provider、动态查询和执行策略。PE32 五项 typed，GetWindowDC/GetPixelFormat 保留 ABI-only；Win64 六项 typed，GetWindowDC absent。名字仅在绑定和查询时解析，调用使用 `GdiDcApi`。

PE32 `win32/gdi/dc_x86.rs` 仅借 Args、实际 Graphics owner、窗口尺寸查询与 LastError。`graphics/dc.rs` 提供 compatible DC 创建、删除和 capability 查询；窗口 DC 的 bitmap backing、字体/位图选择、handle 数值与容量、释放顺序仍由 Graphics 管理。[其余 raster/font/bitmap](gdi-operations.md)使用 typed GdiOperation 和只借 Graphics/错误槽的 adapter，没有复制 DC 表。

Win64 `win32/gdi/dc.rs::DeviceContexts` 私有拥有 DC 成员、完整窗口/线程身份、compatible 类型与单调 next/end；`x64/runtime/user/gdi.rs` 由 User owner 协调，只借内存、tid 与当前错误槽，不再借 Runtime64。创建仅借窗口存在性 closure，失败验证不分配 handle；查询允许原线程范围，释放检查原创建线程和类型。销毁窗口时的旧 DC 生命周期保持。owner 提取证据 `wire-out/refactoring-2026-10-14/r3-user-services64-{source-checks,verification}.json`。

| 行为 | PE32 | Win64 |
| --- | --- | --- |
| provider | GetDC/ReleaseDC/GetWindowDC 属 user32，其余属 gdi32 | 六项支持 kernel32/kernelbase/user32 静态；四项 GDI 名称还支持 gdi32 |
| 默认动态查询 | 对应 user32 或 gdi32 | user32；四项 GDI 还支持 gdi32，原已绑定 gateway fallback 保留 |
| backing | 实际 CPU bitmap/font/DC | 逻辑 display DC，未选择 pixel format |
| version / raster capability | 0x300 / 0x2081 | 0x400 / 0 |
| GetPixelFormat | error120，零返回 | 有效 DC 返回零，保留 LastError |
| GetWindowDC | ABI-only/error120 | absent |

两端完整 handle 与 DWORD carrier、返回 frame、错误输出、stock bitmap 和 COLORREF sentinel 由独立基线与既有 GDI/真实窗口测试验证。验收证据见[批次报告](../reports/refactoring-gdi-dc-2026-10-18.md)。
