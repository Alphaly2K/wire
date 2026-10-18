# AMD64 checked operand 调用边界（2026-10-18）

窗口属性与查询集成候选的 51 条固定路径矩阵全部校验，但 memory CPU 中位数 +4.212%。2.1B work、三标签三对补证为 after/before +5.114%、control/before +1.164%、after/control +3.905%；三对 after/before +3.440%/+5.249%/+6.630%。因此暂缓集成验收，保留 `r2-user-queries-integrated-fixed-memory-control-summary.json`，没有用其它路径改善抵消成本。

相同 executable slot 的四阶段 700M 对照全部校验：Classes / properties / trace-final / queries CPU 中位数 5.672043/5.789324/5.786266/5.960548 秒，相对 Classes +2.068%/+2.014%/+5.086%。首次通用 summarize 调用缺少自定义 after 标签，结束时报错；全部实跑已完成，另以明确四标签独立汇总到 `r2-user-queries-memory-components-fixed-summary.json`，未重跑或删除轮次。

该 fixture 不启用 startup，实际经过 CLI 的执行循环。最初调查的 Process64 caller 不是此输入的入口；其反汇编保留在 `r2-user-queries-integrated-memory-codegen/`，不据此定位 memory 成本。修正后的串行三轮 sample 与反汇编分别在 `r2-user-queries-memory-profile-corrected/` 和 `r2-user-queries-memory-corrected-codegen/`。采样结果均校验相同退休数/work/checksum，未混入正式计时。

实际 caller 和 CheckedSegment 指令数保持；`ScalarIr::checked_access` 从 65 增至 94 条指令、16B 增至 48B 栈帧。原先 tail-call 的 `Memory::owned_noncode_access` 被内联到该函数，引入保存寄存器和额外栈操作。CPU/memory 源码本身在旧集成之间相同，跨模块编译器决策仍会改变机器码；此前只比较部分 helper 助记符没有覆盖此变化。

修订只给 `owned_noncode_access` 显式 `inline(never)`，保持页界限、generation 和非投机权限/guard 处理。`checked_access` 恢复 65 条指令和 16B frame。完整 release 检查通过，包含六项尚未迁移的窗口文本/焦点基线。新冻结 `r2-user-queries-memory-boundary-wire/wire`：SHA256 `04b46d3234302dfdb5d28b3a277a76f228ef6edbe1de1671269cfccf7c83cbe4`，5,970,696 bytes，较上一集成 +32 bytes。

700M 三方三对早期对照全部校验，修订 after/before CPU +1.168%；原集成作为 old 标签保留。结果见 `r2-user-queries-memory-boundary-early-summary.json`。后续完整检查、固定路径矩阵、长 memory control 和 Alice 前缀尚待完成，当前不计入新增验收数量。
