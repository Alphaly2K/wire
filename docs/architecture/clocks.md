# 时钟调用边界

`win32/catalog/clocks.rs` 集中六项时钟 export 的 provider、参数、返回约定与架构执行策略。六项在 PE32 和 Win64 都使用 typed ID；正确性、49 条性能路径及启动回归见 [迁移记录](../reports/refactoring-clocks-2026-10-15.md)。

| export | PE32 | Win64 |
| --- | --- | --- |
| GetTickCount | kernel32；毫秒低 DWORD | kernel32/kernelbase；原 Tick32 热路由 |
| GetTickCount64 | kernel32；EDX:EAX 毫秒值 | kernel32/kernelbase；原 Tick64 热路由 |
| timeGetTime | winmm；毫秒低 DWORD | 静态导入接受 kernel32/kernelbase/winmm；原 Tick32 热路由；动态 lookup 只声明 winmm |
| QueryPerformanceCounter | kernel32；进程 epoch 的 100ns 计数 | kernel32/kernelbase；原 Counter 热路由 |
| QueryPerformanceFrequency | kernel32；输出 10,000,000 | kernel32/kernelbase；原 Frequency 热路由 |
| GetSystemTimeAsFileTime | kernel32；原 wall-time 转换 | kernel32/kernelbase；general 路由 |

冷绑定从 ClockApi 选择原 HotImport，不再维护这六项的重复名称到 hot/arity/provider 表。热路由执行 body 保持原实现；delay import 先完成 provider/handle/IAT 解析，再走 general adapter。adapter 只借用 guest 内存、参数与原进程 Instant，不接触整个运行时或引入可变全局计时服务。

PE32 的 Dword/Qword 结果由调用入口处理。Qword 按原顺序先写 EDX，公共完成路径随后写 EAX 并读取返回 frame；普通 DWORD 调用保持 EDX。checked 输出 fault 不提交返回寄存器、ESP 或 LastError。输出覆盖返回槽时，公共完成路径仍读取被覆盖的值。

Win64 保留完整 64 位输出指针，先读取返回 frame，再调用原热 handler 或 general adapter；输出可以覆盖返回槽，CPU 仍使用预先保存的 frame。坏返回 frame 或 checked output fault 不提交 CPU/API count。原 LastError、trace 和调度提交顺序保持。

GetSystemTimeAsFileTime 的历史架构差异保留：PE32 把 Unix 纳秒先转 u64，再除以 100，pre-epoch 回退零；Win64 先除以 100 再转 u64，pre-epoch 返回原错误。本批不修正这些行为，也不修改 GetSystemTimePreciseAsFileTime、日历转换、sleep 或合作调度。

五项独立边界测试位于 `tests/win32/clocks.rs` 和 `tests/x64/clocks.rs`，先在旧 handler 验证再切换。原创 gateway-clock 用固定 frequency 输出和累计返回值校验退休与 API 次数，避免依赖变化的宿主时间戳。
