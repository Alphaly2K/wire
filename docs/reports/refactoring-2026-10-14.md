# 渐进重构实施记录

按用户提供的 2026-10-11 计划实施，原生 Mono/R6 忽略。用户要求直接在本地 main 分批提交；不创建 worktree，不 push。用户未跟踪设计文档、PDF、IDE 与系统文件保持现状。

## R0 固定基线

- 代码：`2e78377`，tracked working tree 干净。
- 工具链：rustc 1.98.1 (`48a229cea`)，Cargo 1.98.1，aarch64-apple-darwin。
- before binary SHA-256：`b9c486ecde04a645dc5b3a4fb8b115641d1f4d7b9d115b8c822368885b224184`。
- 默认 release：382 passed / 0 failed / 7 ignored；测试名已保存，后续 harness 迁移逐名核对。
- 原创 V2 fixture 原文件 SHA-256：`8cea5037a73ded7c00f483244ae0eaed1dcad3830a7a6546c33e731f4dc23fff`；仅存在于 ignored audit 目录的生成器将在 R0 纳入版本控制。
- 本地输入与输出：`wire-out/refactoring-2026-10-14/`；只保存必要 executable、manifest、日志和原创小 fixture，不复制游戏素材。

已建立 [当前状态](../status.md)、[文档入口](../index.md) 和 [架构边界](../architecture/runtime.md)。历史报告保留原路径和当时结果。

## 阶段进度

| 阶段 | 状态 |
| --- | --- |
| R0 入口/fixture | 文档和 before 已固定，fixture 迁移进行中 |
| R1 GUID 契约试点 | 待开始；先保留两种架构现有错误/输出行为差异 |
| R2 分发收敛 | 在 GUID 试点验收后按领域推进 |
| R3 状态归属 | 待对应领域边界形成 |
| R4 PE32 Process | 待调用边界回归稳定 |
| R5 host/graphics | 待 Process/状态边界形成 |
| R6 原生 Mono | 按用户指示跳过 |
| R7 工具/结构 | 先提取测试 harness，旧入口和脚本兼容保留 |
