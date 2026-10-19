# 注册表统一契约

状态：完成，累计 **750 exports**。独立基线 `c872fbb` 固定两架构 provider、ABI、能力和 trace 差异后，17 项 export 汇入 RegistryApi；移除 Win64 重复的 arity/DWORD/provider/group 规则和名称分发、PE32 九项手写 ABI 及三组根分支。PE32 受限实现移入无状态适配器，Win64 owner、资源和访存顺序保持。

Win64 原 15 项静态 provider 范围不变，advapi32 动态直接导出、kernel32/kernelbase 依赖已注册 gateway；旧式 QueryValueA/W 仍 absent。PE32 的 advapi32 仅 open/query/close 具有受限行为，create/set/delete ABI-only；kernel32 私有 ABI 沿用 Wine spec fallback，kernelbase/user32 未知。指针/handle 全宽、原 DWORD 位点和输出/LastError 行为保持。细节见[领域边界](../architecture/registry.md)。

新增基线初版错误地把 PE32 host 状态赋值当作 TEB LastError，并沿用了 ETW 的动态 provider 预期；在旧生产实现上纠正测试并保留失败日志，没有改生产行为迎合预期。最终基线与迁移后相关 release 均 **6 passed**（PE32 2、Win64 4），catalog 一致性、fmt、strict all-target/all-feature Clippy、release build 通过。Win64 260 项整体验证沿用前一所有权批次，不标记为本批重跑。

风险评估为契约与静态 ID 路由迁移，未改动执行器、内存/cache、资源表示、调度或图形，因此不做性能、游戏或 native 矩阵。证据 `wire-out/refactoring-2026-10-14/r2-registry-*`，verification JSON 记录源码和 executable SHA；不额外复制输入或程序。
