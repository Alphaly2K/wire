# Wasm / JSC 可选加速验证（2026-10-05）

用户已明确允许 Wasm 经 JSC 使用系统 JIT。约束是 wire 不自建 x86→ARM64 JIT、不生成宿主 callback
trampoline；平台引擎的 Wasm 编译属于允许的加速路线。架构保留：

`x86 → Semantic IR → 保守 FLAGS 优化 → {Execution IR解释器，Wasm→JSC}`。

**解释执行是主路径，Wasm只在可用时加速。** wire主程序仍是解释器，当前Wasm代码是独立可行性实验；
不会用实验循环的高MIPS代替优化真实应用的解释执行吞吐。

Wasm lowering 直接表达整数运算、load/store和控制流。避免在 Wasm 内再写一个逐条解释 Execution IR 的循环，
否则只是把同一套分发搬进另一个执行器。解释器仍覆盖未知入口、自修改和未提升语义。

## 两种宿主方案需分别验证

| 宿主 | 执行/内存特点 | 下一步验证 |
| --- | --- | --- |
| 进程内 JavaScriptCore.framework / JSContext | 公共C API可调用JS和读取TypedArray临时指针；native callback无需WebView消息IPC | 目标App的JIT实际可用性，Wasm↔native同步调用成本，linear memory与GuestMem映射 |
| WKWebView的WebKit/JSC | Wasm由WebKit执行；宿主bridge可能跨进程，不具备同一套进程内C API内存访问条件 | 相同guest逻辑的IPC/复制/批量调用成本、回调与线程调度、目标iOS配置 |

不能把“系统引擎拥有JIT实现”推导成“任意iOS进程内JSContext必然启用JIT”。
[Apple allow-jit entitlement文档](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.cs.allow-jit)
列出macOS，并指出没有该权限时，依赖JIT的framework可能退回解释器。
[公开WebKit初始化逻辑](https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/InitializeThreading.cpp)
与[执行选项](https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/Options.cpp)
也会根据实际JIT能力关闭相关执行层；Wasm API可用、能执行模块并不等于已证明JIT在运行。
这些是平台可用性验收项，不是将JSC路线排除的理由。

## 已验证：本机macOS进程内执行与native内存写入

[examples/jsc_wasm_probe.c](../examples/jsc_wasm_probe.c)是独立、自己编写的公开API探针，
没有接入wire主程序或借用第三方Wasm翻译实现：

```sh
xcrun clang -Wall -Wextra -Werror examples/jsc_wasm_probe.c -framework JavaScriptCore -o /tmp/wire-jsc-probe
/tmp/wire-jsc-probe
```

2026-10-05本机结果：

```text
Wasm add(19,23): 42
Native write -> Wasm i32.load: 123456
```

流程：创建Wasm instance和其memory的Uint8Array view；通过`JSObjectGetTypedArrayBytesPtr`
取得临时指针，native同步写入4B，再让Wasm的i32.load读取。
另行验证直接`JSObjectGetArrayBufferBytesPtr(memory.buffer)`会抛出
`TypeError: Cannot get the backing buffer for a WebAssembly.Memory`；不能用这个结果否定TypedArray路径。

探针只证明本机进程内方案有一次不经额外JS复制的native→Wasm写入路径。
后续运行时实验见下文；仍未证明目标iOS的JIT、整个4GiB guest VA映射、长期指针或异步I/O安全。

## 已实现：现有IR直接lower一个寄存器自然循环

`src/ir/wasm.rs`的`Image::lower_wasm_loop()`从现有IR生成实际Wasm操作和structured loop，
不是在Wasm内解释IR。只接受连续区域、一个尾部条件回边：NOP、MOV32与11类I32算术
（ADD/ADC/SBB/OR/AND/SUB/XOR/CMP/TEST/INC/DEC），可有只执行一次的前缀。
拒绝memory、窄算术、额外分支、call与未知语义；可将最终INT3留作不执行的fallback边界。
必须从首个guest PC进入，中途停止后尚不能从区域中间恢复。

GPR缓存在Wasm i32 locals，每条原始guest指令前检查预算，出口准确保存GPR/FLAGS/PC/退休数。
本轮完整计算六个算术FLAGS，包括INC/DEC保留CF；尚未将静态死标志掩码用于Wasm，优先保持恢复正确。
模块memory min1/max16个64KiB页，0处是512B的64×u64 bank，EFLAGS8、EIP9、本次退休数10；
其余槽保持原值。它不是完整CPU context或4GiB GuestMem，FP80/XMM未lower。
DONE=0、BUDGET=1、INVALID_ENTRY=2；无宿主指针，custom metadata保存区域PC和源hash。

