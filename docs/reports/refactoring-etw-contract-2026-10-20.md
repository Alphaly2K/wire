# ETW 八项统一契约

状态：接入完成，累计 **733 exports**。直接运行时基线为 `de3d628` SID；独立 provider/ABI/trace 测试先提交于 `29d2a57`。`ETW_CONTRACTS` 集中八项 provider、arity、参数类型与 EtwApi，删除 root 的重复声明和 owner 的名称识别；已绑定 ID 直接调用原 Providers。源码比较核对仅名称/类型替换、删除 is_call 与格式变化，资源字段、验证、copyout、状态码和登记/注销顺序保持。

PE32 REGHANDLE/keyword 使用 Uint64 的两个 DWORD slot，八项 stdcall 清栈分别为 16/8/12/20/20/24/28/20 bytes；仅 advapi32 有原 ABI，保留不可用返回和日志策略。Win64 静态 advapi32/kernel32/kernelbase、fresh-provider 动态不可见与已注册 gateway fallback 保持；BYTE trace、U64 keyword/handle、DWORD count/info/length 独立固定。ETW 仍为禁用 provider、直接返回 status 并保留 LastError，既不跟随 payload/activity/string，也不调用 callback。详见[调用与资源边界](../architecture/tracing.md)。

风险评估为普通声明和 typed route 迁移，没有改动执行器、memory/cache、资源表示或图形/启动路径，因此**不跑性能、采样、游戏或 native 图形矩阵**。release 所属 **7 passed**（Win32 1、AMD64 6），debug AMD64 所属 **6 passed**，fmt、strict all-target/all-feature Clippy 与 release build 通过。token/SID/ETW 三批整合后只做一次 release 全量检查：**664 passed / 7 ignored**，包含 catalog 一致性检查。工具本批未改，上一批工具 31 保留原上下文。

短 ETW fixture 完整覆盖八项导入，返回 `0x3684`、retired 10,356、work 12,033、API 1,677 校验通过；该重放用于正确性，计时不形成性能结论。输入 SHA `a04dd48a1d93b2b4c58f117f6bbe05e86b0ecf1963c8584234a55bbe13323a4c`，构建 executable SHA `158f67978facfa50d8a0cd7db243f561f96dc739081366454384d694a4a33185`，6,075,688 bytes；307 个 src Rust SHA 已记录。证据 `r2-etw-baseline-*`、`r2-etw-validation-*`、`r2-security-integration-release-tests.txt`、`r2-etw-functional64` 和 `r2-etw-verification.json`，位于 `wire-out/refactoring-2026-10-14/`。不额外复制游戏或整套源码。
