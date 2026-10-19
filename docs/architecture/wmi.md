# PE32 WMI COM 资源边界

`wmi::Wmi` 私有拥有对象种类、引用计数、枚举游标和每个接口的 vtable。创建对象显式借 GuestMem、ApiTable、GuestHeap 和 process heap ID；`wmi/calls.rs` 的普通调用另借同一 Bstrings32 owner，无法访问 ApiState 的其它资源。调用输入由 Request 表达 DLL、方法名和固定参数，保持原 provider 准入。

兼容 dispatch 直接借 Wmi 字段，不再将整个 registry 临时 take 出来。调用不执行 guest callback；对象创建、输出清零、引用增减、游标推进、BSTR/VARIANT 写入及失败后的部分提交保留原先顺序。四套 method table 继续同时用于 vtable 和 stack_bytes，未知方法及不可用 provider 的 HRESULT/trace 保持。

能力仍是本地 locator、root/cimv2 和原 Win32_OperatingSystem 查询；未扩展宿主信息、远程 WMI 或接口集合。内部 COM 方法不计作新增 DLL export。

基线 `8aa4165`。原调用主体、创建/分配、方法表、IID 与 provider 准入归一核对保持；release Win32 com 过滤 30、完整 Win32 255、fmt/release lib/bin strict Clippy/build 通过。普通资源借用调整不跑性能或游戏矩阵。证据：`wire-out/refactoring-2026-10-14/r3-wmi32-{source-checks,verification}.json`。