`examples/wasm_bench.rs`先运行raw与IR得到独立对照，再经公开JSC API执行模块。
ALU样本50例、FLAGS样本86例，检查所有早期预算位置、3组初始值、错误入口和完成状态；
比较全部512B，包括未修改槽，并核对退出原因。FLAGS样本覆盖所有上述算术族。
拒绝不安全区域的Rust回归也通过。解释器连续段优化后，FLAGS的86例再次在允许JIT的配置下通过。

## macOS签名、实际JIT与fast-memory限制

无allow-jit的standalone runner可正确执行Wasm，但本机仅约12–21 guest MIPS，
disassembly诊断为空；不应标成JIT结果。给自己的runner加公开macOS allow-jit entitlement与
hardened-runtime ad-hoc签名后，诊断中出现BBQ/OMG生成的ARM64代码，才确认JIT确实运行。

该配置的**未修改引擎默认fast-memory模式仍有阻塞**：越界`i32.load`探针触发SIGBUS，
fault地址是linear memory首个越界位置，栈在JSC的IPInt memory-bound continuation。
本机诊断性设置`JSC_useWasmFastMemory=0`后，同一模块全部bounds/grow/native检查通过。
这把问题缩小到该runner/系统版本的fast-memory fault处理路径，尚未找到公共API的生产解决方式。
不能静默删除越界测试，不能将该私有诊断选项作为产品运行时依赖，也不能把本机实验当成iOS验收。

复现（独立macOS实验，不改变wire默认后端）：

```sh
# 默认签名配置：功能对照；不据此宣称JIT
cargo run --release --example wasm_bench -- 1000000

# 公开macOS entitlement签名；诊断性禁用fast memory以完成边界验收
WIRE_JSC_ALLOW_JIT=1 JSC_useWasmFastMemory=0 cargo run --release --example wasm_bench -- 2000000 flags
```

工具在`wire-out/jsc/`保存模块、oracle、stdout/stderr与签名plist，明确打印签名及诊断环境。
签名本身不证明JIT；另行用`JSC_dumpWasmDisassembly=1`采集证据，不把大量诊断输出计入性能。
默认fast-memory崩溃与diagnostic-off结果均保留。Apple M4/macOS26.5.1实测：

| 工作负载/配置 | 三次结果 |
| --- | --- |
| ALU循环，允许JIT/fast-memory诊断关闭，6,000,003条guest | 491.60 / 634.18 / 730.37 guest MIPS |
| 11类FLAGS循环，同配置，24,000,003条guest | 331.14 / 344.48 / 279.90 guest MIPS |
| FLAGS样本context+module setup | 5.498 ms，模块3,437B；不含AOT或热身 |
| Wasm→native shim | 128.72 / 119.14 / 114.21 ns/call |
| Wasm→native→Wasm嵌套callback | 238.82 / 231.76 / 292.38 ns/call |
| native fread→Wasm临时借用 | 9753.12 / 12628.26 / 9512.49 MiB/s |

跨边界循环每次250,000调用，校验次数与checksum；IO为已缓存64KiB临时fixture、1024次32KiB读。
这些shim只测调用、临时指针和同步fread，并非完整Win32 ReadFile/CALL_NATIVE ABI，IO不是磁盘或XP3吞吐。
guest MIPS只统计真实x86→IR→Wasm区域，synthetic helper不冒充guest指令。
本机负载/调度波动明显，不将短ALU循环外推为应用性能，也不据此改变解释器主路径优先级。

## 指针与运行时验收

Apple SDK的JSTypedArray.h明确注明：返回指针是临时的，不保证跨JavaScriptCore API调用有效。
短同步写入期间不调用JSC或guest callback；完成后丢弃指针。memory.grow、buffer更换/分离后重建view，重新取得指针。
不能把C API取到的指针永久塞进现有GuestMem，也不能把宿主mmap的ArrayBuffer直接当作可导入的Wasm.Memory。

`examples/jsc_wasm_runner.c`在不借用内存期间才允许nested callback/grow/JSC调用，保护view对象并在grow后重建。
已检查Wasm越界load、native IO越界拒绝、grow分离旧view、旧view IO拒绝、刷新后数据保留及短读/EOF。
默认无JIT与签名后的diagnostic-off配置通过；默认fast-memory阻塞如上。

后续可选加速验收：解决目标平台JIT/引擎fault处理、定义真实Win32 ABI、跨边界FLAGS/FP80恢复，
分别验证进程内JSC与WKWebView。当前先继续优化解释器，不以完整Wasm后端作为其性能改进的前提。
真正进入双后端前要定义唯一内存模型或一致性协议：切回解释器时guest heap、stack、code和CONTEXT必须一致。
多页权限、fault、原始guest PC/退休预算、自修改失效继续共用VMM语义；保守FLAGS规则不因采用Wasm而放宽。
