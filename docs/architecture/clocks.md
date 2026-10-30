# 时钟与平台采样

`win32/catalog/clocks.rs` 集中六项时钟 export 的 provider、参数及返回约定。新 Wasm 入口按这些契约捕获调用帧，PE32 的 DWORD/QWORD 返回和 Win64 的完整指针宽度由统一完成路径处理。

| export | 时间来源与结果 |
| --- | --- |
| GetTickCount、timeGetTime | 单调经过 tick 除以 10000，保留毫秒低 DWORD |
| GetTickCount64 | 单调经过 tick 除以 10000；PE32 使用 EDX:EAX |
| QueryPerformanceCounter | 输出完整的单调 100 ns tick，返回 TRUE |
| QueryPerformanceFrequency | 输出 10000000，不读取时间源 |
| GetSystemTimeAsFileTime | 输出独立的绝对 FILETIME |

RDTSC/RDTSCP、有限等待及相对定时器使用同一单调 10 MHz 来源。Web 入口默认从 performance.now() 转换，也可注入返回 unsigned 64-bit BigInt 的 clock()；回退的经过时间样本被拒绝。wallClock() 返回 FILETIME，默认将 Date.now() 转换到 Windows 纪元，允许墙钟调整。

[日历服务](calendar.md)中的 GetSystemTime、GetLocalTime 和 GetSystemTimePreciseAsFileTime 使用相同的 wallClock()。前两项转换为 SYSTEMTIME，精确 FILETIME 查询保留提供者的低位精度；默认时区为 UTC。日历转换、比较和 UTC 时区信息查询无需时钟，GetSystemTimes 单独读取 `cpuTimes()` 的累计 CPU 时间样本；未配置时返回 FALSE/错误 50。

[日期/时间格式](nls.md)的六个 A/W/Ex 接口仅在 SYSTEMTIME 为 NULL 时使用 wallClock() 和同一 UTC 默认时区；显式输入、其它 NLS 调用和读取调用参数失败时均无需采样。

绝对定时器在设置时取得经过时间与 FILETIME 两个新样本，换算后保存为单调 deadline；后续墙钟调整不改变该 deadline。普通事件、信号量、互斥体和无超时线程等待不读取时钟。具体定时器行为见[定时器契约](timers.md)。

普通文件写入和截断按文件对象选择墙钟。SetFileTime 的全 1 修改时间输入抑制该对象的自动更新，DuplicateHandle 共享该状态，受抑制的写入省去采样。GetTempFileName 只在自动创建文件时采样一次作为初始时间；非零 unique 提示、复制和显式时间设置无需时钟，见[文件契约](files.md)。

所有样本在实际服务执行前获取。提供者错误、类型或范围错误保持网关及原资源状态，允许修复后重试。调用帧完整捕获后再写输出，输出可以覆盖原参数或返回槽；完成使用保存的目标。访存故障保留 CPU、work、退休数及 LastError。

底层 wire_service_clock 返回分类：1 无需时间、2 经过时间、3 FILETIME、4 两者、5 CPU 累计时间。分类 4 使用 wire_service_timed(owner, elapsed, wall)，分类 5 使用 wire_service_cpu(owner, idle, kernel, user, error)，其余使用 wire_service(owner, sample)；未绑定返回 0。两种服务入口返回相同状态，平台桥接不增加核心的时钟宿主导入，见[运行时契约](wasm-runtime.md)。
