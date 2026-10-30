# 日历与时区

新 Wasm 入口在 PE32 与 AMD64 均支持下列十一项服务，通过 kernel32/kernelbase 提供。接口声明复用 `win32/catalog/calendar.rs`，新入口的资格与执行独立于旧架构策略。

| 服务 | 契约 |
| --- | --- |
| GetSystemTimes | 从独立 CPU 提供者写出 aggregate idle/kernel/user 三个累计 100 ns 时长，kernel 包含 idle；允许任意 NULL 输出，先检查全部非空输出再提交 |
| GetSystemTime、GetLocalTime | 从 `wallClock()` 的 FILETIME 转换为 16 字节 SYSTEMTIME；前者使用 UTC，后者使用进程时区 |
| GetSystemTimePreciseAsFileTime | 写出提供者给出的完整 64 位 FILETIME，不截去低位精度 |
| SystemTimeToFileTime | 校验 1601–30827 年的公历日期、时分秒和毫秒，忽略输入星期；成功写出 8 字节 FILETIME |
| FileTimeToSystemTime | 转换到受支持的公历范围并计算星期，舍弃不足一毫秒的尾数 |
| CompareFileTime | 按完整无符号 FILETIME 比较，返回 DWORD 形式的 −1、0 或 1；AMD64 的 −1 carrier 为 `0x00000000ffffffff` |
| FileTimeToLocalFileTime | 应用当前季节的时区偏移，保留原 FILETIME 的全部 100 ns 精度；拒绝最高位为 1 或偏移后越界的输入 |
| GetTimeZoneInformation | 写出配置的 172 字节结构；固定偏移返回 UNKNOWN，季节规则按墙钟返回 STANDARD/DAYLIGHT；失败返回 DWORD −1 和 LastError |
| SystemTimeToTzSpecificLocalTime、TzSpecificLocalTimeToSystemTime | NULL 时区使用进程时区；显式结构覆盖默认配置，支持固定分钟偏移、年度相对规则和绝对转换日期 |

日期运算和 Windows 时区规则直接复用便携的 `win32/kernel/calendar.rs` 与 `timezone.rs`，核心不调用旧执行器或宿主 tzdata。无切换日期时只应用 Bias，忽略额外的 StandardBias/DaylightBias。相对规则支持最后一周和跨年夏令时；回拨的重叠时段优先标准时间，向前跳时的缺口返回错误 87。

`new WasmMachine(core, {timeZone})` 接受 172 字节 `Uint8Array` 形式的 TIME_ZONE_INFORMATION，复制输入并在装载前校验规则；标准偏移的秒数须能表示为 CRT 的有符号 32 位值。省略时为 UTC。Rust 使用 `Machine::configure_timezone`，底层 `wire_configure_timezone` 从公共输入区取同样的结构；失败保留原配置。已装载或执行过的 VM 拒绝重配。配置随静态/动态装载及子进程继承，GetLocalTime、NULL 时区转换、空日期/时间格式输入与 CRT `_timezone`/`_daylight` 共用它；CRT 单元初始化后仍属于客体可写变量。

生产安装清单可携带 `timeZone` 字节数组。启动器默认通过独立 `wire-host` 复用既有原生时区发现，发现年度标准/DST 偏移和切换规则后一次供给。macOS/Linux 的宿主规则不能表示为该 Windows 结构时明确失败，可通过 `--timezone utc` 选择 UTC。仅支持同年一组标准/DST 切换，不伪造政治性基础偏移变更或多个独立季节的规则；核心使用所提供规则，不提供历史时区数据库。

转换服务先读取完整输入，再写入完整输出，允许输入、时区结构或原返回槽与输出重叠。非法日历或时区返回 FALSE 和 LastError 87，不访问结果指针；成功保持 LastError。访存故障保留 CPU、计数和未提交的输出，完成使用预先保存的返回帧。GetSystemTime、GetLocalTime 和精确 FILETIME 查询采样墙钟；GetTimeZoneInformation 和 FileTimeToLocalFileTime 仅在默认规则含季节切换时另取当前墙钟，其余转换无需平台时间。GetSystemTimes 只请求独立 CPU 样本，不读取经过时间或墙钟。

墙钟提供者出错或给出无法转换为 SYSTEMTIME 的值时，调用停在原网关并报告接口错误，允许修复后重试。GetSystemTimes 的 `cpuTimes()` 返回三个 unsigned 64-bit BigInt 组成的数组 `[idle, kernel, user]`、表示缺少提供者的 `null`，或 `{error}` 指定非零 DWORD Windows 错误。`run()` 也接受返回上述结果的 Promise；同步 `service()` 要求同步提供者。取样前检查返回帧、参数和全部输出，异步等待期间不推进客体，并拒绝 CPU 修改和进程组服务重入；提供者拒绝或 VM 销毁后不提交过期结果。kernel 小于 idle、非法类型/范围或提供者抛错均保留调用供重试。

缺少提供者时返回 FALSE/LastError 50，指定错误时原样返回该错误，全部输出保持原值。成功保持 LastError，按输出参数顺序写入，因此重叠输出由后一个参数覆盖。提供者随进程创建继承。生产启动器在 macOS/Linux 使用 `wire-host` 采集真实 aggregate idle/kernel/user 累计时间，通过独立服务边界传入；JSON 使用十进制字符串保留完整 u64，不使用经过时间或指令计数代替。其他宿主返回错误 50。NLS 日期与时间文本格式属于独立的 [NLS 接口](nls.md)。

显式真实宿主检查为 `python3 tools/probes/check_wasm_cpu.py`，需要 macOS/Linux 的可用系统 CPU 计数、Node、Rust 和 release 核心 Wasm；可用 `--core`、`--host-library` 指定本项目已构建产物。工具复用生产采样库、HTTP、浏览器样本解码与 WasmApplication，在 PE32/AMD64 各核对一次完整输出及正常退出，并记录核心/宿主产物和源码哈希；生成的输入自动清理，结果保存在忽略的 target 目录。该工具位于常规测试路径之外；常规 `cpu-times.mjs` 使用稳定样本验证协议。

`python3 tools/probes/check_wasm_timezone.py` 在同一显式环境中验证原生宿主时区及 UTC/南北半球冬夏规则，经生产清单与 HTTP 输入两种客体；Windows 日历、FILETIME、NLS 和默认双向转换逐值对照宿主时间库，进程内临时选择 TZ 不改变系统设置。常规 `timezone.mjs` 使用固定规则和日期，不依赖本机时区数据库。
