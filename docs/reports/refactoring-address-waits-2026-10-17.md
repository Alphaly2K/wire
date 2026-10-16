# R2/R3 地址等待（2026-10-17）

## 独立旧行为基线

起点 `0f71923`，旧 executable 为 `r2-critical-sections-wire/wire`，SHA256 `8dec5f742f5673856f6e2633423bef15fdaba0351b039a675b639f1b81281a66`。范围 WaitOnAddress、WakeByAddressSingle、WakeByAddressAll；Win64 接受 kernel32/kernelbase 与 api-ms-win-core-synch-l1-2-0，PE32 目前 ABI 未知。

四项迁移前回归通过：新增 PE32 三 provider 的 unknown ABI、零参数清栈与 error 120；新增 Win64 三 provider 的 retry 不重复读取比较内存、changed-address 拒绝、size 检查先于 retry identity、CPU/API/LastError 提交；既有完整 SIZE_T/timeout/故障和真实 guest 多线程到达顺序、WakeSingle/WakeAll、无 wake credit、超时测试。证据 `r2-address-waits-baseline-tests.txt`。

新增可选 `gateway-address-wait`，显式八字节不等值比较，核对返回总和、退休指令、work 和 API 次数，9000 work 原版预检通过；默认集合不变。证据 `r2-address-waits-fixture-baseline`。

## 实现与正确性

基线提交 `dc4a6d7`。三项 export 接入 AddressWaitApi，删除根 provider 特例、arity/DWORD 及名称分发，冷绑定专用 control ID。PE32 absent 策略保留，未增设 typed guard。队列操作由 `impl Runtime64` 迁入线程 owner 的 `threads/address.rs`，私有 next_address_wait 序号与 blocking 注册由 Threads 自身管理；只借受检内存和当前错误槽。原 Runtime64 block facade 委托 Threads，CPU、回调和公共返回提交仍在进程层。

四项领域基线、完整 release **488 passed / 7 ignored**、debug **335 passed**、显式 native **7 passed**、工具 **20 passed** 均通过，fmt、strict all-target/all-feature Clippy 和 release build 通过。冻结 `r2-address-waits-wire/wire`，SHA256 `13422035b6ce571a74b94697bc07237b628e7be8e9771e700f2f360cf6fb6a90`，**5,997,624 bytes**，增加 **1,904 bytes**；274 个 Rust 文件（src/tests 266 个）全部与 manifest 核对一致，证据 `r2-address-waits-source-verification.json`。

## 性能与接受决定

稳定启动协议、三对轮换、40 条路径全部结果校验。地址等待网关 CPU **−0.351%**，最大回退 **+1.907%**（AMD64 memory），RSS 最多增加 **80KiB**；没有超过 3% 的路径。完整成对轮次及整数 IR 等差异均保留于 `r2-address-waits-stable-summary.json`，本批没有修改解释器算法。

Alice 200M 三对通过，CPU **−0.408%**、wall **+0.460%**，保持预算出口、隐藏 314×211 窗口、Draw/Present 0；证据 `r2-address-waits-stable-alice`。该启动前缀未进入实际绘制。编译、计时和 native 游戏严格串行，复用原 runtime，未复制或硬链接游戏。

接受本批，契约累计覆盖 **304 个 export**。新增 API 指引也已更新，集中说明领域控制 outcome、状态 owner、架构 fault 差异与 absent/ABI-only 清栈边界。下一批可先处理线程 scalar 查询/设置，保留 CreateThread/ExitThread/APC/CONTEXT 的专用进程控制边界；Mono 继续排除。
