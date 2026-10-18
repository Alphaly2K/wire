# GDI 对象、像素与逻辑字体

`win32/catalog/gdi_operations.rs` 声明 31 个现有 export 的唯一签名和架构策略：PE32 由 gdi32 提供 typed 调用，Win64 保持 absent。手写清栈表不再重复这些项目。14 参数的 CreateFont 与 13 参数的 StretchDIBits 仍由 PE32 完整读取；DWORD、颜色和有符号坐标的 carrier 保持。

纯图形核心拥有 `graphics::GdiOperation`；catalog 将它重导出为 GdiApi，并把 export 名称绑定到操作。`graphics` 不反向依赖 catalog 或进程。主网关消费绑定 ID，`win32/gdi/x86.rs::call` 只借 Args、GuestMem、Graphics 和 LastError；对象、字体、DC、位图与 DIB backing 继续由同一 Graphics 拥有。

原对象、DIB 和字体处理体改为匹配枚举，没有第二张逐调用名称表。font 的优先处理保持：字体选择/删除/查询和 stock 字体先由字体分支处理，剩余对象再按位图/DC 分支处理。这个顺序也保留了 stock 字体与 stock bitmap 不同的删除结果。ANSI/Wide LOGFONT、CP932、字体 metrics 缓存与宿主调用都保持。

适配器保留 COLORREF 的 `u32::MAX` 与 BOOL/handle 的零错误值差异；error120 标记 unimplemented，其它既有错误继续标记 implemented。DIB 失败仍按原 handle/flags/input 顺序判定，并在原位置读取可访问 BITMAPINFO 生成诊断。该读取与调用 trace 没有移入像素核心。

输出和返回帧顺序按 PE32 保持：GetObject 的 BITMAP 输出可先覆盖返回地址，网关随后读取覆盖后的返回地址；跨不可写页的输出保持既有预检和原字节。origin、颜色、blit、GetDIBits 与 font 参数的部分提交和资源释放均使用原实现。

独立行为基线与验收进度见 [GDI 迁移记录](../reports/refactoring-gdi-operations-2026-10-19.md)。DC 创建/释放及两架构设备能力继续使用[原 DC 契约](gdi-dc.md)，没有新建第二套对象表。
