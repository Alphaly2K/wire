# R2/R3 内联同步接口（2026-10-16）

## 独立旧行为基线

起点 `9f02023`，旧 executable 使用 `r2-multimedia-wire/wire`，SHA256 `632dbfd678d7a644959d120b92f03ae579d7ee2f36e2eef53652cf18d2ad4425`。范围为七项 SRW、三项条件变量和两项 InitOnce；尚不迁移 critical section、WaitOnAddress 或线程创建。

迁移前五项独立回归通过：PE32 十二项保持 ABI-only、error 120、stdcall 清栈及输出不变；Win64 保持两个 kernel provider、返回故障先于语义、条件变量非零 token 拒绝与返回槽别名；SRW 多读者 token、归属/损坏诊断、写权限和阻塞返回帧；InitOnce DWORD flags、错误短路、输出预检与别名写入顺序；真实 guest 线程跨 SRW/InitOnce 两次阻塞后重试，并在 1/100000 work 切片下核对最终 context 和完成记录次数。证据 `wire-out/refactoring-2026-10-14/r2-inline-sync-baseline-tests.txt`。

新增可选 `gateway-inline-sync` 原创 fixture，不改变默认性能集合。每轮 TryAcquireSRWLockExclusive/ReleaseSRWLockExclusive，校验成功数和释放后 guest token，精确核对退休指令、work、API 次数；11000 work 旧二进制预检通过，证据 `r2-inline-sync-fixture-baseline`。

## 实现与正确性

基线提交 `f532271`。十二项 export 收进 InlineSyncApi，Win64 冷绑定专用 control ID；删除根 arity/DWORD 表和名称/前后缀分支。SRW map 迁入私有 SrwLocks owner，先准备更新、成功写 guest token 后再提交；调度器借用 available 查询，保留原就绪规则。窄 adapter 仅借内存、锁 owner、当前 tid 与 LastError 槽，显式输出完成/Try失败/两种阻塞，进程保留返回帧和计数提交。PE32 不新增 typed 路由，保持 ABI-only。

完整回归通过：release **479 passed / 7 ignored**，debug **326 passed**，显式 native **7 passed**，工具 **20 passed**；fmt、strict all-target/all-feature Clippy、release build 通过。冻结 `r2-inline-sync-wire/wire`，SHA256 `8ad9ef1977f5d38c5c54379f90a4da7e26b54dce1bd79a54528a02e7a5c6718c`，**5,996,696 bytes**（增加 6,128 bytes）。268 个 Rust 文件、其中 src/tests 260 个，全部与 manifest 核对一致，证据 `r2-inline-sync-source-verification.json`。

## 性能与接受决定

稳定启动协议、三对轮换、36 条路径全部结果校验。SRW 网关 CPU **−0.570%**，全矩阵最大回退 **+1.837%**（日历网关），RSS 最多增加 **176KiB**；没有超过 3% 的路径。完整结果 `r2-inline-sync-stable-summary.json`，输入与 executable 哈希均记录在各目录 manifest。

Alice 200M 三对通过，CPU **−1.825%**、wall **−0.600%**，仍为预算出口、隐藏 314×211 窗口、Draw/Present 0；证据 `r2-inline-sync-stable-alice`。该前缀尚未进入实际绘制。编译、正式计时与 native 游戏严格串行，直接复用原 runtime，未复制或硬链接游戏。

接受本批，契约累计覆盖 **293 个 export**。后续继续 critical section 与地址等待等同步家族；此批没有新增同步能力或修改 Mono。
