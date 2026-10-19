# Win64 注册表所有权迁移

状态：完成。独立旧行为基线 `71fef81` 在生产迁移前通过三项注册表测试。handler 从 `impl Runtime64` 移至私有 Registry，原资源表示、句柄编号、权限、错误、读写顺序保持；只借用 Memory、名称和参数。两处名称解码使用共用 text helper，旧 facade 保留委托。忽略空白并替换 owner/helper 名称后，整个 handler 与基线一致。

新增基线覆盖关闭不复用、完整句柄、4096 次累计耗尽优先于输出访存、输出失败不消耗编号，以及查询输出别名与一次性 guard 的提交顺序。既有长测试继续覆盖 CP932/MULTI_SZ、权限、大小查询、234、关闭重开、系统根和创建输出故障。

风险评估：资源表示和执行路径算法保持，未涉及 CPU、memory/cache、调度或绘制热点，**不做性能、游戏或 native 图形矩阵**。release Win64 集成 **260 passed**，debug 注册表 **3 passed**，fmt、strict all-target/all-feature Clippy、release build 通过。首次沙箱内整体测试因既有 Unicorn native 执行触发 SIGILL，原日志保留；沙箱外完整运行通过。

证据位于 `wire-out/refactoring-2026-10-14/r3-registry-*`，verification JSON 记录源码与构建 SHA，不复制游戏、源码树或 executable。本批未迁移 API 契约，累计仍为 **733 exports**。
