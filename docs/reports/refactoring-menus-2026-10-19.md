# 菜单资源与网关优先级基线

状态：菜单运行时未迁移，四项独立旧行为基线通过。上一候选的性能计时已经停止，才编辑/编译这些测试；测试不属于此前 395 个 Rust 文件冻结。

范围为 CreateMenu/CreatePopupMenu、销毁/查询/窗口挂接、MENUITEMINFO A/W、递归命令查找、删除/移除和状态位，以及原不可用的绘制/跟踪和加载入口。Win64 保持 absent。`Menus` 已私有拥有对象、项和句柄，下一步应将 typed ID 和局部窗口/LastError 借用接到该 owner，减少 handler 对整个 ApiState 的依赖。

三项 PE32 基线固定：输入结构验证先于菜单句柄；GetMenuItemInfo 的文本和结构两个输出均预检后才写入；共享 submenu 销毁会断开所有存活菜单引用，但不清除数值恰好相同的 child control ID；顶层窗口的实际菜单挂接仍按资源生命周期清除。现有递归查找、字符串复制、深度/循环拒绝及修改回滚测试继续保留。

RedrawWindow 存在原始入口差异：API fallback 先由 menu handler 返回 0/error 120，trace 带 `[UNIMPLEMENTED] ; menu/USER32 error 120`，有效 INVALIDATE/VALIDATE flags 也不会更改 update region；直接调用公开 USER facade 则到 paint handler 并更新矩形。测试分别验证两个入口和 trace。后续契约必须显式保留真实 gateway 优先级，修复旧遮盖另作行为提交，不以 typed 迁移代替功能修正。

一项 Win64 基线固定 27 个菜单/跟踪/加载名称在 user32/kernel32/kernelbase 都 absent。证据 `wire-out/refactoring-2026-10-14/r2-menu-baseline-tests.txt`：PE32 3、Win64 1，全部通过；构建后的运行时应与 scalar-isolation 候选保持相同 SHA。未注册下一批菜单契约，没有增加已验收 export 数量。

迁移前追加七个 unavailable 入口的统一行为基线：DrawMenuBar、TrackPopupMenu/Ex、RedrawWindow 保留 menu/USER32 error 120 note，LoadMenuA/W 和 GetSystemMenu 保留空 note；均返回 0、error 120、unimplemented trace，并逐项检查原 stdcall 清栈与返回 EIP。菜单四项 PE32 基线全部通过，日志 `r2-menu-unavailable-baseline.txt`。先固定这些入口差异，再迁移 typed owner。
