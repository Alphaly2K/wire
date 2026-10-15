# Virtual memory 契约迁移

## 独立旧行为基线

八项 export 为 VirtualAlloc/Free/Protect/Query 及其 Ex 版本。运行时基线 `7b9caf6`，before 为 `r2-timers-wire/wire`，SHA256 `cf7f8d6f6bf3f24c48086bad3d8569fd89a11f591123d15a8513731a33488027`。双架构保持独立内存实现及缓存归属，PE32 接受 kernel32，Win64 接受 kernel32/kernelbase。

两项新增基线在旧 handler 上通过：PE32 固定 VirtualProtect 输出覆盖未保存返回槽时 EIP=4、成功 LastError 不变，以及远程无效 handle 先于输出检查；Win64 固定 SIZE_T 64 位、DWORD 截断、完整 64 位 handle、预先保存返回 frame，以及输出 alias 后的权限与 LastError。既有内存领域 16 项回归通过，日志 `r2-vmem-baseline-tests.txt`、`r2-vmem-baseline-memory-tests.txt`。

原创可选 `gateway-vmem` 先分配 4096 字节，循环 VirtualQuery 并累加返回结构大小及查询 BaseAddress 与分配地址的差值。独立预期 n=max(1, work//11)，PE32 EAX=28n、retired=11n+17；Win64 RAX=48n、retired=10n+18、work=11n+19；两端 API calls=n+1。独立 PE inventory 核对恰好两个导入及 IAT 地址；before 的 Win64、PE32 raw/IR 12K smoke 与 20 项工具检查通过。默认 case 集合保持，日志 `r2-vmem-baseline-*-smoke.txt`、`r2-vmem-baseline-fixture-imports.json`。
