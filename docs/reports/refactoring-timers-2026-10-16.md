# Waitable timer 契约迁移

## 独立旧行为基线

九项 export 为 CreateWaitableTimerA/W、CreateWaitableTimerExA/W、OpenWaitableTimerA/W、SetWaitableTimer/Ex、CancelWaitableTimer。运行时基线 `d5d484f`，before 为 `r2-delays-wire/wire`，SHA256 `6b0f99d96603c04488a2f751274ce04d7b54a48cd55abc2d0fef92088b6ff621`。PE32 九项均为 Wine ABI 已知但未实现；Win64 继续支持 kernel32/kernelbase 的原 timer 子集。

两项新增基线在旧 handler 上通过：PE32 固定全部九项 error 120、独立硬编码清栈字节及输出不变；Win64 覆盖 A/W 命名与 create/open/extended alias access、DWORD 截断与完整指针、权限和既有 LastError，以及 due-time 访存故障先于 invalid handle/period/APC 检查且不提交 CPU/API count。既有 manual/auto、consume/cancel、deadline、重复命名对象 wait-all 与权限回归保留。日志 `r2-timers-baseline-tests.txt`。

原创可选 `gateway-timer` 只循环取消未 arm 的 timer，累加 BOOL 返回 n，不依赖到期时间。独立预期 n=work//6、retired=5n+17、work=6n+18、API calls=n+1；独立 PE inventory 核对恰好两项导入和原 IAT 地址。before 的 Win64 12K smoke 通过，20 项工具检查通过；默认 case 集合保持。

运行时迁移尚未接入或验收。
