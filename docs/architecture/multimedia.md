# 多媒体计时器调用与资源边界

timeGetDevCaps、timeBeginPeriod、timeEndPeriod 通过三个 MultimediaApi ID 唯一声明 provider、ABI 与返回策略。PE32 仅 winmm；Win64 静态 kernel32/kernelbase/winmm 均保持可用，动态 GetProcAddress 仅 winmm。timeGetTime 继续属于已迁移的 ClockApi。

PE32 adapter 只借 Args 和 GuestMem。能力输出仍为 1..UINT_MAX，短 buffer 和写入故障返回 97，LastError 保持；begin/end 对非零 period 返回成功，不引入请求记账。public system facade 保持精确 winmm.dll 和原三项名称范围。

Win64 adapter 只借 Memory 和 PeriodRequests，保留 null/短 buffer 返回 97、非空非法输出的 checked fault、DWORD 参数截断和已保存返回帧。请求 map 由私有 PeriodRequests 拥有，最多 256 种非零 period；已有 period 可继续增加引用，checked_add 失败维持原错误。End 减为零时删除请求，未找到返回 97。资源随 Runtime64 释放，不添加宿主 timer-resolution 设置。
