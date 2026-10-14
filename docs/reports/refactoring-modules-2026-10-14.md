# R3 模块状态归属

基线为 R7 的 `b28c5bb`。本批收拢 PE32 逻辑 provider 与 Win64 module identity/lifetime 状态；模块 API 契约仍属 R2 后续工作，未将状态提取计为 catalog 迁移完成。Mono、DLL/TLS 回调顺序、guest 地址、handle 数值和未知 API 策略保持原行为。

## 实现

PE32 provider 的 name/handle 双向索引与 allocator 从 ApiState 移入已有 Modules owner。原来的三个公开可写字段移除，属于 Rust 源接口变化；注册入口继续委托，读取经 owner 方法完成。仓库中的单个直接字段测试改用独立黄金值 `0x70001000`，没有由实现生成预期值。

Win64 私有 Modules 拥有 provider/guest 名字、引用、pin、exports、path、image metadata、DLL directory 和 allocator。资源与 USER 通过只读 view 查询。加载搜索、guest/provider release 的差异，以及 guest 最后卸载仍拒绝的规则保留；Process 仍安排实际加载和回调。

GetModuleHandleEx 的引用更新使用借用 token：准备时只查一次引用表，guest 输出成功后才提交 ref/pin。输出故障丢弃 token，原状态不变。两架构不统一成一个 handle arena，也不引入总运行时 Context。

## 失败候选与修复

最初候选冻结于 `r3-modules-final-wire`，SHA-256 `4afa90e882d7276c64797d6b382b9ce0576fb4bca4e26747095946cf2c0269a9`。正确性通过，但性能验收拒绝：PE32 SSE IR CPU 中位数 +15.79%，raw +10.48%，AMD64 module gateway +3.81%。全部轮次保留在 `r3-modules-{pe32,amd64,v2}-timing`。

反汇编发现 PE32 的 cached_simd/cache_simd 从原先内联变成独立函数调用；对照采样中的 IR 热点包括 Descriptor::execute_inner、execute_mixed_run 与 steps_cached。添加两个 inline 提示并将 Win64 引用更新改为借用 token 后重新构建和冻结。这些证据支持接受修复后的组合候选；没有将 raw cache 的内联变化当成 IR 回退的唯一原因。

profile 汇总工具补上迁移后的 x86::process/x86::cpu 路径识别，并用独立手写 sample 树验证加载排除、回调分类及 inclusive 去重。采样与正式计时分开。

## 验收记录

修复候选为 `r3-modules-inline-wire/wire`，SHA-256 `016de49b9966ff42808c5378971f13ccc19e86f2c849e70d46c411c71244a8cf`；source/diff/toolchain 见同目录 manifest。

完整 release 400 passed / 0 failed / 7 ignored，debug Win32/AMD64/FPU/Unicorn 285 passed；fmt、strict all-targets/all-features Clippy 和 Python 工具 12 项通过。原始日志为 `r3-modules-inline-{all-tests,debug-tests,clippy}.txt`。

700M work 的 PE32 SSE IR/raw 三对 AB/BA 全部校验 checksum/retired：CPU 中位数相对 R7 为 -13.56%/-2.78%，IR 各对 -13.19%/-13.58%/-13.65%，raw 各对 -3.50%/-0.76%/-1.75%。RSS 中位数增加 160KiB/96KiB。结果见 `r3-modules-inline-sse-timing`，不是完整游戏 FPS 测量。

其余 21 个 case/path 以 100M work、三对 AB/BA 串行执行，加上 SSE 共 23 个 case/path、138 轮，全部校验通过。PE32 其它路径最大 CPU 中位数回退 +1.71%（GUID/raw）；AMD64 最大 +1.44%（module gateway）；V2 最大 +0.89%（code-alias）。整个矩阵 RSS 中位数最大增加 208KiB；模块状态容器的既有容量上限没有增加。binary 为 5,893,080 bytes，相对 R7 增加 26,720 bytes（约 0.46%）。各轮次与输入 hash 见 `r3-modules-inline-{pe32,amd64,v2}-timing`。

修复版符号表 `r3-modules-inline-symbols.txt` 未出现两个 SIMD cache helper 的独立符号。该结果与回归消退一致；未进一步推导唯一因果。

正式计时结束后，外部 native GPU/Metal 7 项显式执行，7 passed；复用此前冻结的 native graphics、MoltenVK、GS/DXBC 输入。Alice 同目录 200M work 的启动 smoke 以预算出口结束：retired 199588616、API calls 390496、隐藏窗口 client 314×211、draw/present 0。该次可核对停点与 R7 相同；不将宿主计时/线程参与的启动路径要求为每次逐指令一致。日志为 `r3-modules-inline-native-tests.txt` 和 `r3-modules-inline-alice-smoke.txt`，没有复制/硬链接游戏。
