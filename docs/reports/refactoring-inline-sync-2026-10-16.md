# R2/R3 内联同步接口（2026-10-16）

## 独立旧行为基线

起点 `9f02023`，旧 executable 使用 `r2-multimedia-wire/wire`，SHA256 `632dbfd678d7a644959d120b92f03ae579d7ee2f36e2eef53652cf18d2ad4425`。范围为七项 SRW、三项条件变量和两项 InitOnce；尚不迁移 critical section、WaitOnAddress 或线程创建。

迁移前五项独立回归通过：PE32 十二项保持 ABI-only、error 120、stdcall 清栈及输出不变；Win64 保持两个 kernel provider、返回故障先于语义、条件变量非零 token 拒绝与返回槽别名；SRW 多读者 token、归属/损坏诊断、写权限和阻塞返回帧；InitOnce DWORD flags、错误短路、输出预检与别名写入顺序；真实 guest 线程跨 SRW/InitOnce 两次阻塞后重试，并在 1/100000 work 切片下核对最终 context 和完成记录次数。证据 `wire-out/refactoring-2026-10-14/r2-inline-sync-baseline-tests.txt`。

新增可选 `gateway-inline-sync` 原创 fixture，不改变默认性能集合。每轮 TryAcquireSRWLockExclusive/ReleaseSRWLockExclusive，校验成功数和释放后 guest token，精确核对退休指令、work、API 次数；11000 work 旧二进制预检通过，证据 `r2-inline-sync-fixture-baseline`。性能正式验收待实现后运行。
