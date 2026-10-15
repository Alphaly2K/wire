# Sleep 与线程让出契约迁移

## 旧行为基线

三项 export 为 Sleep、SleepEx、SwitchToThread；PE32 只接受 kernel32，Win64 接受 kernel32/kernelbase。运行时基线 `80be38b`，before 为 `r2-waits-wire/wire`，SHA256 `006ec9bb9ffb0c85be19c42d39b9893e660ce31e12fc46d9ae7f9b9f74252bbe`。

两项新增独立测试在旧 handler 上通过：PE32 返回槽 fault 时 EAX 已写、线程尚未让出、frame 和 LastError 保持，以及 SleepEx 忽略 alertable；Win64 DWORD 高位截断、Sleep/SleepEx/SwitchThread 的不同 control、正常返回 frame/计数、返回槽 fault 前 CPU/API count 均未提交和 wrong DLL 拒绝。既有实际线程 zero-delay yield、三种 APC callback 及带预算暂停的 timed sleep/timeout 测试也通过。日志前缀 `r2-delays-baseline-`。

原创可选 `gateway-delay` 循环调用 SleepEx(0,0)，返回累加 0。PE32 独立预期 n=work//6、retired=6n+7、API calls=n；Win64 n=work//7、retired=6n+10、work=7n+10、API calls=n。before 的 12K 双架构与 PE32 raw/IR smoke 全通过，20 项工具检查通过；默认 case 集合不变。

运行时契约尚未接入或验收。
