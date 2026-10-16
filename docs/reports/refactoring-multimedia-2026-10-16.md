# 多媒体计时器契约与状态迁移

## 独立旧行为基线

运行时基线 `04a5921`，before 为 `r2-calendar-wire/wire`，SHA256 `e4d015565582e01f496115997a1516ee1a11919dd043d2e595e99318ab526d70`。范围 timeGetDevCaps、timeBeginPeriod、timeEndPeriod；PE32 仅 winmm，Win64 静态 kernel32/kernelbase/winmm，动态查找仅 winmm。timeGetTime 保持已验收 ClockApi。

两项独立边界回归和三项相关回归共五项通过，固定 PE32 stateless period、短 buffer/写入故障返回 97、LastError 与 public facade 精确范围；Win64 256 种 period 上限、重复请求/释放恢复容量、DWORD 截断、保存返回帧、checked fault 与静态/动态 provider 差异。日志 `r2-multimedia-baseline-tests.txt`。

原创可选 `gateway-multimedia` 调用 timeGetDevCaps，累加 `(max >> 16) + min`，即 65536n。n=max(1,work//11)，Win64 retired=10n+10、work=11n+10，PE32 retired=11n+9；API calls=n。独立 PE inventory 确认两架构均为单一 winmm!timeGetDevCaps 导入、IAT 0x1190，证据 `r2-multimedia-baseline-imports.json`。旧实现的双架构 12K smoke（PE32 raw/IR）与 20 项工具检查通过，默认 case 集合保持。日志前缀 `r2-multimedia-baseline-`。

## 候选接入

三项 export 接入 MultimediaApi，删除根重复 ABI/arity/DWORD、provider 与名称分发。PE32 typed guard 追加到既有日历路由之后，public system facade 保留原精确 DLL 范围并委托。Win64 period map 收入私有 PeriodRequests，只提供 begin/end，保留容量、checked_add 与引用删除语义；adapter 只借 Memory 与该 owner，资源仍随 Runtime64 释放。

迁移后的五项领域回归通过，日志 `r2-multimedia-focused-tests.txt`。边界见[多媒体计时器](../architecture/multimedia.md)。

完整 release **473 passed / 7 ignored**、debug **321 passed**、native **7 passed**、工具 **20 passed** 及 fmt/strict Clippy/build 通过。冻结 `r2-multimedia-wire/wire`，SHA256 `632dbfd678d7a644959d120b92f03ae579d7ee2f36e2eef53652cf18d2ad4425`，**5,990,568 bytes**，较日历增加 **4,064 bytes**；265 个 Rust 文件（src/tests 257 个）全部核对 manifest 哈希一致，证据 `r2-multimedia-source-verification.json`。

## 初筛与调查

35 条路径、各三对全部结果校验。timeGetDevCaps 网关 Win64 **−11.752%**、PE32 IR/raw **−1.193%/−2.279%**。四条 PE32 IR 路径超过 3%：bit **+3.170%**、SystemInfo **+3.402%**、Local/Global **+6.115%**、TLS/FLS **+9.501%**；暂不接受，完整轮次保留于 `r2-multimedia-stable-summary-initial.json`。bit 扩至 900M，三个网关扩至 150M，并加入同路径/同哈希 before control、轮换三种标签顺序继续调查。

Alice 200M 三对全部通过，CPU **−0.331%**、wall **−0.549%**，保持预算出口、隐藏 314×211 窗口及 Draw/Present 0。证据 `r2-multimedia-stable-alice`；直接复用原 runtime，该启动前缀尚未进入实际绘制。

## 扩样与接受决定

四条路径三种顺序全部结果校验。bit、SystemInfo、Local/Global、TLS/FLS 的 after/before CPU 分别为 **−0.192%/+0.737%/+0.210%/+1.106%**；同哈希 control/before 分别为 **+0.143%/−0.261%/−0.167%/−1.574%**，after/control 为 **−0.334%/+1.001%/+0.379%/+2.723%**。Local/Global 三对为 **+4.716%/+0.490%/+0.065%**，全部离散轮次保留。汇总 `r2-multimedia-stable-summary-control.json`，不同 work 没有与初筛混合。

其余 31 条初筛路径最大回退 **+2.395%**，初筛 RSS 最多增加 **16KiB**。本批新增 typed 路由和 PeriodRequests owner 的正确性、性能及启动前缀验收通过，接受本批，契约累计覆盖 **281 个 export**。没有在正式计时中运行 Cargo、native 诊断或另一组测量；游戏输入沿用原目录，未复制或硬链接。
