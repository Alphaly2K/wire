# R2/R3 GDI DC 与设备能力（2026-10-18）

状态：六项独立旧行为基线与有效 DC 查询 fixture 已通过，运行时尚未接入。不得计入已验收累计数。

七项范围：GetDC、ReleaseDC、GetWindowDC、CreateCompatibleDC、DeleteDC、GetDeviceCaps、GetPixelFormat。PE32 GetWindowDC/GetPixelFormat 保持 ABI-only/error120，其他五项已有处理；Win64 GetWindowDC absent，其他六项已有处理。

审计发现两端原能力存在差异，需要独立冻结：Win64 其余六项支持 kernel32/kernelbase/user32 静态导入，四项 GDI 名称还支持 gdi32；动态仅 user32，以及四项 GDI 的 gdi32。PE32 使用原 user32/gdi32 归属。Win64 DC 保留完整 handle、线程与窗口归属、compatible 类型、单调 next/end 和 4096 上限；PE32 Graphics 同时拥有真实 CPU bitmap/font/DC，不能抽成不拥有 backing 的假句柄表。

GetDeviceCaps 的 PE32 version=0x300、raster=0x2081；Win64 version=0x400、raster=0、额外固定能力值及 60-Hz profile 均保留。错误码、COLORREF 失败 sentinel、stock bitmap 生命周期、无格式的 GetPixelFormat 零返回与正常 LastError 保留，各自通过独立测试固定。

计划仅提取所需 typed DC 操作与 Win64 DC owner，继续复用原 Graphics backing；其余 bitmap/font/raster API 保留原实现。避免在这批同时重排 pixel/backend 边界。

原创可选 `gateway-gdi-profile` 只创建一个 compatible DC，重复 GetDeviceCaps(LOGPIXELSX)，最后 DeleteDC，checksum 消费查询和销毁返回。旧 display 冻结双架构 12K 实跑通过：Win64 retired 6n+18/work 7n+20，PE32 retired 6n+15，API 均为 n+2；PE32 IR/raw 均通过，工具 20 项通过。基线与 fixture 证据 `r2-gdi-dc-baseline{32,64}-tests.txt`、`r2-gdi-dc-fixture-baseline{32,64}`、`r2-gdi-dc-tool-baseline.txt`。编译与前批正式计时/游戏复测严格串行。

before 为 `r2-display-wire/wire`，SHA256 `f1e4bcb455d1c1eab8fd776c84a9269e79f0355ec1b0a12f0c5660d0a7a919dd`，5,957,672 bytes。文件预算 +24KiB，证据根 `wire-out/refactoring-2026-10-14/`。
