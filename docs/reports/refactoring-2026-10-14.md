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
| R0 入口/fixture | 已完成；12 组 golden 输入一致，默认与旧路径各 7 个运行 smoke 通过 |
| R1 GUID 契约试点 | 唯一契约和双架构冷绑定已接入；下一步提取纯语义与架构适配器 |
| R2 分发收敛 | 在 GUID 试点验收后按领域推进 |
| R3 状态归属 | 待对应领域边界形成 |
| R4 PE32 Process | 待调用边界回归稳定 |
| R5 host/graphics | 待 Process/状态边界形成 |
| R6 原生 Mono | 按用户指示跳过 |
| R7 工具/结构 | 先提取测试 harness，旧入口和脚本兼容保留 |

## 测试 harness 提取

PE32 `Vm` 与固定内存布局移入 `tests/support/x86.rs`；PE64 原创 builder、startup 和 Win64 调用 frame helper 移入 `tests/support/x64.rs`。原 integration target 和测试函数名保持不变；全部 389 个测试名逐名一致。Win32/AMD64 239 项行为测试通过，两个 target 的 strict Clippy 和 fmt 通过。没有变更 runtime 代码。

## R0 fixture 验收

原创生成器已进入 `tools/perf/fixtures/amd64_audit.py`，默认 V2 replay 不再需要 ignored audit 输入。旧 `--audit-probes PATH` 保留；两个入口的 7 个 input manifest 完全一致。`test_perf_fixtures.py` 对照迁移前固定的 12K/100M SHA-256 和结果，工具测试 6 项通过。源函数内容保持原样，没有修改任何 runtime 热路径。

## R1 契约与冷绑定

三个试点 API 的 DLL、scalar 参数、返回约定、路由 ID 来自 `win32::catalog`。PE32 仅 ole32，Win64 同时支持 combase，保持原范围。Win64 DWORD 索引由参数类型派生，PE32 stdcall 字节数由参数数派生；Wine spec 继续作为 legacy ABI 来源。动态 GetProcAddress 使用相同契约，kernel 模块不会错误导出试点。

PE32 公开 `ApiEntry`/`entries` 和 stub 地址保持兼容，内部 Arc 快照保存绑定信息。公开条目变化时重新绑定，未变调用不解析 catalog 或 Wine spec；handler 重入不会借用可变 API 表中的 entry。Win64 保留 HotImport，只给试点增加 typed ID。

独立 GUID 回归从接入前的 5 项扩展到 6 项，加上原 Win32/AMD64，共 245 项通过。覆盖 wrong DLL、signed capacity、高 DWORD 截断、HRESULT/访存差异、output alias return slot，以及公开条目/gateway 替换后的重绑定。全 target strict Clippy 通过。一次沙箱内 AMD64 测试因 Unicorn SIGILL 终止；在允许参照执行的环境重新运行，130 项全部通过，保留最终日志。

增加原创 `gateway-guid` fixture，expected 指令数/工作量/输出不依赖 catalog。绑定阶段的三对串行 AB/BA 使用 210M work；thread-id CPU 中位数 +2.65%，GUID +2.67%，RSS 略降。小样本和扩大样本均保留于 `guid-binding-timing*`；阶段结束还需完整矩阵，不据此宣称性能改善。固定的 `guid-bound-wire` 供后续定位代码布局影响。
