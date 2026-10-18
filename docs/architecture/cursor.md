# 游标状态与架构边界

状态：10 项契约及状态 owner 已接入候选，验证进度见[本批报告](../reports/refactoring-cursor-2026-10-18.md)。

`catalog/cursor.rs` 集中声明参数、返回载体和架构策略。PE32 Show/Get/Set/GetPosition/SetPosition typed，其余五项 ABI-only；Win64 九项 typed、SetPosition absent。Win64 TrackMouseEvent 静态仅 user32，其他八项保留静态 kernel32/kernelbase/user32；动态均仅 user32。BOOL 参数按 DWORD 计划截断，句柄保留 guest 指针宽度。

`user/cursor.rs` 私有拥有当前句柄、signed position 和 per-thread saturating display count；用具体 guest handle/thread 类型实例化，保持两端 key 宽度。PE32 线程退出移除计数，Win64 保留既有生命周期。image 有效性仍由各架构资源 owner 判断：PE32 只接受 NULL/stock arrow，Win64 必须是完整 image handle 且 kind=cursor。

PE32 适配只借 Args、GuestMem、cursor、tid 和 LastError；位置设置不裁剪，输出仍由 handler 写入后再读取返回槽。Win64 Cursor64 拥有虚拟桌面 clip，裁剪与 host 鼠标事件复用同一 owner；只约束 guest 指针，不改变宿主桌面。invalid clip 保持旧状态，NULL clip 恢复固定桌面，position 同步 clamp。消息入队与元数据从 owner 取得坐标，原有消息时间和键盘处理顺序保持。

Win64 tracking 只借 Memory 和窗口存在性查询，先检查完整 24-byte TRACKMOUSEEVENT，再按旧顺序处理 size/query/HWND/flags/cancel/unsupported 注册。没有 hover/leave 或 WM_POINTER 后端时保留既有错误。入口负责提交 LastError，saved return frame 与跨页输出故障行为保持。旧手写 ABI/arity/DWORD/provider 特例和名称分发删除。
