# Token 三项 API 统一契约

状态：接入完成。直接基线为 `310882d` 的 Toolhelp 契约冻结；provider/ABI 独立旧行为和 PE32 fixture 先提交于 `00966ee`。`TOKEN_CONTRACTS` 统一三项 provider、arity、DWORD 和 TokenApi，删除 Win64 重复支持/arity/DWORD/字符串分支及 PE32 两项手写 ABI 登记。累计 **716 exports**；原 token owner 与 SID codec 逐字保持。

Win64 静态导入继续接受 advapi32/kernel32/kernelbase，完整宽度的 process/thread/token handle 和 DWORD access/BOOL/class/length 不变。动态查询在三个 fresh provider 都返回 127，注册 gateway 后由原 fallback 找到，不新增默认动态导出。首轮基线错误地预期 advapi32 默认动态导出，测试失败记录 `r2-token-baseline-x64.txt` 保留；之后按实际旧行为修正测试，运行时没有为此改变语义。OpenThreadToken 保持合法 guest 线程返回 1008、其它为 6，不读取输出地址。PE32 三项 advapi32 均 ABI-only，kernel32 仅两项 Open 的 ABI 已知；其余保留未知 ABI/零清栈。详见[token 边界](../architecture/tokens.md)。

验收选择遵循用户本轮调整：这是声明和普通冷绑定路由迁移，没有改动 CPU、memory/cache、资源表示或图形路径，**不执行性能矩阵、长样本、采样或游戏/native 图形复测**。完成 fmt、strict all-target/all-feature Clippy、release build、release 全量 **657 passed / 7 ignored**、debug 定向 **7 passed**、工具 **31 passed**。上一批完整 debug 501 和 native 7 保留其原验收上下文，不记成本批新执行。

原创 PE32 query 循环先在旧冻结校验单次/12K 的 raw/IR，n=max(1,work/9)，EAX=0、retired=9n+7、API=n。迁移后十次调用的前后 raw/IR 四份完整日志均为 1,134 bytes，SHA256 `02450f090bdcb3c735951590e0ee7b108c708feba27faf2d3e2c63fc49e2352a`，逐字节一致且每份包含十条 UNIMPLEMENTED。Win64 原 12K 查询 fixture 的前后结果、retired/work/API count 校验通过。这六次小型重放用于正确性和 trace 检查，不用于性能结论。

冻结 `r2-token-wire/wire` SHA256 `7ec1562e4fc9bdbb9ef538c76273083d9b1841654eee8234f37c8e7398a46ead`，6,032,872 bytes，较前批 −10,960 bytes；304 个 src Rust 文件与 target/frozen/manifest 一致。证明 `r2-token-verification.json`，检查日志 `r2-token-validation-*`，功能证据 `r2-token-functional64` 与 `r2-token-trace32`，均位于 `wire-out/refactoring-2026-10-14/`。没有改动资源限额或复制游戏资产。
