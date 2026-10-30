# NLS / locale 边界

新 Wasm 入口通过 `crates/wire-wasm/src/win32/nls.rs` 接入下列 43 项服务，两种 ABI 均接受 kernel32/kernelbase。共享目录共记录 46 项 NLS 契约，旧入口的执行策略与新入口的接入范围分别维护。

| 家族 | 新入口支持 |
| --- | --- |
| 代码页 | GetACP、GetOEMCP、GetConsoleCP、GetConsoleOutputCP、GetCPInfo、GetCPInfoExA/W、IsValidCodePage、IsDBCSLeadByte/Ex |
| 转换 | MultiByteToWideChar、WideCharToMultiByte |
| locale 标识 | GetUserDefaultLCID、GetSystemDefaultLCID、Get/SetThreadLocale、GetUserDefaultLangID、GetSystemDefaultLangID、GetUserDefaultLocaleName、GetSystemDefaultLocaleName、LocaleNameToLCID、LCIDToLocaleName、IsValidLocaleName、IsValidLocale |
| locale 数据 | GetLocaleInfoA/W/Ex |
| 字符处理 | GetStringTypeA/W/ExA/ExW、LCMapStringA/W/Ex |
| 日期/时间文本 | GetDateFormatA/W/Ex、GetTimeFormatA/W/Ex |
| locale 枚举 | EnumSystemLocalesA/W/Ex |

虚拟默认代码页为 CP932，默认 locale 为 ja-JP/0x411；locale 词汇还包括 en-US、invariant 以及 ja/en neutral 名称。代码页转换支持 CP932、CP1252 和 UTF-8，0/1 映射为 CP932；CP_THREAD_ACP（3）按当前线程 locale 选择 en 的 CP1252 或 ja 的 CP932，invariant 和没有 ANSI 代码页的值回到进程 CP932。IsValidCodePage 只接受实际支持的代码页编号。CPINFO/CPINFOEXA/CPINFOEXW 分别输出完整的 20/284/544 字节结构。DBCS 只在 CP932 的 lead-byte 范围返回 TRUE，Ex 版本读取所指定的代码页。

转换按显式有符号计数处理内嵌 NUL；-1 包括终止 NUL，零或小于 -1 的输入计数失败。容量为零时仅返回所需长度，不访问目标缓冲区；负容量返回零/87。输入与输出为同一非查询地址时返回零/87。有限输入最多 16 MiB，终止输入最多 32768 单位；跨页读取保持精确范围，不扩大到字符或页之外。

MultiByteToWideChar 的 UTF-8 仅接受零或 MB_ERR_INVALID_CHARS；CP932/1252 接受默认预组合与 strict decode，未接入的 composite/glyph 处理返回零/50。严格解码遇无效序列返回零/1113，宽松解码使用替换字符。WideCharToMultiByte 的 UTF-8 仅接受零或 WC_ERR_INVALID_CHARS，default/used-default 指针必须为 NULL；其余两种代码页支持默认替换、自定义单/双字节替换及 WC_NO_BEST_FIT_CHARS。禁用 best-fit 时仅保留能按现有转换表往返到原 Unicode 标量的映射，其余使用默认字符并标记 used-default。组合字符选项尚未接入。

所有输入先复制，转换结果与 used-default BOOL 的写范围全部预检后才依次发布；它们可以重叠，BOOL 按最后写入的顺序保留。容量不足返回零/122，非法 flags 返回零/1004，参数错误返回零/87。访存故障保留 CPU、计数与输出，修复后可重试；错误槽和返回槽沿统一服务协议处理。成功保持 LastError，完成计一 work、不计客体退休。仅日期/时间格式的 NULL SYSTEMTIME 输入采样墙钟，其余 NLS 调用不采样时间。

LCIDToLocaleName 允许零容量查询并返回含 NUL 的长度；两个默认名称查询要求非 NULL 且足够大的缓冲区，零容量返回零/122。neutral 名称按 LOCALE_ALLOW_NEUTRAL_NAMES 选择。GetLocaleInfo 支持现有 locale 词汇、长度查询及 LOCALE_RETURN_NUMBER；数值输出为 DWORD，A 返回四个字节、W/Ex 返回两个 UTF-16 单位。

GetStringTypeA/ExA 使用 locale 的 ANSI 代码页解码，再为每个 UTF-16 单元输出一个 WORD，DBCS 字符只对应一个单元；源字节数大于实际输出元素数时不写多余位置。GetStringTypeW/ExW 直接按 UTF-16 单元分类，ExW 忽略 locale 参数。四项接口均仅接入 CT_CTYPE1；C2/C3 返回零/50，未知类型返回零/1004。NULL 输出、NULL 源和零输入计数返回零/87；任意负计数按 NUL 终止输入并包含终止单元。

LCMapStringA/W/Ex 接入 upper/lower，继续使用既有 Rust Unicode 表和大小写规则；Wide 输入中的未配对代理项原样保留，完整代理对按 Unicode 标量映射。A 输入及输出使用 locale 的 CP932/CP1252，invariant 没有默认 ANSI 代码页时回到进程 CP932；LOCALE_USE_CP_ACP 显式选择进程代码页。任意负输入长度包含 NUL，显式长度保留内嵌 NUL；零容量查询返回实际输出字节数或 UTF-16 单元数。NULL 的非查询输出或负容量返回零/87，容量不足返回零/122 且不写前缀，支持输入/输出重叠及覆盖已保存返回槽。未知 flags 返回零/1004，已识别但未迁移的排序键、语言 casing、title case、kana/width 等选项返回零/50。当前 Unicode 规则未替换为 Windows 的完整 NLS 字符数据库；C2/C3、collation、排序键和语言映射仍属缺口。

