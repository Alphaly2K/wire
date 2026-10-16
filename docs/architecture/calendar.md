# 日历与宿主计时调用边界

11 项 CalendarApi export 统一声明 provider、参数宽度与返回策略。PE32 的 GetSystemTime/GetLocalTime、GetTimeZoneInformation 为 typed；八项转换/计时接口保持 ABI-only error 120。Win64 十项 typed、FileTimeToLocalFileTime absent。CompareFileTime 保持原 DWORD 返回 carrier，负值为 0x00000000ffffffff。

PE32 适配器只借内存和 LastError。SYSTEMTIME 16-byte copy-out、时间字段 cast、错误 87 和成功保持 LastError 保持；时区 API 继续写出 172-byte 零结构与原 NLS trace note。公共 system facade 只委托原两项时间 API，公共 nls facade 只委托原时区 API，各自保留 DLL 精确大小写匹配。

Win64 适配器只借 Memory 与活动 TEB 的 LastError 地址。纯日历算术继续复用 win32/kernel/calendar；时区转换/结构编码从 Runtime64 的 impl 中分离为私有 zone 模块，保留 host tzdata、显式 DST 转换、重叠和缺失时间、访存/输出/错误顺序及已保存返回帧。原 byte buffer 编码保持。

GetSystemTimes 的宿主 CPU 计时独立为 host/cpu_time：接收无 guest 参数，返回三个数值；Win64 adapter 继续先 preflight 全部非空输出，再获取计时并 copy-out。host 模块不依赖 Memory、CPU 或 Runtime64。保留 macOS 公共 Mach ABI、Linux /proc/stat、错误码和 100ns 单位。

本批保持已有时区/日历语义及各架构差异，未添加新的 guest 能力。wall-time 和 monotonic clock 仍分别处理，不调整时钟来源或 host timezone 配置。
