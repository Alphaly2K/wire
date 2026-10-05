# 对 Project design.md 的评审与近期执行顺序（2026-10-05）

本文件评审用户提供的 [Project design.md](Project%20design.md)，将其中内容视为方案建议，
并不把它自动当成启用 Wasm、移植整个 Wine 或扩展 x86-64 的任务授权。
当前明确约束仍为 PE32、无自身 JIT、提高解释执行吞吐、极度保守的静态 FLAGS 消除。
具体编码与状态模型见 [IR AOT 设计](ir-aot-design.md)。

## 适合采用的部分

- 分离 Semantic IR 与 Execution IR：安装期尽量充分分析，热路径只读紧凑且类型化的执行指令。
- 以总 dispatch / guest 为核心指标，用 handler specialization 和有限融合控制指令膨胀。
- 原 x86 解释器永久兜底未知/重叠入口、自修改代码、未支持提升的指令，兼容性不依赖加速路径。
- FLAGS 逐位建模、先块内证明、保留精确 CONTEXT 与异常恢复能力。
- 显式 GuestPtr/GuestHandle/Callback gateway，禁止宿主裸指针混入 guest 状态，不生成 callback trampoline。
- 统一 VMM 与代码失效；从第一版就区分 guest MIPS、IR MIPS、分发数和 API/内存开销。
- Wasm 先验证内存与跨边界成本；native library replacement 必须可退回 guest 实现。

## 需要修正或补充的部分

1. **执行路线与范围。** 当前先收敛 PE32 interpreter，x86-64、完整 Wine 移植、D3D10/11 和双后端调度
   是后续产品范围。十八个阶段不能都变成当前性能优化的前置任务。现有 loader、TEB、堆、SEH 等已有实现，
   应直接列缺口，避免从 Phase 1 重新建设。Wine 的每个拟移植文件仍要审核许可、依赖和宿主适配，不能当普通库直接链接。

2. **Wasm 与 no-JIT。** 文档的 WebKit/JSC 路线最终使用 JIT。如果约束是全链路禁止 JIT，则这条加速路线不成立；
   若只是 wire 进程不能生成宿主机器码，WKWebView 的系统引擎可以作为独立可行性实验。
   不能假定原生 App 具有可任意使用的 JSC JIT entitlement，也不能把内部 WebKit 接口当公开可用 API。
   WKWebView 往往跨进程；JS typed-array 可访问 linear memory，不代表 native Rust 能拿到可长期共享的裸指针。
   在公共 API 下完成真实 memory/callback/调度测试前，不把“near zero-copy”列成架构事实。

3. **FLAGS 与融合。** 暂不自动升级为跨块/跨函数消除。所有未知出口、调用、faultable 访问和恢复边界全 live。
   CMP_BRANCH 等也须保留两条 guest 的退休计数、预算/单步边界和分支后的完整可恢复 FLAGS。
   当某一位不能证明死亡时保留，不能仅按 Jcc 的 condition mask 删除其他标志。

4. **浮点与寄存器。** 加入 x87 FP80、TOP/tag/CW/SW、异常、XMM/MXCSR 的完整状态模型。
   512B 的64个64位槽不是完整 CPU context；vreg16–31 建议组成八组 FP80 双槽，不直接缩成 F64。
   PC=53 也不等于 F64，因为指数范围和舍入/异常行为仍不同。子寄存器与高字节写回需显式语义。

5. **Threaded interpreter。** 稳定 Rust 没有 C computed-goto，普通函数指针分发不保证 tail call，
   不能据“threaded”这个名字预期更快。先测试 switch、函数指针、预编译 C/汇编 dispatcher 的真实分发成本，
   再决定是否引入额外语言/平台代码；持久 IR 只能保存可移植 opcode，宿主 handler 地址最多在装载后建立。
   superinstructions、sequential IR PC 和 block chaining 更值得优先测量。

6. **VMM 先于 unchecked fast memory。** “已知堆/栈”仍可能被 VirtualProtect、decommit、guard、另一线程或 API 改变。
   省检查要证明访问范围并绑定映射/权限代数；地址加法越界、跨页和4KiB guest/16KiB host页的差异不能忽略。
   当前 GuestMem 只有范围/commit检查，完整保护语义与CPU fault→SEH仍是缺口。

7. **Native replacement 不只是名称和 hash 匹配。** 还需要调用约定、数据布局、对象身份、allocator/free 配对、
   callback、TLS、LastError、longjmp/异常、版本可见行为。静态链接/内联的 codec 也不能只凭 DLL 名识别。
   先把 CALL_NATIVE 的受控 Win32 gateway 完善，再选择一个实际热点验证 replacement，避免先做通用替换框架。
   GPL/LGPL codec 配置与分发方式另需文件/构建级许可核实。

8. **性能数值只是目标。** 100–200 MIPS sustained、200–300 MIPS microbenchmark 需明确硬件、guest混合、
   功能覆盖、trace/stats、API日志、AOT/加载是否计时。主指标为 retired original x86 instructions；
   fallback、融合和 REP 都要有一致计数口径。增加 IR 总字节、resident cache、代码失效率、native调用与恢复次数，
   并用较长运行和重复测量避免热身/短样本噪声。

## 近期执行顺序

| 顺序 | 交付 | 验收依据 |
| --- | --- | --- |
| 1 | 原解释器/64B预解码/8B typed热路径的可重复对照 | 差分和边界回归；关闭观测开销的 runtime 数据；完整记录 total IR/cache footprint |
| 2 | 扩大热指令覆盖：typed load/store、寻址融合、顺序 IR PC | ALU/branch/memory三类测试；不能增加过多dispatch或减少必要访问检查 |
| 3 | guest边界/恢复表、有限 CMP/TEST fusion、block chaining | 每个预算/故障/单步/回调位置恢复一致；FLAGS只按静态证明消除 |
| 4 | 紧凑持久格式、512B bank、稀疏 guest-PC 索引 | 文件与resident尺寸下降；正确性和runtime吞吐不退化 |
| 5 | 根据真实目标补齐 RtlUnwind、窗口/消息循环及 GDI | 原先21M指令处的功能阻塞被解除，获得更长且可代表应用的性能区段 |
| 6 | VMM权限/映射、gateway ABI、热点 native replacement | Win32行为/异常/线程/回调回归与可量化端到端收益 |
| 后续独立实验 | 公共 WebKit API 下的 Wasm feasibility | 明确 no-JIT 的产品边界；测真实跨进程内存、调用和状态恢复成本 |

本轮已完成第1项的过渡原型、I32 typed热路径子集和 CALL_NATIVE 的受控分发/IAT guard。
64B文件仍偏大，尚未完成最终 Execution IR，也未启动 Wasm/Wine整体移植；不能将当前原型记为上述全部阶段完成。
