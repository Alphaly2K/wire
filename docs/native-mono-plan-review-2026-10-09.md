# Unity 原生 Mono 计划复核（2026-10-09）

复核用户提供的 `wire-out/unity-native-mono-plan-2026-10-09/plan.md` 及同目录的
样本审计、ABI 反汇编、许可/构建文本。七份参考文本的大小和 SHA-256 均与
source-evidence.json 相符；这验证本地证据的一致性，不代表匹配源码已构建或整包许可已验收。
没有在本轮接入或构建宿主 Mono。

## 判断

这条路线值得做，潜在收益来自消除两层执行：让宿主 ARM64 Mono 直接解释 C# IL，
避免先由原始 Mono 产生 x64 方法体、再由 wire 解释这些方法体；VM、GC、元数据
和反射本身也由宿主原生代码运行。UnityPlayer、Burst、Live2D 和 x64 插件仍要另计。
它仍可保留原始 guest 解释器作为默认/未知版本后端，不要求宿主执行 guest x86。

计划正确限定了一个 VM/GC 所有者、启动前选择后端、失败后隔离重启回退，
并把 M1/M2 的无 JIT、共享指针、线程、GC 与回调验证放在大规模导出包装之前。
这个顺序应保留。目前不能从宿主 ScalarIr::step 的占比反推出 Mono 的耗时占比，
290 个 mono_ 符号查询也不是调用次数。2 倍只能作为目标，不能作为预测。

## 实施前需要补成明确验收项

1. **IL 的 unsafe 访存。** 共享 backing 能解决地址身份和别名，但不自动保持 checked memory。
   `ldind/stind`、`cpblk/initblk`、IntPtr 算术及 UnsafeUtility 可能让宿主解释器直接解引用
   原始 guest 整数地址。M2 必须验证 unmapped、NOACCESS、readonly、guard、跨页及释放后
   访问仍受控，明确哪些访问经 Mono 适配检查，哪些对象区域可直接访问。不以同一数组可读写
   就宣布内存闭环完成；也不把任意宿主地址注册成 guest 区域来填补这个缺口。
2. **返回方法入口的 C API。** 把 mono_compile_method、虚调用解析、委托 function pointer
   等返回入口的实际查询单列契约。guest 必须得到可分发的 token，native interpreter 必须
   得到匹配真实签名的预编译入口；参数个数不能替代 Win64/ARM64 类型化 ABI。
   在 M1/M2 验证泛型、异常、ref/out、浮点、struct/sret 和反向委托全路径没有动态 ARM64
   trampoline、closure 或 executable memory 分配，而不只是设置 interpreter 开关。
3. **布局验证范围。** 现有反汇编支持 object header=16、array header=32、length offset=24
   和 unbox 返回内指针；不足以证明 vtable、class、string、bounds 与 Unity 直接读取的所有
   metadata 兼容。M2 加入真实 Unity 的最小布局探测，并给每种长期内指针登记区域与所有者。
4. **线程模型对现有优化的影响。** 当前 Memory/Runtime 使用单宿主线程的 owned 页、Cell、
   Rc 和合作上下文。稳定的原生 Mono TLS 线程必须通过执行令牌/队列交接，不能直接并行
   改写这些状态。GC 暂停、finalizer、锁等待和嵌套回调必须验证不会相互等待。
   当前 IR 的执行页证明、translation epoch、API 后 suspension 检查均依赖此串行假设；
   引入异步写入时须重新设计并重新跑故障/自修改/调度回归。
5. **候选构建准入。** 固定的 Unity fork commit 尚未证明对应 Unity 6000.3.11f1 样本。
   先记录导出/布局/行为匹配证据和真正启用的 ARM64 interpreter＋Unity BDWGC 配置。
   所附 Unity-build.md 给出了 embed runtime 构建路径，但没有证明这个组合已能无 JIT
   闭环。实际 libatomic_ops/类库/第三方选入项仍需按构建清单核对许可。

样本的 mono_unity_install_memory_callbacks 入口在现有反汇编中直接返回。
因此“频繁 Unity allocator 回调抵消收益”在这个 profile 上仍是待测风险；不能从其他
Unity 版本推定它已经发生，也不能据此默认省掉其他版本的回调契约。

## 下一批工作的边界

建议先执行 M0/M1，再把 M2 分成三份可独立复现的原创 fixture：
纯托管 IL＋强制 GC；共享对象/unsafe 故障＋类型化双向回调；两线程/阻塞/GC/异常重入。
每份同时记录正确性、宿主可执行内存使用情况和 crossing 成本。M2 加一个最小真实 Unity
绑定探测，通过后再接 domain/assembly 生命周期和大量 InternalCall。
源码来源选择或实际链接项变化时更新许可清单。

基线归因可与 M0/M1 同步做；不要求先测得很高的 Mono 占比才允许原型。
在原生后端可运行后，用相同 IL checksum 和同一正确游戏场景比较 wall/CPU、首可交互、
帧时间及 GC 停顿。原生化会主动减少 guest 指令，MIPS 不适合对两个后端排名。

本轮通用解释器的当前对照已经更新为 SRW 条件变量错误终点，wall 中位数
147.462→105.639 秒，详见 [性能记录](performance-unity-resource-2026-10-09.md)。
它只说明现有解释器的收益；不能用旧 SRV 错误的 97.417 秒或本轮提前退出时间
与未来原生 Mono 的标题/可玩路径作对照。Logo 淡入淡出已捕获；
加载页首帧后的 SRW API 阻塞仍是独立兼容任务，不能当作纯性能问题。
