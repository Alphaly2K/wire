# Wasm / JSC 加速路线与首个验证（2026-10-05）

用户已明确允许 Wasm 经 JSC 使用系统 JIT。约束是 wire 不自建 x86→ARM64 JIT、不生成宿主 callback
trampoline；平台引擎的 Wasm 编译属于允许的加速路线。架构保留：

`x86 → Semantic IR → 保守 FLAGS 优化 → {Execution IR解释器，Wasm→JSC}`。

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

这证明本机进程内方案有一次不经额外JS复制的native→Wasm写入路径。
**没有证明**其吞吐、目标iOS的JIT、整个4GiB guest VA映射、长期指针有效性、memory.grow、回调重入或异步I/O安全。
探针只是功能验证，不报告guest MIPS，也不把它标为已完成Wasm backend。

## 指针与运行时验收

Apple SDK的JSTypedArray.h明确注明：返回指针是临时的，不保证跨JavaScriptCore API调用有效。
短同步写入期间不调用JSC或guest callback；完成后丢弃指针。memory.grow、buffer更换/分离后重建view，重新取得指针。
不能把C API取到的指针永久塞进现有GuestMem，也不能把宿主mmap的ArrayBuffer直接当作可导入的Wasm.Memory。

下一项实验测**运行时**：Wasm内长循环、预编译native shim回调、同步ReadFile写linear memory、
跨边界恢复FLAGS/FP80、每秒调用数与字节数。分别测进程内JSC和WKWebView，并单列引擎启动/模块编译/热身时间。
真正进入双后端前要定义唯一内存模型或一致性协议：切回解释器时guest heap、stack、code和CONTEXT必须一致。
多页权限、fault、原始guest PC/退休预算、自修改失效继续共用VMM语义；保守FLAGS规则不因采用Wasm而放宽。
