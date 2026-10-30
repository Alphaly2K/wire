# 可等待定时器

新 Wasm 入口在 PE32 与 Win64 均支持 CreateWaitableTimerA/W、CreateWaitableTimerExA/W、OpenWaitableTimerA/W、SetWaitableTimer、SetWaitableTimerEx 和 CancelWaitableTimer。provider 与参数宽度来自 `win32/catalog/timers.rs`。名称、别名、权限、安全属性与容量遵循[同步对象规则](sync-objects.md)。

普通创建的 BOOL 与 Ex 的 CREATE_WAITABLE_TIMER_MANUAL_RESET 选择 manual-reset 或 auto-reset。Ex 同时接受 CREATE_WAITABLE_TIMER_HIGH_RESOLUTION，但不改变宿主提供时间的精度；其它 flags 返回错误 87。等待需要 SYNCHRONIZE，设置与取消需要 TIMER_MODIFY_STATE。

due time 是有符号 64 位的 100 ns 数。负数表示相对时间，只采样单调经过时间；非负值表示绝对 FILETIME，同时取得一对新的经过时间与墙钟样本，换算后保存为单调 deadline。设置在过去的绝对时间立即到期；deadline 加法溢出或负 period 返回错误 87。period 以毫秒计，零表示一次性；周期定时器跳过错过的周期，不累积信号 credits。

Set/SetEx 拒绝非空 APC completion routine，普通 Set 的 resume 及 SetEx 的 reason context 非零均返回错误 50。SetEx 的 tolerable delay 作为容差接受，当前按原 deadline 调度。时钟提供者错误在调用分派前返回宿主，原定时器状态保持，可重试。

到期只设置信号。manual-reset 保持信号，auto-reset 在成功等待时消耗信号；重新 Set 清除旧信号。Cancel 停止后续到期并保留已有信号，包含本次采样时已到期但尚未观察的状态；没有活动 deadline 时取消不读取时钟。

等待使用与有限超时相同的单调时间源，无可运行线程时返回 Waiting，宿主再次 run 才继续推进，不调用宿主 sleep。绝对设置使用 `wire_service_timed(owner, elapsed, wall)`；普通 `wire_service` 对缺少墙钟样本的绝对设置报接口错误。底层时间分类与桥接见[运行时契约](wasm-runtime.md)。

消息定时器 SetTimer/KillTimer 已接入相同单调时间源，生成 WM_TIMER，由 DispatchMessage 经同一 JIT 调用 TIMERPROC。它们使用 owner/HWND/ID 身份、合并到期消息及 1024 项独立容量；不作为内核对象参与 WaitForSingleObject。查询/消息等待按需推进，取消保留已排队消息，具体契约见[消息队列](messages.md)。
