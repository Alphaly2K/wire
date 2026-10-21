# AMD64 解释执行优化

最终保留版在约 600M work 的原创混合 SSE/访存负载中达到原始版的 **1.967×**，比中间的精简回滚版再快 **5.24%**。同批访存为 **1.076×**，间接调用为 **0.991×**。这是执行器负载对照，不能换算成游戏 FPS；实际可玩性仍未关闭。

证据根为 `wire-out/executor-opt-2026-10-21/`。基线为 `before/`，SHA-256 `f0b4c6dd5c49e4098a4e5ad3a9528782d26cc700daac97863ef512dc62777374`；最终版为 `after-isolated-mixed/`，SHA-256 `7b15fe12e6cadf4fe672e1e0eda99d335dc6356cc9020f6ca5ab2814792bb2d5`。输入修复已在基线中，最终版另含本轮执行器优化与 D3D11 getter。

## 保留实现

- checked block 从最多 8 条扩大为 **4–32 条**，可组合整数扩展、移位、乘法、位扫描、向量搬运/逻辑和标量 SSE。原 4–16 条 register block 及其链接保留；checked block 不跨调用或返回衔接。
- 只读访存仍由原 handler 按执行顺序检查完整宽度、权限和 guard，省去重复的页证明；写入继续要求不会替换余下代码。Scalar、vector 和 FP 的目标、FLAGS、MXCSR 与失败规则保留，详见[指令块边界](../architecture/amd64-execution-blocks.md)。
- checked 标量事务省去通用 POP 路径的目的寄存器与 RSP 备份。接纳集合保证可失败读取先于寄存器修改，失败的内存写最多需要恢复 FLAGS。
- 含浮点/向量的混合块按预算前缀一次设置 fallthrough RIP，成功时批量更新 work/retired；失败或写入证明不满足时恢复当前指令 PC，只计入已成功前缀。普通整数块保留逐条提交。接纳的 handler 不读取 RIP/计数，只有末条能分支，且失败 handler 尚未分支，是该优化的必要条件。
- 混合循环显式禁止内联，macOS 上放入独立的代码段，避免其控制流与局部变量扩大标量入口的栈帧、改变标量派发布局。此前编译器将混合循环内联进标量入口；最终二进制保留独立函数，证据为 `mixed-loop-symbols.json`。
- ARM64 上，正常有限且非零的 binary32 加减乘，在 guest 舍入与 host FPCR 满足条件时使用宿主算术，精确保留 precision sticky 与未掩码 #XM。零、非正常值、NaN、无穷、不适用的控制或结果回到 APFloat；只读 host 控制。详见[精确 SSE 边界](../architecture/scalar-simd.md)。
- 修复实际长跑在 **44,455,752,095 work** 遇到的 `OMGetRenderTargets` 未实现错误。返回真实绑定 view，先预检全部输出和引用容量，再按实际返回次数 AddRef；可选 NULL 输出不增引用，错误不部分改写。累计 export 契约仍为 **925**。

## 验证

最终 root release all-targets 为 **731 passed / 0 failed / 9 ignored / 23 suites**，包含既有 Unicorn 参照测试。`wire-simd` 另有 **2 项**独立测试通过，不计入上述 root 数量：10 万组输入 × 三种运算 × 五组 MXCSR 对照 APFloat，以及 host FPCR 控制检查和保持。该库此后未修改，源码 hash 与冻结版本一致。

混合块对照覆盖所有入口和预算、非零起始计数、RIP 相对访存、条件分支、向量和转换的真实源宽度、未掩码 SIMD 异常、只读/no-access/guard、RSP 目标、INC/NEG/SHIFT/ADC、调用/返回/栈故障、代码写入及下一代码页观察点。逐条参照核对完整 CPU、成功前缀与内存效果。getter 单元测试覆盖重复引用、未绑定零值、可选输出及全部输出预检，不依赖真实 GPU。

root release all-targets/all-features strict Clippy、独立 SIMD strict Clippy、fmt、release build 与 diff whitespace 检查通过。最终日志为 `isolated-mixed-{all-targets-tests,clippy,fmt,build}.txt`，独立库日志为 `retained-native-single-tests.txt`、`retained-simd-clippy.txt`。九个 ignored 保持既有设备条件；本轮未重跑全部 native GPU 矩阵或未修改的 Python 工具矩阵。汇总由 `verification-final.json` 记录。

## 最终计时

三版分别为原始基线、`after-compact-rollback/` 和最终隔离版。每个输入对三个标签各执行三次，顺序轮换并反向；utility QoS、稳定 argv/cwd、固定 executable slot，无采样或执行计数探针。计时与编译串行，期间仅暂停本任务的测试进程，结束自动恢复；不干预其它应用。

共 **54 次**均校验 work、retired 和 RAX，相同输入的三个标签 RAX 一致。表为 wall time 中位数，倍率为原始版除以最终版。

| 原创负载 | 工作量约 | 原始秒 | 最终秒 | 倍率 |
| --- | ---: | ---: | ---: | ---: |
| 整数 | 60M | 0.305003 | 0.309431 | 0.986× |
| SSE 寄存器 | 60M | 0.376085 | 0.373172 | 1.008× |
| 位流/访存 | 60M | 0.512636 | 0.482492 | 1.062× |
| 访存 | 600M | 7.469478 | 6.938867 | 1.076× |
| 间接调用 | 600M | 5.715206 | 5.764721 | 0.991× |
| 混合 SSE/访存 | 600M | 17.395141 | 8.842454 | **1.967×** |

