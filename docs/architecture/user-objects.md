# USER 注册名称与窗口属性

Wasm 入口在 PE32 与 AMD64 通过 user32 接入十项注册名称/属性操作和六项剪贴板查询/失败协议。`win32/catalog/user_objects.rs` 保留旧 provider/ABI/执行策略；四项 EnumPropsA/W/ExA/ExW 仍未接入新入口，没有属性枚举能力。

当前入口共用 `user/object_services.rs` 的处理函数和 `user/objects.rs` 的状态。RegisteredNames 保留 clipboard-format/window-message 共享的进程 namespace，按 Unicode lowercase 匹配；先校验原名称非空且不超过 255 UTF-16 units，再查已有记录，最后分配 0xc000..0xffff。表满后已有名称仍可查询。名称注册仅维护客体进程状态，不接宿主 clipboard 或桌面。

Properties 存放窗口拥有的完整指针值。Atom/Name 是两类独立键；名称沿原规则小写，空名称与零属性值有效。属性名解码先于 HWND 校验，HWND 校验先于 NULL key 校验；无效 HWND 返回错误 1400，NULL key 返回 87，普通查询缺失返回零并保持 LastError。窗口销毁回调期间属性仍可访问，最终移除窗口时释放；线程退出、启动拒绝和进程终止沿同一 USER 清理路径回收。

新适配使用当前窗口表、CP932/严格 UTF-16、VMM 和保存的调用帧。实际源、错误槽或返回帧故障不完成调用，也不消耗名称身份或修改属性；修复后可重试。全部调用使用当前 VMM 和保存的返回帧。

OpenClipboard 没有数据 owner 或传输后端：NULL 或有效本进程 HWND 返回 FALSE/错误 50，非法 HWND 返回 1400。CloseClipboard、EmptyClipboard、GetClipboardData 和 SetClipboardData 返回零与 ERROR_CLIPBOARD_NOT_OPEN(1418)，不访问数据、不接管或释放传入句柄、不调用延迟渲染过程。IsClipboardFormatAvailable 对所有格式返回 FALSE并保持 LastError；已注册格式名称也不会发布数据。宿主剪贴板、进程间共享、Global 内存所有权和延迟渲染仍缺失。
