# 多媒体计时器调用与资源边界

Wasm 入口在两种 ABI 接入 timeGetDevCaps、timeBeginPeriod、timeEndPeriod，仅接受 winmm provider。三项使用 MultimediaApi 冷绑定，timeGetTime 属于 ClockApi。调用不读取时间样本。

能力输出共用 `kernel/multimedia.rs` 的 1..UINT_MAX 记录。短 buffer 或 NULL 返回 97；PE32 继续将写入故障转换为 97，并保持 LastError。PE32 的 begin/end 保留非零 period 成功的无记账策略。

AMD64 直接复用旧 PeriodRequests owner。非空非法输出报告可恢复访存故障，DWORD 参数截断；最多记录 256 种非零 period，已有 period 可继续增加引用。End 减为零时删除请求，未找到返回 97。状态属于进程，线程间共享，启动拒绝与进程终止清空；不修改宿主 timer resolution。
