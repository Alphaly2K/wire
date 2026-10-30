# 地址等待与线程状态边界

新 Wasm 入口的三项 WaitOnAddress/WakeByAddressSingle/WakeByAddressAll 服务由 `crates/wire-wasm/src/win32/address_waits.rs` 实现。两种 ABI 均接受 kernel32、kernelbase 和 api-ms-win-core-synch-l1-2-0；SIZE_T 保持对应指针宽度，timeout 截断为 DWORD。该 API-set 的 Sleep/SleepEx 复用现有延迟服务。共享目录中的旧 PE32 absent 策略保留在旧入口，新运行时独立接入。

首次等待先验证 SIZE_T 为 1/2/4/8，再依次读取当前值和比较值；值不同立即成功，保持 LastError。值相同且 timeout 为零返回 FALSE/1460；无效 SIZE_T 返回 FALSE/87，且不读取比较地址。等待按加载/回调层级保存完整参数、返回位置和首次 deadline；重试只观察通知与 deadline，不重新读取比较内存。

WakeSingle 选择地址匹配、未通知且未过期的最早登记者；WakeAll 标记全部有效匹配者。暂时挂起的线程及外层回调的等待仍可收到通知。通知先于超时时，后续延迟调度仍返回成功；已经到期的等待不消耗后来通知。Wake 不读取客体地址，无等待者时不积累信号。

有限正超时使用统一单调 10 MHz 来源；无限等待、零超时和没有有限候选的 wake 不采样时间。通知或确认到期后停止采样。故障不消耗 work，也不推进等待序号或清掉原调用；错误输出故障可修复后重试。完成计一 work，不计客体退休；无可运行线程时返回 Waiting，宿主恢复后继续调度。

`src/win32/kernel/address_wait.rs` 仅包含便携等待身份、通知/deadline 判断和 FIFO 选择，不依赖任何 CPU 或内存类型。旧入口的内存适配保留在其线程模块，新入口使用同一 VMM 与待完成服务生命周期。
