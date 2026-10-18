# R2 虚拟桌面与显示查询（2026-10-18）

状态：独立旧行为基线与 fixture 已通过，尚未接入实现。不计入已验收累计数。

16 项范围：EnumDisplayDevicesA/W、EnumDisplaySettingsA/W、EnumDisplaySettingsExA/W、EnumDisplayMonitors、MonitorFromPoint/Rect/Window、GetMonitorInfoA/W、GetDisplayConfigBufferSizes、QueryDisplayConfig、DisplayConfigGetDeviceInfo、GetSystemMetrics。PE32 前十二项及 metrics 有原处理，其余 CCD 三项 ABI-only；Win64 十六项均有原处理。

首轮独立基线 PE32 三项通过、Win64 两项通过一项失败：审计遗漏根分发中的 GetSystemMetrics，错误预期其 absent。运行时保持原样，修正测试与拟议契约；原日志 `r2-display-baseline{32,64}-tests.txt` 保留。校正轮又发现测试 harness 的 dispatch 不会登记 gateway，改为显式 register 后，最终双架构各三项通过，日志 `r2-display-baseline{32,64}-tests-final.txt`。Win64 metrics 静态支持三个 provider，但默认动态导出表缺失；已有 static gateway 的动态发现 fallback 仍保留，独立基线固定这一差异。

重点保持：PE32 monitor lookup 宽松返回与 Win64 精确 flags/intersection/HWND 校验的差异；POINT 的 PE32 12-byte 栈与 Win64 一个 packed carrier；PE32 adapter-only display enumeration 与 Win64 adapter/monitor 两级；PE32 display copy fault 转 error 87、Win64 fault 不提交 CPU/API；CCD 的 PE32 error120/零返回与 Win64 direct status50/保留 LastError。callback 继续由进程执行，保留 clipping、用户返回值、lParam 宽度、临时资源和返回帧顺序。

拟把元数据汇入 DisplayApi，共享已证明相同的 device/mode/monitor 编码与固定 desktop profile；架构适配保留输入验证、错误和 trace。EnumDisplayMonitors 使用明确 control 路由，既有真实 guest callback 测试继续执行。原 PE32 display facade 和 legacy 名称分发在切换时移除，不提前扩展显示能力。

原创可选 `gateway-display` 以 MonitorFromPoint(origin,0) 循环消费 monitor handle，资源不随轮次增加。旧 images 冻结双架构 12K 实跑通过：Win64 retired 6n+10/work 7n+10，PE32 retired 7n+7，API 均为 n；PE32 IR/raw 均通过，工具 20 项通过。证据 `r2-display-fixture-baseline{32,64}`、`r2-display-tool-baseline.txt`。

before 为 `r2-images-wire/wire`，SHA256 `eafa6755485153773615025776c0bc5ab67361969f79418a86cf1ebcda1ee225`，5,949,256 bytes。文件预算 +24KiB；证据根 `wire-out/refactoring-2026-10-14/`。
