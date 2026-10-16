# R2/R3 线程查询与设置（2026-10-17）

## 独立旧行为基线

起点 `84e6fe0`，旧 executable 为 `r2-address-waits-wire/wire`，SHA256 `13422035b6ce571a74b94697bc07237b628e7be8e9771e700f2f360cf6fb6a90`。范围 Get/SetThreadErrorMode、Resume/SuspendThread、GetThreadId、Get/SetThreadPriority、GetExitCodeThread、SetThreadAffinityMask，共九项 export；Create/ExitThread、APC、CONTEXT 和 description 分配不在本批。

新增六项独立迁移前回归全部通过。PE32 固定二态挂起/恢复、优先级空操作、无效 handle 查询 exit 仍成功写零、pseudo handle 差异、输出故障与未保存返回槽别名；GetThreadId 和两项 error mode 保持 Wine ABI 已知但未实现，错 provider 保持未知 ABI/零参数清栈。Win64 固定完整 handle/affinity carrier、优先级 DWORD 截断及零扩展返回、127 层挂起上限、invalid handle 先于参数/输出检查、mode 校验/输出 fault/state 提交顺序、guard、保存返回帧和错 DLL 拒绝。证据 `r2-thread-properties-baseline-tests.txt`。

新增可选 `gateway-thread-properties`，循环设置并读回优先级，独立核对返回总和、退休指令和 API 次数；Win64 另核对 work。两种架构及 PE32 raw/static IR 在旧 executable 上 9900 work 预检通过，默认 fixture 集合保持。证据 `r2-thread-properties-fixture-baseline64`、`r2-thread-properties-fixture-baseline32`。
