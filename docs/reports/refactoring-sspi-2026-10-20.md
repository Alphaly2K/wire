# SSPI 契约与函数表所有权

状态：完成，累计 **787 exports**。独立旧行为基线 `613383c` 先于生产迁移通过。34 项 SspiApi 收敛 provider/arity/DWORD 元数据与执行路由，删除 legacy 字符串规则。两个函数表地址移入私有 Security，handler 显式借用 Memory、分配游标和 Gateways；网关冷注册移入现有 Gateways，Runtime64 facade 保留。

34 项参数数量和 DWORD 位点逐项与旧表比较一致；函数表主体经 owner/borrow 改名与空白归一后保持。保留 A/W 布局、空槽、只读页、地址/注册/故障/提交顺序，以及空包集、错误、输出和 LastError。原始能力范围见[领域边界](../architecture/sspi.md)。

风险评估：迁移涉及冷阶段函数表构造和注册，不改执行器、memory/cache、热网关查找、调度或图形；不做性能、游戏或 native 图形矩阵。release Win64 **264 passed**，包含两项原函数表测试和新 provider 基线；另验收现有网关单元测试、catalog 一致性、fmt、strict all-target/all-feature Clippy 和 release build。上一公共 catalog 整合 release 670/ignored7 保留原上下文，没有在本批重跑全架构。

证据位于 `wire-out/refactoring-2026-10-14/r2-sspi-*`；verification JSON 记录源码与 executable SHA，无额外游戏或源码副本。
