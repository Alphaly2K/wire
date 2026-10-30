# PE32 菜单契约与资源归属

新 Wasm 入口接通 `win32/catalog/menu.rs` 的全部 27 项 PE32 导出。二十项提供真实菜单和条目状态；DrawMenuBar、TrackPopupMenu/Ex、RedrawWindow、LoadMenuA/W、GetSystemMenu 返回零和 LastError 120。菜单绘制、弹出跟踪、资源加载和系统菜单仍待实现。AMD64 保留旧 absent 范围。

新旧入口共用 `win32/user/menus.rs` 的对象、条目和菜单图处理体，旧 `menus32.rs` 只适配内存、窗口表、LastError 和 trace。保留递归命令查找、30 层全图校验、循环拒绝及修改回滚。新进程最多拥有 4096 个菜单，每菜单最多 4096 项，总计费额度 16 MiB；计费包含菜单、条目和 UTF-16 payload。PE32 单调句柄位于 `0x41010000..0x42000000`，销毁释放容量并保持身份不复用。

MENUITEMINFO 保留 44/48 字节布局、掩码归一化、CP932 字节截断、原始 UTF-16 和旧 bitmap 插入顺序。输入字符串复制到菜单所有者，cch 不限制输入长度。读取条目时预检文本和结构两个完整输出，再依次写文本、结构；别名遵循该顺序，API 使用预先保存的返回帧。新适配传播客体访存 Fault，修复后可重试；旧适配保留将内存错误转换为 87 的协议。普通逻辑错误写入 LastError，五种 DWORD 查询/状态接口使用全一失败值。

PE32 顶层窗口可在创建或 SetMenu 时附加已有菜单，类菜单名尚不自动实例化菜单。菜单有无变化时按共享默认边框重算客户区，替换已有的自定义客户区；当前不投递菜单变更专用 NCCALCSIZE 通知。菜单和客户区提交前整批核对 GDI 像素额度并准备 backing，保留 DC/bitmap 身份与重叠像素，失败不发布任何几何或菜单变化；成功后重新失效客户区。

DestroyMenu 和 DeleteMenu 递归退休共享子菜单，并清除存活菜单及顶层窗口的失效引用；RemoveMenu 只分离条目。child control ID 即使与菜单句柄相同也保持独立。窗口最终销毁、失败创建和线程退出沿用同一菜单退休入口。强制窗口退休无法因像素扩容失败而回滚，此时存活窗口更新几何并保留既有 DC/像素，下一次 GetDC 重试 backing 分配。静态启动拒绝清空菜单所有者。实际宿主窗口和图形提交仍待接入。

旧网关继续保留四个 typed 未实现入口的 menu note，以及三个 ABI-only 入口的空 note；公开旧 user32::dispatch 的 RedrawWindow 仍走原 paint 路径。根 menu 模块只重导出共享 Menus 与字体、文本、窗口使用的 read_text。
