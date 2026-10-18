# R2 虚拟桌面与显示查询（2026-10-18）

状态：2026-10-18 验收通过。独立基线 `c415d78`，实现 `0acb937`，16 项契约已接入，累计 **471 exports**。

16 项范围：EnumDisplayDevicesA/W、EnumDisplaySettingsA/W、EnumDisplaySettingsExA/W、EnumDisplayMonitors、MonitorFromPoint/Rect/Window、GetMonitorInfoA/W、GetDisplayConfigBufferSizes、QueryDisplayConfig、DisplayConfigGetDeviceInfo、GetSystemMetrics。PE32 前十二项及 metrics 有原处理，其余 CCD 三项 ABI-only；Win64 十六项均有原处理。

首轮独立基线 PE32 三项通过、Win64 两项通过一项失败：审计遗漏根分发中的 GetSystemMetrics，错误预期其 absent。运行时保持原样，修正测试与拟议契约；原日志 `r2-display-baseline{32,64}-tests.txt` 保留。校正轮又发现测试 harness 的 dispatch 不会登记 gateway，改为显式 register 后，最终双架构各三项通过，日志 `r2-display-baseline{32,64}-tests-final.txt`。Win64 metrics 静态支持三个 provider，但默认动态导出表缺失；已有 static gateway 的动态发现 fallback 仍保留，独立基线固定这一差异。

重点保持：PE32 monitor lookup 宽松返回与 Win64 精确 flags/intersection/HWND 校验的差异；POINT 的 PE32 12-byte 栈与 Win64 一个 packed carrier；PE32 adapter-only display enumeration 与 Win64 adapter/monitor 两级；PE32 display copy fault 转 error 87、Win64 fault 不提交 CPU/API；CCD 的 PE32 error120/零返回与 Win64 direct status50/保留 LastError。callback 继续由进程执行，保留 clipping、用户返回值、lParam 宽度、临时资源和返回帧顺序。

元数据汇入 DisplayApi，共享已证明相同的 device/mode/monitor 编码与固定 desktop profile；架构适配保留输入验证、错误和 trace。EnumDisplayMonitors 使用明确 control 路由，既有真实 guest callback 测试继续执行。原 PE32 display facade 和 legacy 名称分发已移除，原显示能力保持。

原创可选 `gateway-display` 以 MonitorFromPoint(origin,0) 循环消费 monitor handle，资源不随轮次增加。旧 images 冻结双架构 12K 实跑通过：Win64 retired 6n+10/work 7n+10，PE32 retired 7n+7，API 均为 n；PE32 IR/raw 均通过，工具 20 项通过。证据 `r2-display-fixture-baseline{32,64}`、`r2-display-tool-baseline.txt`。

before 为 `r2-images-wire/wire`，SHA256 `eafa6755485153773615025776c0bc5ab67361969f79418a86cf1ebcda1ee225`，5,949,256 bytes。文件预算 +24KiB；证据根 `wire-out/refactoring-2026-10-14/`。

## 实现候选

DisplayApi 统一冷绑定声明，纯 desktop profile 与 device/mode/monitor 编码汇入领域；PE32 标量 adapter 仅借 Args、Memory、LastError，monitor callback 保持具名 process facade；Win64 scalar typed dispatch 与 monitor control 分离。删除原 display facade、重复 ABI/arity/DWORD/provider 和运行时名称分发，旧 legacy bit 位置保留。详见[架构说明](../architecture/display.md)。

六项独立回归与两项既有真实 callback 测试通过，证据 `r2-display-candidate{32,64}-tests.txt`、`r2-display-callback{32,64}-tests.txt`。首轮保留一个迁移后 unused import warning，完整检查前已删除。

完整 release **566 passed / 7 ignored**，debug Win32/AMD64/FPU **412 passed**，显式 native **7 passed**，工具 **20 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。证据 `r2-display-{release-tests,debug-tests,native-tests,clippy,final-build,tools,fmt}.txt`。

冻结 `r2-display-wire/wire`，SHA256 `f1e4bcb455d1c1eab8fd776c84a9269e79f0355ec1b0a12f0c5660d0a7a919dd`，**5,957,672 bytes**，较 before **+8,416 bytes**，通过 +24KiB 文件预算。冻结时 **347 个 Rust 文件**（src/tests **339**）逐项与活动源码和快照一致，target/frozen SHA 相同；证据 `r2-display-source-verification.json`。后续 GDI 草稿不属于此冻结。

## 性能与启动验收

37 个 workload protocol 使用稳定启动与三对轮换，结果、退休数和 API 次数全部校验。最大 CPU 中位数回退为 SSE/IR **+1.567%**，RSS 中位数最多 **+160KiB**，未触发追加回退调查门槛。AMD64 CPU 七条范围 −2.640%～+0.849%，PE32 CPU 八条 −9.725%～+1.567%。PE32 integer/IR 的中位数 −9.725% 同时伴随逐对 +1.318%/−9.725%/+5.905% 的明显漂移，保留原始离散，不据此宣称通用整数收益。

新 monitor 查询网关 CPU Win64 **−0.896%**，PE32 IR/raw **−20.398%/−18.693%**。三对分别为 Win64 +0.127%/−1.790%/−0.896%，PE32 IR −23.133%/−20.188%/−21.379%，raw −18.742%/−19.520%/−18.474%。完整矩阵 `r2-display-stable-{amd64-cpu,v2,amd64-gateways,pe32-cpu,pe32-gateways}`，汇总 `r2-display-stable-final-summary.json`。

Alice 200M 三对全部校验，CPU/wall 中位数 **−1.384%/−1.359%**，维持预算出口、隐藏 client 314×211、Draw/Present 0。复用原 runtime，未复制或硬链接游戏；见 `r2-display-stable-alice/compare.json`、manifest 与逐轮日志。编译、正式计时与原生游戏严格串行，未推断完整游戏 FPS。
