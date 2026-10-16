# 日历、时区与 CPU 计时契约迁移

## 独立旧行为基线

运行时基线 `0044ae6`，before 为 `r2-system-profile-ordered-wire/wire`，SHA256 `fce5b7ce8275b1d46187753b12e5150a3f023127e7660a4cd08c38bb80ce00d9`。范围为 11 项 export；PE32 的 GetSystemTime/GetLocalTime/GetTimeZoneInformation 已有实现，其余八项保留 ABI-only error 120；Win64 十项已有实现，FileTimeToLocalFileTime absent。

两项独立 legacy 日历边界回归已随系统信息批次冻结。本批在迁移前重跑七项日历/时区/CPU 计时回归，固定 public facade 的精确 DLL/名称范围、PE32 172-byte 零时区 copy-out、LastError/trace、Win64 保存返回帧、CompareFileTime 的 DWORD carrier、日期校验/DST/alias 与受检输出故障。日志 `r2-calendar-baseline-calendar-tests.txt`、`r2-calendar-baseline-timezone-tests.txt`、`r2-calendar-baseline-system-times-tests.txt`。

原创可选 `gateway-calendar` 循环转换固定 1970-01-01 SYSTEMTIME，累加返回值和 FILETIME；n=max(1,work//9)，返回 `(116444736000000001*n) mod 2^64`，retired=8n+13、work=9n+13、API calls=n。单导入 inventory 见 `r2-calendar-draft-fixture-imports.txt`；旧实现 12K smoke 和 20 项工具检查通过，默认 case 集合保持。日志前缀 `r2-calendar-baseline-`。

## 候选接入

11 项 export 接入 CalendarApi，删除重复 PE32 ABI、Win64 arity 与根名称分发。PE32 typed guard 追加在 SystemProfile 后，保留已有路由次序。两个 public facade 保留各自原 DLL/名称范围；新增回归核对 NLS 时区 note、输出哨兵和 System facade 不接收其它日历接口。

Win64 handler 只借 Memory 与活动 TEB 错误地址，时区逻辑分离到私有 `win32/x64/calendar/zone`。宿主 CPU 计时迁至 `host/cpu_time`，不再依赖 guest Memory 或 Runtime64。独立比较确认原 CPU 函数 body 相同，zone body 在移除 Runtime64 wrapper 后相同，证据 `r2-calendar-extraction-equivalence.json`。完整边界见[日历调用边界](../architecture/calendar.md)。

首次编译发现 timer 模块仍通过父模块导入 SystemTime，改为直接导入标准库；失败日志 `r2-calendar-focused-tests.txt` 保留。修正后七项领域回归通过，日志前缀 `r2-calendar-focused-v2-`。

完整 release **471 passed / 7 ignored**、debug **319 passed**、native **7 passed**、工具 **20 passed** 及 fmt/strict Clippy/build 通过。冻结 `r2-calendar-wire/wire`，SHA256 `e4d015565582e01f496115997a1516ee1a11919dd043d2e595e99318ab526d70`，**5,986,504 bytes**，比系统信息增加 **3,808 bytes**；261 个 Rust 文件（src/tests 253 个）全部核对 manifest 哈希一致，证据 `r2-calendar-source-verification.json`。

## 性能与接受决定

32 条路径、各三对全部结果校验；日历转换网关 CPU **−14.970%**，PE32 路径最大回退 **+2.771%**，RSS 最多增加 **208KiB**。三个 Win64 网关初筛超过 3%：线程 ID **+3.977%**、Local/Global **+3.514%**、时钟 **+3.942%**。全部初筛及离散轮次保留于 `r2-calendar-stable-summary-initial.json`。

三条路径扩大至 300M work，加入同 before 路径、同哈希 control，采用 after/before/control、before/control/after、control/after/before 三种顺序。CPU after/before 分别为 **+1.647%/+4.570%/+2.630%**；control/before 为 **−2.175%/+1.507%/+0.926%**，after/control 为 **+3.906%/+3.018%/+1.689%**。Local/Global 三对 after/before **+0.789%/+4.570%/+4.041%**。不同 work 独立保留，汇总 `r2-calendar-stable-summary-control.json`。

正式计时后独立 diagnostics：线程 ID、Local/Global、时钟两版分别为 IR **9610/10016/10011**，translations **14/21/16**，fallback/invalidations 均 **0**。CheckedSegment/RegisterSegment 的生成指令数仍为 **2156/1968**，各自助记符序列 SHA 相同，原始反汇编与 `r2-calendar-ir-codegen.json` 保留；该比较没有核对全部操作数。catalog_call 由 17,500 增至 17,528 bytes，dispatch_bound 从 58,572 缩至 55,440 bytes，符号地址发生变化；局部成本的具体机制尚未确证，证据 `r2-calendar-symbol-layout.json`。

Alice 200M 三对全部通过，CPU **−0.121%**、wall **−0.026%**，保持预算出口、隐藏 314×211 窗口及 Draw/Present 0。证据 `r2-calendar-stable-alice`；直接复用原 runtime，该启动前缀尚未进入实际绘制。

接受本批并记录 Local/Global 网关扩样后的有限局部成本；没有三对均超过 5% 的路径，解释器/cache 行为计数、完整正确性和真实启动出口保持。契约累计覆盖 **278 个 export**。后续多媒体计时器草稿未注册，未混入冻结源码和本次验收。
