# 窗口类注册与元数据

`win32/catalog/classes.rs` 是十七项 class API 的 provider、参数与执行策略权威来源。PE32 全部接入 typed ID；Win64 只绑定原有六项注册/注销，信息、字段和类名查询仍 absent。Win64 静态接受 kernel32/kernelbase/user32，默认动态只有 user32；已有 gateway fallback 仍保留。

PE32 的受检记录、持久 A/W 名称/菜单和 typed handler 在 `user32/classes.rs`。公开 `user32::WindowClass` 与 `find_class` facade 保留；`User32.classes` 仍服务窗口创建和兼容访问，本批没有将这张表伪装成私有 owner。系统控件隐式注册由同一 helper 服务 GetClassInfo* 和 CreateWindowEx*，保留原调用顺序。字段查询、额外存储限制、名称输出及原 trace/LastError 保持；GetClassWord 也保留原完整 32 位值。

Win64 的 `win32/user/classes64.rs::Classes` 私有拥有类记录和 next atom，提供注册、按 instance/key 查找与注销。注册先检查同 instance 的重名，再检查单调 atom 上限；窗口创建借用只读记录并复制所需属性，注销仍由架构协调层先检查存活窗口。guest 记录解析、受检内存和 LastError 在 `x64/runtime/user/classes.rs`，不进入领域 owner。

| 现有规则 | PE32 | Win64 |
| --- | --- | --- |
| 类名限制 | 255 UTF-16 code units | 256 UTF-8 bytes |
| 名称匹配 | Unicode lower-case | ASCII case-insensitive |
| 全局类跨 instance | 查找/查询/注销允许 | 窗口创建允许，注销要求相同完整 instance |
| class extra / menu | 保留原 metadata 和持久菜单 | 非零 class extra/menu 返回 error50 |
| atom 上限 | 可分配 0xffff | next==0xffff 即拒绝 |

这些差异按旧行为保留。PE32 先分配持久字符串再插入类，访存失败不消耗 atom；已分配 scratch 不在本批增加回滚。两端继续使用各自既有返回帧与故障提交规则。独立基线、窗口生命周期和冻结性能证据见[窗口类批次报告](../reports/refactoring-classes-2026-10-18.md)。
