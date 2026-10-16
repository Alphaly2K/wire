# 多媒体计时器契约与状态迁移

## 独立旧行为基线

运行时基线 `04a5921`，before 为 `r2-calendar-wire/wire`，SHA256 `e4d015565582e01f496115997a1516ee1a11919dd043d2e595e99318ab526d70`。范围 timeGetDevCaps、timeBeginPeriod、timeEndPeriod；PE32 仅 winmm，Win64 静态 kernel32/kernelbase/winmm，动态查找仅 winmm。timeGetTime 保持已验收 ClockApi。

两项独立边界回归和三项相关回归共五项通过，固定 PE32 stateless period、短 buffer/写入故障返回 97、LastError 与 public facade 精确范围；Win64 256 种 period 上限、重复请求/释放恢复容量、DWORD 截断、保存返回帧、checked fault 与静态/动态 provider 差异。日志 `r2-multimedia-baseline-tests.txt`。

原创可选 `gateway-multimedia` 调用 timeGetDevCaps，累加 `(max >> 16) + min`，即 65536n。n=max(1,work//11)，Win64 retired=10n+10、work=11n+10，PE32 retired=11n+9；API calls=n。旧实现的双架构 12K smoke（PE32 raw/IR）与 20 项工具检查通过，默认 case 集合保持。日志前缀 `r2-multimedia-baseline-`。运行时代码尚未接入。