相对精简回滚版，最终混合负载为 **1.052×**，五个控制负载为 **0.988–1.018×**。接近 1 的数字不作稳定收益解释。原始轮次、CPU 耗时、峰值 RSS、输入与 binary hash 见 `isolated-mixed-three-way/comparison.json`；保留了混合候选一次 10.208s 的离散轮次，没有裁掉后重算。

混合样本覆盖其加减乘和搬运组合，不代表所有 SIMD、Double、除法或实际帧时间。较短任务曾出现约 0.1 秒级的额外 wall/CPU 差异，因此访存、间接调用和目标混合负载采用 600M 长样本；差异的宿主机制未进一步归因。

## 候选与撤回

全部记录保留，不能把下列候选的结果替代最终结果：

| 候选 | 结果与取舍 | 证据 |
| --- | --- | --- |
| 仅 mixed32 | 混合负载 1.293×，作为早期有效结果 | `mixed-32-validated-comparison/` |
| 跨 checked block、PUSH/POP/CALL/RET 衔接 | 虽通过正确性，但间接调用 0.866×、位流也退化，撤下 | `after-final/`、`final-comparison/` |
| 无跨调用链的 mixed32 + native f32 | 混合短样本 1.733×；间接调用短样本 0.959×，600M 复查 1.051× | `after-retained/`、`retained-comparison/`、`retained-indirect-long/` |
| 精简标量回滚 | 混合长样本 1.913×；独立微小增益不作稳定结论 | `after-compact-rollback/`、`compact-vs-retained/`、`compact-vs-original/`、`compact-long-controls/` |
| 所有 checked block 合并提交 | 相对精简版混合 1.051×，间接调用 0.946×，撤下 | `after-batched-commit/`、`batched-three-way/` |
| 仅混合块合并提交，但允许内联 | 混合 1.075×，间接调用 0.930×、访存 0.962×，撤下 | `after-mixed-commit/`、`mixed-commit-three-way/` |
| 混合块合并提交且隔离代码生成 | 混合额外 1.052×，控制负载约在 2% 内，最终保留 | `after-isolated-mixed/`、`isolated-mixed-three-way/` |

早期一次冻结发生于异步编译尚未完成时，两标签实际都是旧 binary，且与编译重叠；`premature-comparison-invalid.json`、`after-premature-freeze/`、`mixed-32-comparison/` 明确无效，不用于任何结论。回退时还曾残留旧 `longest > 8` 测试断言，失败保存在 `retained-all-targets-tests.txt`；修正后完整回归另存并通过。上述失败证据没有删除。

## 实际运行与 profile

正常入口已关闭高频 watch、输入 trace 和 API inventory，无抓帧、无 wall timeout，沿用同一 runtime/Users、后端与排他锁。下列两次 20 秒采样均发生于绘制开始后，计数窗口包含采样前后时间；LLDB 在主采样期间已 detach。主线程栈占比含等待，不是 CPU/GPU 利用率；各次运行阶段和用户活动不同，不构成游戏性能 AB。

| 实例 | 主线程样本 | 解释器/调度/内存 | 图形 | 已识别的 APFloat 运算调用 | 计数窗口与 Present |
| --- | ---: | ---: | ---: | ---: | --- |
| `interactive-executor-6ftina92`，第一轮版 | 15,059 | 71.09% | 27.02% | 1.03% | 55.17184s，5351 Draw / 411 Present，7.45/s |
| `interactive-executor-lc2kgw54`，精简回滚版 | 15,103 | 73.91% | 23.76% | 1.07% | 64.191325s，3663 Draw / 282 Present，4.39/s |

第二段重叠子项为标量 SSE **6.61%**、管线创建 **6.77%**、fence 等待 **14.96%**。带有 APFloat 类型名的 `wire_simd::Scalar` 包装入口也包含宿主快路径，整条路径 **4.54%** 不能全部算成软件浮点。已识别的 APFloat 运算调用的分类使用反混淆后的所属函数。原始栈、分类脚本、计数和火焰图均保留。首次调试入口因优化后的局部变量不可见失败，之后用静态调试类型字段偏移只读定位，失败日志保留。

第一轮实例在 25.025B 周期观察后结束；精简回滚实例继续至 **48.425B** 周期观察，没有新报告的执行错误，随后为最终构建让位。这两个结果均为本任务主动 SIGTERM，不是 guest 正常退出或完整保存验收，Users 后置大小/hash 在各自 `result.json` 中。

最终隔离版已启动为 `interactive-executor-nscpv87h`（PID 41735），binary SHA 与最终冻结一致，启动 manifest 保留源码与 Users 前置大小/hash，不重置保存文件。该实例的最新只读采样独立存放，实际流畅度仍待运行与人工反馈；旧诊断版 2.03/s 与上述正常运行窗口不能推导受控加速倍数。

最终隔离版的 20 秒采样取得 **15,096 个主线程样本**：解释器/调度/内存 **68.36%**、图形 **29.74%**、窗口轮询 **1.24%**。重叠子项为标量 SSE **5.87%**、可明确归属 APFloat 函数的调用 **1.15%**、包含软件回退与宿主快路径的数学包装入口 **4.87%**、管线创建 **8.52%**、fence 等待 **18.89%**。内联的软件浮点代码可能仍落在包装入口中，1.15% 不能解释为全部软件浮点成本。

最终两份计数快照间隔 **140.202733s**，完成 **14652 Draw / 1127 Present**，约 **8.04 次 Present/s**；这是比 20 秒主采样更宽的窗口。并非固定场景 AB，不能由各次运行的 2.03、4.39、7.45、8.04/s 推导受控收益。当前解释器仍是主要路径，图形管线创建与同步等待也占明显比例，未据此关闭实际可玩性。`readiness-observation.json` 保存最新周期观察及人工反馈仍待确认的状态；窗口继续运行。
