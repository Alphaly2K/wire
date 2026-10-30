# GDI 对象、像素与位图

新 Wasm 入口在 PE32/Win64 接入下列 26 项 gdi32.dll 服务，共用同一 GDI owner；PE32 另接入五项字体服务：

| 服务 | 当前范围 |
| --- | --- |
| CreateCompatibleBitmap、SelectObject、DeleteObject | 兼容位图与 stock brush；源 DC 选择 DIB 时创建同格式 DIB，否则创建 owned bitmap；位图同一时刻只能选入一个内存 DC，选中时禁止删除 |
| GetObjectA/W、GetObjectType、GetCurrentObject、GetStockObject | BITMAP、LOGBRUSH、DC 类型和已选择对象；stock bitmap 与六种 stock brush |
| CreateDIBSection、GetDIBits、StretchDIBits | 32 位 DIB 的客体 backing、读回和正尺寸 SRCCOPY 缩放，具体格式及限制如下 |
| SetPixel、SetPixelV、GetPixel | COLORREF、monochrome 转换、DC 原点/viewport 和目的 clip |
| BitBlt | 正宽高的 SRCCOPY，零面积成功；目的裁剪后检查源范围，重叠搬运使用源快照 |
| Set/GetWindowOrgEx、Set/GetViewportOrgEx | MM_TEXT 坐标原点；输出旧值成功后才提交新值 |
| Set/GetTextColor、Set/GetBkColor | 普通 RGB COLORREF，保存/查询颜色 |
| Set/GetMapMode、GdiFlush | 仅 MM_TEXT；flush 无待完成宿主命令 |

窗口/兼容位图使用 owned COLORREF；DIB 使用客体 VMM 内的 BGRA8，直接客体写入立即可见。分配、尺寸限制、monochrome 颜色、DIB header/mask/扫描线规则复用便携纯函数。新运行时的位图与窗口 backing 不依赖旧 CPU、GuestMem 或原生 raster。资源额度与 DC 寿命见[设备上下文](gdi-dc.md)。

GetObject 的 BITMAP 输出在 PE32 为 24 字节，Win64 为 32 字节；LOGBRUSH 分别为 12/16 字节。DIBSECTION 分别为 84/104 字节，保留完整 bmBits、带符号的 biHeight 和 BITFIELDS masks。空输出返回所需大小；DIB 输出容量至少为 BITMAP 大小时，可按容量返回 BITMAP 或完整 DIBSECTION。owned bitmap 的 bmBits 为零，DIB 的 bmBits 是其客体地址。输出在捕获原调用帧后统一写入，跨页故障保留原字节及对象状态，输出覆盖返回槽不改变调用者返回位置。

GetPixel、SetPixel 和颜色接口的失败值为 DWORD u32::MAX；BOOL/handle 接口使用零，错误写入当前线程的 LastError。无效句柄操作返回错误 6（GetObjectType 返回零），选中/内部位图的非法删除返回 170，未支持的模式、ROP、palette 颜色或对象类别返回 50。stock brush 的删除返回成功并保留对象。

CreateDIBSection 接受 40 字节 BITMAPINFOHEADER、DIB_RGB_COLORS、32bpp/1 plane、BI_RGB 或标准 RGB masks 的 BI_BITFIELDS。宽度为正、高度可正负；正高度为 bottom-up。当前仅支持匿名 section，文件 section、非零 offset、palette 和其他格式返回 50。完整 bits 指针输出成功后才发布位图和新身份；输出故障保留调用帧与原字节，不消耗资源。

DIB backing 以 MEM_MAPPED、PAGE_READWRITE 建立，属于位图 owner。DeleteObject 在未选中时释放映射，普通 VirtualFree/decommit 协议不能释放它。保护可以在初始 read/write 权限内缩减及恢复，不能增加 execute。线程退出只释放 DC，DIB 位图保留为进程资源；静态启动拒绝会预检并回收全部 DIB 映射。无 DEP 的 PE32 沿整体内存策略允许数据执行，GDI 写入和删除仍撤销对应 JIT 源码 authority。

GetDIBits 的 NULL bits 查询填入标准 32bpp BI_RGB header，保留输入中的分辨率字段；非空读回要求匹配宽高和该格式，位图必须未选入 DC。支持 start/count、top-down 或 bottom-up 输出，超出末尾返回零行。输出前捕获源像素，所以输出与 DIB backing 重叠时仍按原数据读回。

StretchDIBits 支持上述 RGB/BITFIELDS 源与正尺寸的 SRCCOPY，按最近像素缩放并服从目的 DC 的 MM_TEXT 原点/clip。top-down 的源原点在左上，bottom-up 的源原点在左下；裁取部分扫描线也遵循该方向。负尺寸镜像、其他 ROP、压缩/调色板格式尚未接入。源缓冲先保存快照，目的每个受影响扫描线在写入前统一预检；重叠和跨页失败不会留下部分目的像素。GDI 写回的 alpha 为零，直接客体写入的原 alpha 保留，窗口快照对外提供不透明 RGBA8。

PE32 的 CreateFontA/W、CreateFontIndirectA/W 创建真实逻辑字体，新旧入口共用 LOGFONT 字段、CP932 和 UTF-16 编码规则。直接 A 名称沿用有损 CP932 解码，间接 A 使用严格解码；W 保留原始 UTF-16，名称最多 31 单元并以 NUL 结束。GetObjectA/W 分别输出 60/92 字节 LOGFONT，空输出查询大小。GetObjectType 返回 OBJ_FONT，GetCurrentObject 和 SelectObject 与各 DC 保存的字体一致。

七种 stock font 归进程所有，新 DC 默认选择 SYSTEM_FONT；stock 删除成功但保留，选中的动态字体禁止删除并返回 170。动态字体与 DC、位图共用对象额度，线程退出释放 DC 后字体仍属进程。GetTextMetricsW 验证 DC 和所选字体后返回零和错误 120，保持输出；当前缺少字体 realization、metrics 与字形绘制后端。AMD64 五项字体入口保留原 absent 范围。

宿主通过 [windowPixels 与 BrowserPresenter](input-events.md) 取得并显示各进程的 owned RGBA8 快照；D3D9 Present 共用这些像素，DXGI/D3D11 使用独立 WebGPU canvas。
