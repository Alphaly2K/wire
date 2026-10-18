# 菜单资源与网关优先级基线

状态：27 项菜单契约与 typed owner 已接入，完整正确性通过，性能尚未验收。原四项独立旧行为基线先于迁移通过；测试不属于此前 395 个 Rust 文件冻结。

范围为 CreateMenu/CreatePopupMenu、销毁/查询/窗口挂接、MENUITEMINFO A/W、递归命令查找、删除/移除和状态位，以及原不可用的绘制/跟踪和加载入口。Win64 保持 absent。`Menus` 已私有拥有对象、项和句柄，下一步应将 typed ID 和局部窗口/LastError 借用接到该 owner，减少 handler 对整个 ApiState 的依赖。

三项 PE32 基线固定：输入结构验证先于菜单句柄；GetMenuItemInfo 的文本和结构两个输出均预检后才写入；共享 submenu 销毁会断开所有存活菜单引用，但不清除数值恰好相同的 child control ID；顶层窗口的实际菜单挂接仍按资源生命周期清除。现有递归查找、字符串复制、深度/循环拒绝及修改回滚测试继续保留。

RedrawWindow 存在原始入口差异：API fallback 先由 menu handler 返回 0/error 120，trace 带 `[UNIMPLEMENTED] ; menu/USER32 error 120`，有效 INVALIDATE/VALIDATE flags 也不会更改 update region；直接调用公开 USER facade 则到 paint handler 并更新矩形。测试分别验证两个入口和 trace。后续契约必须显式保留真实 gateway 优先级，修复旧遮盖另作行为提交，不以 typed 迁移代替功能修正。

一项 Win64 基线固定 27 个菜单/跟踪/加载名称在 user32/kernel32/kernelbase 都 absent。证据 `wire-out/refactoring-2026-10-14/r2-menu-baseline-tests.txt`：PE32 3、Win64 1，全部通过；该基线阶段的运行时 SHA 与 scalar-isolation 候选相同，记录在 `r2-menu-baseline-runtime-identity.json`。当时尚未注册菜单契约。

迁移前追加七个 unavailable 入口的统一行为基线：DrawMenuBar、TrackPopupMenu/Ex、RedrawWindow 保留 menu/USER32 error 120 note，LoadMenuA/W 和 GetSystemMenu 保留空 note；均返回 0、error 120、unimplemented trace，并逐项检查原 stdcall 清栈与返回 EIP。菜单四项 PE32 基线全部通过，日志 `r2-menu-unavailable-baseline.txt`。先固定这些入口差异，再迁移 typed owner。

27 项元数据随后汇入唯一契约，PE32 24 项 typed、3 项 ABI-only，Win64 全 absent；删除原 27 项手写清栈和每调用名称分发。菜单实现移入 win32/user/menus32，原对象/条目 owner 的方法只借用 GuestMem、窗口表和 LastError。根 menu 仅保留原私有路径的 reexport，原 Wine 版权完整保留。架构见[菜单边界](../architecture/menus.md)。

源文本核对确认：菜单图、结构 codec、文本及 item 更新/输出 helper 除明确的 enum 选择与 import 之外保持相同；27 项清栈字节数与旧表逐项相等；scalar 编译隔离的输入完全未变，证据 `r2-menu-semantic-source-comparison.json`。首次编译暴露 Win64 exhaustive match 需要显式 absent 分支，诊断保留在 `r2-menu-typed-targeted.txt`；补齐后定向 release 八项全部通过，见 `r2-menu-typed-targeted-v2.txt`。

独立原创 gateway-menu 在一个真实菜单项上反复置位/清零 checked 位，消费两次旧状态并校验插入/销毁结果；无逐轮资源增长。新 fixture 与菜单候选分别对旧 scalar 冻结和本批冻结核对 raw/IR。两批冻结独立保留；对话框性能准入仍待复核，菜单本批同样不预增已验收计数。

完整 release 619 passed / 7 ignored、debug 465、显式 native 7、工具 23，以及 root/library fmt、strict Clippy、release build 全部通过。冻结 `r2-menu-wire`，SHA256 `ad5f87ad780763733aba2500814628149c4d0812e4b9e373fb380b99d0b2427d`，6,013,928 bytes，相对 scalar 冻结 +21,520 bytes，在本批 +24KiB 上限内；399 个 Rust 输入和 target/frozen executable 核对一致。原创 gateway-menu 13K work 旧/新 raw/IR 四次 smoke 全部校验，retired 13,022、API 2,003、checksum 0。该低 work smoke 仅验证行为，不作性能准入。

随后串行运行旧/新 Alice 各一次 200M 功能 smoke，均在相同预算退出，hidden HWND 0x7ffd00630000、client 314×211、Draw/Present 0/0；证据 `r2-menu-functional-alice`。复用已有游戏 runtime，只临时软链接二进制并在退出后删除。该 smoke 不用于性能准入；完整验证汇总 `r2-menu-complete-verification.json`。
