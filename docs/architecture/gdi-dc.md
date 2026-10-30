# GDI DC 与设备能力

新 Wasm 入口在两种 ABI 接入 GetDC、ReleaseDC、CreateCompatibleDC、DeleteDC、GetDeviceCaps 和 GetPixelFormat。前两项属于 user32.dll，其余属于 gdi32.dll；新运行时保留目录已有的 provider 资格。GetWindowDC 未绑定。

DC 身份与成员存放在共享 DeviceContexts 表中，新入口的 raster 状态直接附在该记录上，不维护第二张 DC 成员表。记录保存完整窗口/线程身份、compatible 类型、选择的位图/brush/font、颜色、原点、viewport 和 clip。句柄单调分配，不因释放立即复用；窗口、内存和 paint DC 的释放方式及 owner 必须匹配，失败返回零和错误 6。

GetDC 为窗口 client 创建或复用实际 32 位像素 backing，ReleaseDC 只释放 DC，像素保留到窗口销毁。GetDC(NULL) 对应独立的 1280×720 虚拟屏幕。兼容 DC 初始选择 1×1 monochrome stock bitmap，可选入自己的兼容位图。线程退出释放其 DC，独立位图和 DIB backing 属于进程；窗口销毁释放窗口 backing 和相关 DC。paint DC 的特殊寿命见[绘制契约](painting.md)。

每 VM 的 DC、位图和逻辑字体合计最多 4096 项，包含 stock bitmap 和 PE32 的七种 stock font；像素 backing 总计最多 256 MiB。单个位图每轴为 1..16384，最多 16 Mi 像素。资源或身份容量不足返回错误 8，非法尺寸返回 87；分配失败不消耗新句柄。窗口 backing 随 client resize 保留重叠内容，失败保留原 backing 和窗口几何。

GetDeviceCaps 发布固定 1280×720、32 bits/pixel、1 plane、96 DPI、60 Hz 的 raster profile；RASTERCAPS 发布 RC_BITBLT 和 RC_STRETCHDIB。GetPixelFormat 对有效 DC 返回零，当前没有已选择的像素格式；ChoosePixelFormat、SetPixelFormat、DescribePixelFormat 和 SwapBuffers 提供[无后端失败协议](graphics-contracts.md)。该 profile 不查询宿主显示设备。

对象、像素与位图服务见 [GDI operations](gdi-operations.md)。旧运行时的适配器仍留作迁移依据，默认产品入口切换记录在根 GOAL。
