# 原生 Mono 跨架构原型（2026-10-09）

## 已通过的部分

`python3 tools/mono/probe_m2.py` 构建独立 Rust probe，直接使用当前 wire 的
`Pe64`、`Cpu64` 和 checked `Memory`，不修改 CPU、SSE、共享 Cargo 配置或游戏。
原创 PE32+ 的 Win64 调用进入数据 gateway；宿主 ARM64 Mono 执行
`GuestBridge.RoundTrip(Int32)` 的 IL，InternalCall 再进入原创 x64 callback。
回调由现有解释器执行 `mov/add/ret`，最终 checksum 为 19。
没有在宿主上执行 PE 指令，也没有运行时生成 ARM64 callback。

guest 回调只在持有当前 host thread 的执行 token 时允许，FFI 入口捕获 Rust
失败并停止进程，禁止 unwind 穿过 Mono。外层 CPU/Memory 的可变借用在进入
Mono 前结束，避免嵌套 callback 制造 Rust 别名。此 probe 仅覆盖 Int32 typed
gateway；M1 的浮点、结构体、ref/out 测试是 host↔host ABI 验证，不能据此
声称 Win64↔ARM64 的全部签名已完成。

第二组测试把托管数组的引用保存在 guest register 和 checked guest stack，
将相应字快照到专用 BDWGC root 区间。释放 strong handle 后，数组经强制 GC
仍存活；清除 guest 引用、清除快照并移除 root 后，weak handle 变为空。
这验证保活和释放两方向，未把整个托管堆登记为 root。

创建、存活检查、root 移除和回收检查各使用已结束的独立 native thread，
避免原始对象地址留在宿主保守扫描的栈/寄存器里制造假阳性。
Guest 的 owned Memory 在各线程间顺序移交，不开放并发写入。
这些线程和 fixture 的调用都处于独立 noexec 保护层下；本轮日志为
`checks=50 denied=0`，进程正常退出。

工具位于 `tools/mono/m2/`，使用独立 `wire-out/native-mono-m2/cargo`。
release 构建和 Clippy `-D warnings` 通过。可复核输出：
`wire-out/native-mono-m2/results.json`、`run-guest-bridge.log`、原创
`GuestBridge.exe` 和 `GuestBridge.dll`。这些测试二进制不提交仓库。

## 未达到 M2 出口的部分

- 数组只用于 GC 引用生命周期验证，未交给 guest 直接解引用；这不是共享数组读写闭环。
- 目前 Memory 的页仍为离散 owned Box。缺少受控 host backing、权限和长期内指针的映射。
- Mono 的 unchecked indirect/block/field/intrinsic 路径仍需适配；当前测试没有把 guest 地址传入 unsafe IL。
- 调度仅证明单 owner 的重入和顺序移交，未覆盖真实 Unity 的并行阻塞、finalizer→guest 回调或异步队列。
- 真实 Unity binding、Mono C API provider/loader、版本准入和隔离重启回退尚未接入。

因此 M2 仍在实施中，原生游戏后端保持关闭。下一步在独立 checkout 中完成
共享 backing 与权限测试，避免和另一位 agent 的 Memory/SSE 修改相撞。