GetThreadLocale 读取当前线程 TEB 的 DWORD CurrentLocale；初始线程和新线程均从用户默认 0x411 初始化，原始零值在查询时恢复为该默认。SetThreadLocale 接受现有词汇的 LCID，default/neutral 值先归一化，再更新当前线程；未知值返回零/87，失败不修改旧值。成功保持 LastError，不改变用户/系统 locale 或进程 ACP。IsValidLocale 按现有 0x409/0x411/0x7f/9/0x11 词汇判断，接受 installed/supported、零与兼容标志 0x39；其它标志或 default 别名返回 FALSE，保持 LastError。IsValidLocaleName 判断实际名称，两个默认名称指针哨兵返回 FALSE。

EnumSystemLocalesEx 枚举现有 en-US、ja-JP、en、ja 与 invariant 名称，分别提供 WINDOWS|SPECIFICDATA 或 WINDOWS|NEUTRALDATA；标志按并集筛选，零为全部，当前词汇没有 supplemental 或 alternate-sort 项。A/W 仅枚举两个 specific LCID 的八位十六进制文本，不包含 invariant 或 neutral；零等同 supported，installed 与 supported 互斥。未知标志返回零/1004，NULL callback 或非 NULL reserved 返回零/87；Ex 的 lParam 保留完整指针宽度。

枚举 callback 使用同一 JIT 和每线程续点，字符串位于该次客体调用的临时栈空间，只在回调期间有效。返回 BOOL 按低 DWORD 判断，FALSE 提前停止后 API 仍成功；回调对 LastError 和线程 locale 的修改继续保留。首个字符串和调用帧全部预检后才发布续点；下一回调准备失败保留已完成的 BOOL 和游标，不重放回调。嵌套枚举、Sleep、异常、DLL 通知和 AMD64 目标展开共用既有调度，正常结束、非局部展开或线程退出均丢弃相应队列，无独立客体堆分配。

日期/时间格式支持现有 en-US、ja-JP、invariant 及 neutral 词汇的公历、默认短/长/年月/月日图片、自定义图片、单引号文字和转义。W/Ex 图片的原始 UTF-16 文字逐单元保留；A 图片及输出使用 locale 的 CP932/CP1252，LOCALE_USE_CP_ACP 改用进程 CP932。计数包含 NUL；零容量不访问输出，容量不足不发布前缀。输出可与已保存的输入、图片或原返回槽重叠。

日期仅验证 1601–9999 的年月日，重算 weekday 并忽略时间字段；时间仅验证时分秒和毫秒，忽略日期字段。时间图片支持四种 TIME 标志，强制 24 小时制保留原有 marker，只有 TIME_NOTIMEMARKER 才去掉 marker；抑制组件时一并处理关联分隔符。NULL SYSTEMTIME 使用与日历服务相同的进程时区，未配置的 VM 为 UTC。日期样式标志互斥，显式图片不能与日期样式或 LOCALE_NOUSEROVERRIDE 组合；DATE_AUTOLAYOUT 在当前 locale 词汇中不增加方向标记。alternate calendar、era 图片和显式 bidi 标记尚未接入，返回零/50；不以公历文字替代。GetDateFormatEx 的保留参数必须为 NULL。

便携转换表位于 `src/codepage/portable.rs`，locale 词汇、格式 token、枚举筛选和 C1 分类位于 `src/win32/kernel/nls.rs`，UTF-16 数值转换位于 `src/win32/utf16.rs`；均不依赖旧 CPU 或内存。新字符/格式适配器位于 `crates/wire-wasm/src/win32/nls/characters.rs` 和 `calendar.rs`，枚举续点位于 `crates/wire-wasm/src/process/nls.rs`；旧 text/枚举适配器复用相同词汇和函数。CompareStringA/W/Ex 尚未接入新入口。

参数边界依据微软公开的 [MultiByteToWideChar](https://learn.microsoft.com/en-us/windows/win32/api/stringapiset/nf-stringapiset-multibytetowidechar)、[WideCharToMultiByte](https://learn.microsoft.com/en-us/windows/win32/api/stringapiset/nf-stringapiset-widechartomultibyte)、[GetUserDefaultLocaleName](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getuserdefaultlocalename)、[LCIDToLocaleName](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-lcidtolocalename)、[GetDateFormatEx](https://learn.microsoft.com/en-us/windows/win32/api/datetimeapi/nf-datetimeapi-getdateformatex)、[GetTimeFormatEx](https://learn.microsoft.com/en-us/windows/win32/api/datetimeapi/nf-datetimeapi-gettimeformatex)、[EnumSystemLocalesEx](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-enumsystemlocalesex)、[SetThreadLocale](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-setthreadlocale)、[GetStringTypeA](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getstringtypea) 和 [LCMapStringA](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-lcmapstringa) 契约核对。

## 语言排序缺口

CompareStringA/W/Ex 在两种 ABI 下已有入口，校验已知 flags、当前支持的 locale 及非 NULL 字符串。未知 flags 返回零/1004，非法 locale 或 NULL 字符串返回零/87；其他调用返回零/50，字符串 payload、version、reserved 和 handle 指向的存储保持。

当前没有 Windows linguistic sort weights、语言例外、compression、组合字符、diacritic、kana 或 width 的排序后端。CompareStringOrdinal 的序数比较独立存在；语言排序入口不返回近似字典序结果。
