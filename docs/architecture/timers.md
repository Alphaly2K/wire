# Waitable timer 调用边界

九项 waitable timer 的 provider、签名、DWORD/指针宽度和 TimerApi ID 集中在 `win32/catalog/timers.rs`。PE32 全部保持 ABI-only error 120 与原 stdcall 清栈，不新增 timer 实现。Win64 接受 kernel32/kernelbase，普通 scalar 冷绑定直接调用 typed timer adapter，删除独立 arity/DWORD 表和 legacy group 15 路由。

原 timer 状态仍由 Win64 Events owner 管理，保留与 event/semaphore 共用的命名身份、per-alias access、manual/auto 信号、重复对象 wait-all 与取消后的信号行为。创建仍拒绝 SECURITY_ATTRIBUTES；扩展 create 保留 flags/access 的验证与原权限。A/W 名称及 create/open/extended 来自 ID，调用阶段不再解析名称。

timer handler 也已归 Events，只借 Memory 与只读 Threads；Runtime64 仅组合资源，错误分支才通过 Threads 查询当前 TEB。相对/绝对时钟取样仍在原位置，未引入统一时钟或等待抽象。

Set/SetEx 先受检读取 64 位 due time，再检查负 period、APC/completion 与 resume/reason context，最后检查 handle/access 并设置单调 deadline；原相对/绝对时间换算和 checked deadline 加法保持。SetEx 的 reason context 保持完整指针，普通 Set 的 resume 保持 DWORD；未新增回调或唤醒系统的能力。

Create 按是否复用对象写 LastError 0/183，其它成功保持旧错误；失败设置原错误码。公共 gateway 先受检保存返回 frame，再执行资源变更，最后提交 CPU、trace 与 API count。故障与不支持参数的原优先级不变。独立 ABI、A/W/access、due-time fault 以及原创取消循环见[验收记录](../reports/refactoring-timers-2026-10-16.md)。
