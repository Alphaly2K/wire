# PE32 菜单契约与资源归属

`win32/catalog/menu.rs` 集中 27 项 export 的 DLL、参数、stdcall 清栈、typed ID 和架构执行策略。PE32 的 24 项领域入口在绑定时选择 ID；LoadMenuA/W、GetSystemMenu 保留 ABI-only。Win64 全部 absent。当前迁移与验收状态见[批次报告](../reports/refactoring-menus-2026-10-19.md)。

`win32/user/menus32.rs` 拥有原有菜单对象、条目和单调分配句柄。调用只借用 GuestMem、窗口表和 LastError，不再取得整个 ApiState。原菜单图、30 层限制、4096 个对象/条目上限、递归命令查找、循环拒绝与修改回滚保持；本轮没有合并两架构资源模型。

MENUITEMINFO 的 44/48 字节布局、掩码归一化、CP932/UTF-16 文本、旧 bitmap 顺序和输出提交规则保持。读取 item 时先验证文本与结构两个输出，才提交任一输出。共享子菜单销毁会断开存活菜单的引用；窗口上的 child control ID 与菜单句柄即使数值相同也分别处理。窗口销毁继续通过原 Menus::destroy 入口释放菜单。

DrawMenuBar、TrackPopupMenu/Ex 和真实 API 网关的 RedrawWindow 保留 error 120、menu note 和 unimplemented trace。公开 user32::dispatch 的 RedrawWindow 仍进入原 paint 路径；这个已有的入口差异由独立基线固定，后续功能修正需要另行提交。三个 ABI-only 入口保留空 note。根 menu 模块仅重导出 Menus 和已被字体、文本、窗口使用的 checked read_text，只有一份实现。

性能证据独立对照 scalar-isolation 冻结，不覆盖尚待验收的对话框批次。本批预设 executable 增量上限 +24KiB；累计已验收数量在完整正确性、稳定性能与游戏前缀完成前保持 595。
