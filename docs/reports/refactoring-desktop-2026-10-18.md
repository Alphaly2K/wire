# R2/R3 进程桌面设置与 DWM 查询（2026-10-18）

状态：六项契约及状态迁移已验收。实现提交 `fb73a4c`，独立基线提交 `ac29b45`，累计已验收 484 个 export。

六项范围：SystemParametersInfoA/W、GetDoubleClickTime、GetDialogBaseUnits、DwmGetWindowAttribute、DwmIsCompositionEnabled。PE32 前两项 ABI-only、两个固定值 typed、DWM 保留 unknown ABI/零清栈；Win64 两项设置与两项 DWM 已有处理，两个固定值 absent。

Win64 SystemParametersInfo 静态支持 kernel32/kernelbase/user32，默认动态表缺失，保留已注册 gateway fallback；DWM 静态支持上述三个 provider 加 dwmapi，动态 user32/dwmapi。不依据 Windows 名义 DLL 收窄原虚拟 provider 范围。

screen-reader 标量已移入私有进程桌面设置 owner，固定计时/字体单位成为纯 profile；DWM 保留窗口存在性、HRESULT、NULL/尺寸检查、LastError、checked output 与返回帧顺序。已有真实窗口 DWM frame bounds 回归继续保留。删除重复 ABI/arity/DWORD/provider、动态查询和名称分发，详见[架构说明](../architecture/desktop.md)。

原创可选 `gateway-desktop`：Win64 重复读取 screen-reader 设置并消费 BOOL 与实际输出，每轮预写 sentinel；PE32 重复 GetDoubleClickTime 并累加。资源恒定。旧 GDI DC 冻结双架构 12K 实跑通过：Win64 retired 11n+10/work 12n+10，PE32 retired 4n+7，API 均为 n；PE32 IR/raw 均通过，工具 20 项通过。证据 `r2-desktop-baseline{32,64}-tests.txt`、`r2-desktop-fixture-baseline{32,64}`、`r2-desktop-tool-baseline.txt`。

before 为 `r2-gdi-dc-wire/wire`，SHA256 `b94fa7683a3625b801a703ce34a0abef6b724c3fc010604df35e692c620ebaf9`，5,960,776 bytes。文件预算 +24KiB，证据根 `wire-out/refactoring-2026-10-14/`。前批计时、游戏与本批编译严格串行。

候选五项独立基线与一项真实窗口 position/DWM frame-bounds 回归通过，证据 `r2-desktop-candidate{32,64}-tests.txt`、`r2-desktop-window-tests.txt`。

候选完整检查：release 577 passed / 7 ignored，debug Win32/AMD64/FPU 423 passed，显式 native 7 passed，工具 20 passed；fmt、strict all-target/all-feature Clippy、release build 均通过。默认 ignored 的七项已在提供原生依赖后显式执行。

冻结 `r2-desktop-wire/wire`：SHA256 `97689a99eb05cbf37372cbce693d3af16e9caa64fdb5ae317753cb3f035d7383`，5,956,232 bytes，相对 before −4,544 bytes，满足 +24KiB 预算。359 个 Rust 文件（src/tests 351 个）的 live/frozen/manifest SHA 及文件集合一致，target 与 frozen executable SHA 一致，见 `r2-desktop-source-verification.json`。正式计时使用这份冻结程序，后续文档更新不改变运行时。

正式三对矩阵共 43 条 workload protocol，结果、retired/work、API 计数及输入全部校验，见 `r2-desktop-stable-final-summary.json` 与五个 `r2-desktop-stable-*` 目录。桌面查询 Win64 CPU −1.088%，PE32 IR/raw −20.724%/−21.863%；PE32 原名称分发已被固定 profile 的 typed 路由替换。RSS 中位数最多 +176KiB。

初筛唯一超过 3% 门槛的是 PE32 bit-test/IR：CPU 中位数 +4.269%，逐对 +1.499%/+4.592%/+0.167%，存在跨轮漂移。保留原轮次，另外进行同 work/launch/QoS 的三对复测，加入同一 before 二进制的 control。补证 CPU 中位数 +0.240%，逐对 +0.240%/+1.920%/−0.692%；control 相对 before −1.081%，after 相对 control +1.336%。未出现稳定的超门槛回退，原报告不改写为补证结果，见 `r2-desktop-stable-bit-test-control-summary.json`。其余原始矩阵最大中位数回退为 AMD64 memory +2.540%。integer/IR 中位数 −9.938%，逐对 +0.261%/−9.938%/+7.374% 漂移明显，不据此宣称通用整数性能收益。

Alice 使用既有 runtime/原游戏资产，严格在矩阵与补证之后串行三对运行。均到 200M work 预算出口，窗口隐藏、client 314×211、Draw/Present 0；CPU/wall 中位数 +0.241%/+0.303%，见 `r2-desktop-stable-alice/compare.json`。此处验收启动前缀与同终点，不涉及实际游戏绘制或 FPS。
