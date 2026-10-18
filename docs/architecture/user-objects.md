# USER 注册名称与窗口属性

`win32/catalog/user_objects.rs` 声明十四项 provider/ABI/执行策略：PE32 十项名称/属性操作为 typed，四项属性枚举保留 ABI-only、error120、零返回和原清栈，不调用 callback；Win64 全部 absent。

`win32/user/objects.rs::RegisteredNames` 私有拥有名称表与 next atom，由 PE32 User32 组合。保持原有 clipboard-format/window-message 共享的进程逻辑 namespace，按 Unicode lowercase 匹配；校验原名称非空且不超过 255 UTF-16 units，再查已有记录，最后检查容量并插入。范围包括 0xffff，表满后已有名称仍返回原 atom。此表不访问宿主 clipboard 或桌面。

每个 PE32 Window 组合私有 `Properties` owner，存放完整 u32 值。`PropertyKey::Atom/Name` 表达原字符串前缀区分的两种键；atom 查询不再构造字符串。名称继续按原规则小写，空字符串与零属性值保持；窗口释放同时释放属性。四项枚举尚未实现，不增加属性排序或 callback 行为。

受检适配位于 `user32/objects.rs`。注册名与属性名解码、参数/返回布局留在架构层；domain owner 不访问 guest 内存、CPU、线程或 LastError。属性解码先于 HWND 校验，HWND 校验先于零 key 校验；原访存失败、LastError、trace 与返回帧顺序保持。

独立回归包含完整 atom 容量、满表查询、A/W 与两类名称共享、atom/name 键区分、零值/空名、窗口销毁和枚举 ABI；见[批次报告](../reports/refactoring-user-objects-2026-10-18.md)。
