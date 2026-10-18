# R2/R3 GDI DC 与设备能力（2026-10-18）

状态：2026-10-18 验收通过。独立基线 `41f561c`，实现 `594a38f`，七项契约与 DC owner 已接入，累计 **478 exports**。

七项范围：GetDC、ReleaseDC、GetWindowDC、CreateCompatibleDC、DeleteDC、GetDeviceCaps、GetPixelFormat。PE32 GetWindowDC/GetPixelFormat 保持 ABI-only/error120，其他五项已有处理；Win64 GetWindowDC absent，其他六项已有处理。

审计发现两端原能力存在差异，需要独立冻结：Win64 其余六项支持 kernel32/kernelbase/user32 静态导入，四项 GDI 名称还支持 gdi32；动态仅 user32，以及四项 GDI 的 gdi32。PE32 使用原 user32/gdi32 归属。Win64 DC 保留完整 handle、线程与窗口归属、compatible 类型、单调 next/end 和 4096 上限；PE32 Graphics 同时拥有真实 CPU bitmap/font/DC，不能抽成不拥有 backing 的假句柄表。

GetDeviceCaps 的 PE32 version=0x300、raster=0x2081；Win64 version=0x400、raster=0、额外固定能力值及 60-Hz profile 均保留。错误码、COLORREF 失败 sentinel、stock bitmap 生命周期、无格式的 GetPixelFormat 零返回与正常 LastError 保留，各自通过独立测试固定。

候选提取所需 typed DC 操作与 Win64 DC owner，继续复用原 Graphics backing；其余 bitmap/font/raster API 保留原实现。pixel/backend 边界保持。删除对应重复 ABI/arity/DWORD/provider、动态查询和名称分发。详见[架构说明](../architecture/gdi-dc.md)。

原创可选 `gateway-gdi-profile` 只创建一个 compatible DC，重复 GetDeviceCaps(LOGPIXELSX)，最后 DeleteDC，checksum 消费查询和销毁返回。旧 display 冻结双架构 12K 实跑通过：Win64 retired 6n+18/work 7n+20，PE32 retired 6n+15，API 均为 n+2；PE32 IR/raw 均通过，工具 20 项通过。基线与 fixture 证据 `r2-gdi-dc-baseline{32,64}-tests.txt`、`r2-gdi-dc-fixture-baseline{32,64}`、`r2-gdi-dc-tool-baseline.txt`。编译与前批正式计时/游戏复测严格串行。

before 为 `r2-display-wire/wire`，SHA256 `f1e4bcb455d1c1eab8fd776c84a9269e79f0355ec1b0a12f0c5660d0a7a919dd`，5,957,672 bytes。文件预算 +24KiB，证据根 `wire-out/refactoring-2026-10-14/`。

候选 PE32 GDI/GDI+ 与独立基线合计 14 项、Win64 独立基线三项通过，证据 `r2-gdi-dc-candidate{32,64}-tests.txt`。额外窗口过滤命令误用不存在的名称而选中零项，原 `r2-gdi-dc-window-tests.txt` 保留；随后按真实测试名重跑，不把零项计为通过。

校正后的真实窗口创建/DC/拒绝回滚一项通过（`r2-gdi-dc-window-tests-corrected.txt`）。完整 release **572 passed / 7 ignored**，debug Win32/AMD64/FPU **418 passed**，显式 native **7 passed**，工具 **20 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过，日志 `r2-gdi-dc-{release-tests,debug-tests,native-tests,clippy,final-build,tools,fmt}.txt`。

冻结 `r2-gdi-dc-wire/wire`，SHA256 `b94fa7683a3625b801a703ce34a0abef6b724c3fc010604df35e692c620ebaf9`，**5,960,776 bytes**，较 before **+3,104 bytes**，通过 +24KiB 文件预算。冻结时 **353 个 Rust 文件**（src/tests **345**）与活动源码和快照逐项一致，target/frozen SHA 相同；证据 `r2-gdi-dc-source-verification.json`。后续 desktop 草稿不属于此冻结。

## 性能与启动验收

40 个 workload protocol 使用稳定启动与三对轮换，全部结果、退休数、API 次数通过校验。最大 CPU 中位数回退 GUID/IR **+2.985%**，RSS 中位数最多 **+96KiB**，未触发追加回退调查门槛。Win64 原网关中位数成本最高 **+2.455%**，保留局部成本与离散，不宣称所有路径加速。PE32 integer/IR 中位数 −10.349%，逐对 −0.789%/−10.349%/+6.629% 明显漂移；GUID/IR 逐对 +4.813%/+0.709%/+1.185% 与中位数比较亦有差异，全部保留。

新有效 DC 查询循环 CPU Win64 **+0.026%**，PE32 IR/raw **−4.065%/−3.336%**。三对分别为 Win64 +0.394%/+0.036%/−2.659%，PE32 IR −4.308%/−3.450%/−4.712%，raw −2.838%/−11.851%/−3.151%。完整矩阵 `r2-gdi-dc-stable-{amd64-cpu,v2,amd64-gateways,pe32-cpu,pe32-gateways}`，汇总 `r2-gdi-dc-stable-final-summary.json`。

Alice 200M 三对全部校验，CPU/wall 中位数 **−0.169%/−0.123%**，保持预算出口、隐藏 client 314×211、Draw/Present 0。复用原 runtime，未复制或硬链接；证据 `r2-gdi-dc-stable-alice/compare.json`、manifest 与逐轮日志。编译、正式计时、原生游戏验证严格串行，未推断完整游戏 FPS。
