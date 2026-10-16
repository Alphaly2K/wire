# 系统信息与内存状态契约迁移

## 独立旧行为基线

范围为 GetSystemInfo、GetNativeSystemInfo、GlobalMemoryStatus、GlobalMemoryStatusEx 四项 export。运行时基线 `10fbe95`，before 为 `r2-local-memory-wire/wire`，SHA256 `4554773e51c5f828d21c637b2286b81c047bbb218e5272612cb90f85767ecae8`。PE32 四项 typed、kernel32 provider；Win64 前两项和 Ex typed、kernel32/kernelbase，GlobalMemoryStatus 继续 absent。

两项新增旧行为基线通过，固定 PE32 36-byte 与 Win64 48-byte SYSTEM_INFO、各架构虚拟硬件值、32/64-byte 内存状态、Ex length 校验、输出哨兵及 LastError。PE32 公共 `wire::system::dispatch` 精确接受 `kernel32.dll`，拒绝其它大小写和 kernelbase，继续保留；输出故障转 error 87，输出覆盖返回槽时 EIP 读取被覆盖内容。Win64 则先保存返回帧，受检输出故障不提交 CPU/API count/LastError。既有布局、内存记账与时钟相关回归一并通过，共五项，日志 `r2-system-profile-baseline-tests.txt` 和 `r2-system-profile-baseline-related-tests.txt`。

原创可选 `gateway-system-profile` 每轮 GetSystemInfo 后累加 architecture 和 page size，输出位于本地栈。n=max(1, work//9)，Win64 返回 4105n、retired=8n+10、work=9n+10；PE32 返回 4096n、retired=9n+9；API calls 均为 n。独立 PE inventory 确认单一 GetSystemInfo 导入及 IAT 0x1190；before 的双架构 12K smoke（PE32 raw/IR）及 20 项工具检查通过，默认 case 集合保持。日志前缀 `r2-system-profile-baseline-`。

## 候选接入

四项 export 接入三个 SystemProfileApi ID，删除 PE32 重复 ABI、Win64 arity 及根名称分发。双架构 adapter 只借内存与各自 LastError 槽；PE32 原硬件/内存状态 body 从 system.rs 迁出，保留 public facade 的精确 DLL 匹配并按家族契约委托，Win64 继续使用 Memory 的内存状态和原受检 copy-out。完整边界见[系统信息调用边界](../architecture/system-profile.md)。

首版完整 release **469 passed / 7 ignored**、debug **317 passed**、native **7 passed**、工具 **20 passed** 及 fmt/strict Clippy/build 通过。冻结 `r2-system-profile-wire/wire`，SHA256 `571ee3b9d029b7fd52377d920614a1bfd3ea9b96d486b16bf7c83406b8734288`，5,982,696 bytes，增加 400 bytes；准备后续日历草稿前，257 个 Rust 文件（src/tests 249 个）全部核对 manifest 哈希一致。

## 首版性能调查

31 条路径、各三对全部结果校验。GetSystemInfo 网关 Win64 **−13.208%**、PE32 IR/raw **−20.363%/−13.895%**。七条 PE32 初筛超过 3%：SSE/IR +3.554%、FP/raw +5.215%、bit/IR +5.125%、Local/Global IR/raw +4.334%/+9.758%、heap/raw +3.609%、vmem/raw +6.297%。全部初筛与离散轮次保留于 `r2-system-profile-stable-summary-initial.json`。

七条路径加入同 before 路径、同哈希 control，采用 after/before/control、before/control/after、control/after/before 三种顺序。SSE/bit 扩到 900M、gateway 扩到 150M；FP/raw 已约十秒，保持 300M。其余六项 after/before 为 −6.438%、−1.455%、−6.211%、+0.510%、+0.634%、+0.989%，但 Local/Global IR 仍 **+7.747%**（三对 +1.771%/+9.061%/+3.338%，control/before +0.034%、after/control +7.710%）。汇总 `r2-system-profile-stable-summary-control.json`；不同 work 独立保留。首版暂不接受。

首版 Alice 200M 六轮通过，CPU **−2.479%**、wall **−2.522%**，仍为隐藏 314×211 窗口和 Draw/Present 0。证据 `r2-system-profile-stable-alice`，沿用现有素材目录。

新增 SystemProfile guard 原先位于既有 typed 路由之前，会增加现有家族的判断。第二版把它追加到原 scalar typed 路由之后，保留既有内存路由的次序；完整检查与新性能验收进行中。新增日历测试也覆盖未迁移 legacy 路由，日历 adapter/catalog 草稿尚未注册。

## 第二版

七项系统信息/legacy 日历边界回归通过。新增日历基线首次误把 PE32 GetTimeZoneInformation 视为 ABI-only，独立测试发现其已有 172-byte 零结构 copy-out；修正预期并保持现有实现后通过，失败日志 `r2-system-profile-ordered-focused-tests.txt` 和修正后的 `r2-system-profile-ordered-focused-v2-tests.txt` 均保留。

完整 release **471 passed / 7 ignored**、debug **319 passed**、native **7 passed**、工具 **20 passed** 及 fmt/strict Clippy/build 通过。冻结 `r2-system-profile-ordered-wire/wire`，SHA256 `fce5b7ce8275b1d46187753b12e5150a3f023127e7660a4cd08c38bb80ce00d9`，5,982,696 bytes；257 个 Rust 文件（src/tests 249 个）全部核对 manifest 哈希一致。日历运行时代码草稿位于证据目录，尚未接入编译或纳入此候选。

先复测原有 Local/Global IR 回退，保持 150M work 和三顺序 before/after/control。第二版 CPU **−1.118%**，三对 **−1.808%/−0.210%/−1.118%**；同哈希 control/before **+0.654%**，after/control **−1.761%**。全部结果校验，汇总 `r2-system-profile-ordered-stable-local-ir-summary.json`。

完整 31 条路径、各三对全部结果校验，最大 CPU 回退 **+2.602%**，RSS 最多增加 **80KiB**。GetSystemInfo 网关 Win64 **−14.393%**、PE32 IR/raw **−19.368%/−17.740%**；Local/Global IR/raw **−0.398%/−0.597%**。汇总 `r2-system-profile-ordered-stable-summary-final.json`，原始目录前缀 `r2-system-profile-ordered-stable-`，首版数据独立保留。

Alice 200M 三对全部通过，CPU **−0.740%**、wall **−1.742%**，保持隐藏 314×211 窗口、Draw/Present 0 与预算出口。证据 `r2-system-profile-ordered-stable-alice`；直接复用原 runtime，没有复制游戏输入。该启动前缀尚未进入实际绘制。

接受第二版，契约累计覆盖 **267 个 export**。本批同时保留两项独立 legacy 日历回归，后续 Calendar 迁移以此冻结实现作为 before。
