# 进程错误模式与启动属性

五项 `PROCESS_OPTION_CONTRACTS` 统一 ProcessOptionApi、ABI 和 provider。Win64 静态及动态均保留 kernel32/kernelbase，PE32 保留 Wine spec 中 kernel32 的 ABI-only 策略，不扩大到 kernelbase。

私有 `x64::runtime::process_options::ProcessOptions` 拥有进程错误模式及以 guest 指针标识的属性列表。处理函数只借 Memory、ID、参数、当前线程 LastError 地址及只读 Pipes/Events；线程创建通过窄查询继承当前进程错误模式，后续线程模式仍由 Threads 独立拥有。

保持错误模式的 0x8007 mask 和未知位拒绝行为。属性列表仍限 1–16 项，初始化保留长度查询、输出清零、最后登记的顺序；更新仅支持完整宽度、已标记可继承的 pipe/event handle list，保留 flags、大小、源读取、句柄验证、列表身份/容量和覆盖检查的次序。删除未知列表保持原诊断。现有 child-process backend 仍未实现，本批不改 CreateProcessW。

五项 arity/DWORD 和 handler 对照 `b6ac06a`，经 ID、owner、窄借用、格式归一及整除写法等价整理保持一致。沿用既有属性列表与真实 APC/线程测试验证资源和错误模式继承，不增加重复测试或性能计时。证据位于 `wire-out/refactoring-2026-10-14/r2-process-options-*`。

相关 release 属性列表 1 项、真实线程/APC 1 项、catalog 一致性、fmt、release lib/bin strict Clippy 和 release build 通过，累计 **841 exports**。源码/executable SHA 记录于验证 JSON，首次编译及 lint 诊断保留。
