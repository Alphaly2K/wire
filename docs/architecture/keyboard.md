# 键盘状态与布局边界

新 Wasm 入口在 PE32/Win64 接入 `KeyboardApi` 的 16 项接口，ABI 与 provider 来自 `win32/catalog/keyboard.rs`。PE32 的布局操作在新运行时具备实际实现，旧目录中的 ABI-only 执行策略保持不变。状态和文本复用共享 Keyboard owner，虚拟 US 的 scan/character/key-name 规则复用已有 LGPL 实现并保留归属；不引入旧 CPU 执行器。

GetAsyncKeyState 查询进程的物理键状态并消费该键的“自上次查询后按下”低位；GetKeyState/GetKeyboardState 查询当前线程取出硬件消息时的快照。只有成功的 GetMessage 或 PM_REMOVE 更新队列状态，PM_NOREMOVE、输出故障和 PostMessage 不模拟硬件消费。左右修饰键独立保存，通用键 high bit 为两侧的 OR；异步自动重复不重置 press latch。非法 VK 返回零。GetKeyboardState 写出完整 256 字节，跨页故障保留可访问前缀，输出可与原调用帧重叠。

GetKeyboardLayout 使用固定 HKL `0x04090409`，tid 为零表示当前线程，已退出或不存在的线程返回零。GetKeyboardLayoutList 返回一个布局，negative signed count 返回 87，count 为零或输出为 NULL 时仅返回数量，不访问输出；实际输出使用 guest 指针宽度。GetKeyboardLayoutNameA/W 写入 `00000409` 及 NUL，GetKeyboardType 使用虚拟增强键盘类型 4、子类型 0、十二个功能键。HKL 参数保留完整宽度，Ex 接受零或该固定身份，其余返回 87。

MapVirtualKeyA/W/ExA/W 支持虚拟键、Set-1 scan、字符和 EX 左右修饰键映射，未知映射类型返回零。GetKeyNameTextA/W 使用 signed capacity，非正容量或 NULL 输出返回零；结果按容量减一截断并 NUL 结束。key name 来自同一 US profile。

ToUnicode/Ex 先检查 signed 容量，再检查 flags/HKL，然后读取完整的调用者 256 字节状态并写 UTF-16。输入/输出可以重叠，写入故障不修改前缀。flags 接受 0..7；keyup 且没有相应 flag 时不生成字符。当前至多产生一个字符，容量大于一时再写一个 NUL；无字符时写零并返回零，实际 U+0000 字符返回一。接口不改变队列键态或消费宿主文本缓存。

虚拟 US 布局与客体日语 NLS profile 独立。宿主按实际布局解析的 UTF-16 仍经[输入事件](input-events.md)缓存并由 TranslateMessage 交付；ToUnicode 的 US 查询独立于这条路径。IME、dead-key、AltGr、组合状态及布局切换尚未接入。上述查询不读取时钟，成功保留 LastError，错误分支才写错误槽；访存失败沿统一 Fault/修复协议处理。
