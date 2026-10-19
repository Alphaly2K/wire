# Win64 ETW provider 所有权

`x64::runtime::tracing::Providers` 私有拥有进程的 provider 注册表与下一个句柄。Runtime64 只组合这个 owner、读取既有 ABI 参数并提交返回帧/诊断；ETW 处理函数只借用 `Memory`、绑定的 EtwApi 和参数，不再通过 `impl Runtime64` 访问总状态。八项 provider/arity/参数类型集中于 `ETW_CONTRACTS`，删除 legacy 重复表和逐调用字符串分发。

当前能力仍是没有 tracing session 的禁用 provider。注册时保存 GUID 与原 guest callback/context 数值，既不调用 callback，也不提交事件或读取事件 payload。最多 64 个同时注册的 provider，句柄从 `0x7fff6000` 单调分配到 `0x7fff6fff`，注销不复用旧值；累计耗尽仍返回状态 8。

EventRegister 保留 source 16 bytes、output 8 bytes 的检查顺序，随后才判断容量，再读 GUID、写出句柄并登记资源。source guard 优先消耗；output fault 不消耗句柄。输出可以覆盖 guest 返回地址，gateway 继续使用提前保存的返回帧。所有 ETW 状态码直接返回，LastError 保持；访存故障不提交 CPU 返回和 API count。

无效句柄的 EventEnabled/EventProviderEnabled 返回 0，其它操作返回 6，并跳过后续指针。有效 EventEnabled 检查 16-byte descriptor；EventWrite/Transfer 先检查 descriptor，再拒绝超过 128 个数据条目，非零 count 只检查 16-byte 数组元素的可读范围，不跟随其中的 payload 地址。Transfer 保留忽略 activity/related 指针的既有行为。EventWriteString 不读取禁用事件的文本；EventSetInformation 保留状态 50。注销后原句柄立即失效。

独立旧行为测试位于 `tests/x64/tracing_owner.rs`，先提交再迁移。正式原创 `gateway-etw` 在循环前仅注册一次，六项调用循环后注销并消费退休句柄状态；没有反复注册导致累计句柄耗尽。n=max(1,work/43)，checksum=50n+6、retired=37n+33、work=43n+36、API=6n+3。OFT 与 IAT 独立，离线 PE inventory 要求恰好八个 advapi32 导入；两个小规模输入已在旧冻结程序上校验。

PE32 在 advapi32 保持 ABI-only，其它 provider 保持未知 ABI/零清栈。REGHANDLE 和 keyword 使用 Uint64，在 PE32 占两个 DWORD slot、Win64 占一个参数位置。BYTE level 的原始 trace carrier 不截断，DWORD count/info/length 的截断规则不变。Win64 静态导入仍接受 advapi32/kernel32/kernelbase；动态查询默认不可见，原已注册 gateway fallback 保持。

八项统一契约已完成所属行为/ABI 回归和三批整合的 release 全量检查，按低风险评估不重跑性能或游戏/native 图形，见[契约验收](../reports/refactoring-etw-contract-2026-10-20.md)。原 owner 的性能验收记录继续见[历史报告](../reports/refactoring-etw-owner-2026-10-19.md)。
