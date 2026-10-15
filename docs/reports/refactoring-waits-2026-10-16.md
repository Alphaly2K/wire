# WaitFor* 契约迁移

## 旧行为基线

四项 export 为 WaitForSingleObject/Ex、WaitForMultipleObjects/Ex。PE32 前三项 typed、MultipleEx 仍为 ABI-only；Win64 四项均支持 kernel32/kernelbase，USER 消息等待和 Winsock 等待不在本次契约范围。

运行时基线为 `df2d4ff`，冻结 before 为 `r2-mutex-wire/wire`，SHA256 `66ad662479909066aacc1642b25f4217a229bcdff30451d70a3e2518dd43739b`。三项新增独立测试在旧 handler 上通过：PE32 空数组、65→64 count clamp、MultipleEx error 120/20 字节 stdcall、SingleEx 忽略 alertable 及 acquire 在返回槽 fault 前提交；Win64 DWORD 截断、完整 64 位 handle、正确/错误 DLL、数组后续元素 fault 前不 acquire、不提交 CPU/API count。

既有 APC 用例扩为 SleepEx、SingleEx、MultipleEx 三种实际 guest 线程场景，以逐指令预算验证 callback 的 64 位参数、TID、线程 error mode 和返回 0xc0。旧网络 wait 回归也通过。日志为 `r2-waits-baseline-tests.txt` 和 `r2-waits-baseline-apc.txt`。

原创 `gateway-wait` 创建 manual/signaled event，循环零超时 Wait 并累加返回值。独立预期为返回 0、PE32 retired=6n+15、Win64 retired=6n+18/work=7n+19，两架构 API calls=n+1。before 的 12K Win64 与 PE32 raw/IR smoke 全通过，20 项工具检查通过。仅新增可选 case，默认测量集合不变；本批使用已修正的 event fixture，后续按实际输入哈希配对。

契约迁移尚未接入或验收。
