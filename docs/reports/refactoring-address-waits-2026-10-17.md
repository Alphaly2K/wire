# R2/R3 地址等待（2026-10-17）

## 独立旧行为基线

起点 `0f71923`，旧 executable 为 `r2-critical-sections-wire/wire`，SHA256 `8dec5f742f5673856f6e2633423bef15fdaba0351b039a675b639f1b81281a66`。范围 WaitOnAddress、WakeByAddressSingle、WakeByAddressAll；Win64 接受 kernel32/kernelbase 与 api-ms-win-core-synch-l1-2-0，PE32 目前 ABI 未知。

四项迁移前回归通过：新增 PE32 三 provider 的 unknown ABI、零参数清栈与 error 120；新增 Win64 三 provider 的 retry 不重复读取比较内存、changed-address 拒绝、size 检查先于 retry identity、CPU/API/LastError 提交；既有完整 SIZE_T/timeout/故障和真实 guest 多线程到达顺序、WakeSingle/WakeAll、无 wake credit、超时测试。证据 `r2-address-waits-baseline-tests.txt`。

新增可选 `gateway-address-wait`，显式八字节不等值比较，核对返回总和、退休指令、work 和 API 次数，9000 work 原版预检通过；默认集合不变。证据 `r2-address-waits-fixture-baseline`。正式验收待实现后执行。
