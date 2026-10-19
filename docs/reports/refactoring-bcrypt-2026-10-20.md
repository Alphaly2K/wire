# BCrypt RNG 契约与所有权

状态：完成，累计 **753 exports**。独立旧行为基线 `19517df` 通过后，三项 API 汇入 BcryptApi，原 RNG 句柄集合和三项处理函数收进私有 RngProviders。删除 root 的重复 provider、arity、DWORD 和名称路由。三项 handler 经 ID/owner/helper 改名和空白归一后与旧实现一致，保留 OS entropy、分块、句柄复用、错误、输出及返回顺序。

新增第 64 个家族时，原多层 `Iterator::chain` 类型触发 Rust E0275 递归上限。catalog 改用静态 slice 分组和 flat_map，既有 63 个家族顺序逐项核对保持，不增加堆分配、运行时规则或 trait object。首次编译错误日志保留。

风险评估：RNG 资源表示/算法保持，catalog 仍在绑定/查询冷阶段遍历；无需性能、采样、游戏或 native 图形矩阵。相关 release **3 passed**，catalog 一致性、fmt、strict all-target/all-feature Clippy、release build 通过。公共 catalog 变化另做一次整合 release 正确性回归：**670 passed / 7 ignored**；不把这次整合变为每批要求。

独立用例覆盖 provider 静态/动态差异、DWORD trace、完整句柄、关闭复用、flags/handle/长度错误优先级，以及 1024 容量耗尽时的输出预检和故障不提交。原真实 OS entropy 测试保持。证据 `wire-out/refactoring-2026-10-14/r2-bcrypt-*`、`r2-registry-bcrypt-integration-release.txt`；不复制 executable、源码树或游戏。
