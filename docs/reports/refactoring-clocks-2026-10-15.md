# 时钟 API 契约迁移

## 独立基线

基线为 `390665b` 的路径/句柄运行时，复用 `r2-paths-handles-v2-wire/wire`，SHA256 `fe24dec579585793f92866a893b500fbd53447a53297e72236eb40fe50cfd074`。本批六项为 GetTickCount、GetTickCount64、timeGetTime、QueryPerformanceCounter、QueryPerformanceFrequency、GetSystemTimeAsFileTime，不调整 sleep、调度或日历 API。

新增五项独立回归已在旧 handler 上通过：PE32 的 EDX:EAX、DWORD 保留 EDX、10MHz frequency、stdcall 与 LastError；输出只读 fault、覆盖返回槽的提交顺序；Win64 hot route 的完整指针和 frame/output fault；timeGetTime 的静态 provider 与 kernel 动态查询差异；delay import 解析后走 general route，输出覆盖返回槽仍使用提前保存的 frame。首次测试 fixture 漏注册 kernel provider，保留失败日志，补齐原 harness 所需前置条件后五项全部通过，没有修改运行时。

原创 gateway-clock 只调用 QueryPerformanceFrequency，累加 BOOL 返回并读出固定的 10,000,000 输出，两架构均检查退休数和 API count。PE32 raw/IR、Win64 的 12K 预检通过，默认 case 集合保持。源码和日志位于 `tests/{win32,x64}/clocks.rs`、`tools/perf/fixtures/clock_gateway.py` 与 `wire-out/refactoring-2026-10-14/r2-clocks-baseline-*`。
