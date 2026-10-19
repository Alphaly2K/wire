# PE32 资源查询与字符串输出

`win32::catalog::resources` 是十项 export 的 provider、ABI 和执行策略来源：user32 的 `LoadStringA/W`，kernel32 的 `FindResourceA/W`、`FindResourceExA/W`、`LoadResource`、`SizeofResource`、`LockResource`、`FreeResource`。PE32 在导入或动态注册时绑定 ResourceApi，执行时不按名称判断 A/W、Ex 或载入/大小操作；Win64 保持 absent。

`win32::x86::resources` 负责参数解释、输出和错误提交，只借用 GuestMem、EXE 基址和 LastError。资源本身是映射 image 的只读视图，没有独立分配的 host handle 或新的资源 owner。公开 `resource` 模块继续负责 PE 目录、data entry 和字符串块的受检读取，现有调用者保持原入口。

现有边界如下：

- NULL module 使用主 EXE 基址。名称指针按 A/W 解码，数字 ID 使用原范围；目录名称保留 ASCII 大小写折叠。Ex 的语言参数保留完整 DWORD carrier，查找失败后仍回退首个语言项。
- FindResource 缺失返回零并设置 LastError 1814；资源数据项读取失败返回零，保留 LastError。LockResource 保持地址原值，FreeResource 返回零。
- LoadString 把 ID 截到低 16 位；W 的零容量模式写出字符串地址及长度，空槽保留其地址，缺失块写零。正容量包含终止 NUL，A 使用 CP932 并保留按字节截断，负容量不写输出。
- W 逐 UTF-16 单元写出，A 的数据与终止字节分开写出。故障可以发生在部分输出之后；此时 gateway 不完成 API 计数和返回帧。成功输出先于原 gateway 读取返回地址，保留输出与返回槽重叠的行为。

独立布局和故障测试位于 `tests/win32/resources_contract.rs`；Win64 的导入与 fresh-provider GetProcAddress 缺口由 `tests/x64/resources_contract.rs` 固定。原创 `gateway-resources` fixture 每轮调用全部十项 API，并消费 W/A 输出；输出放在 guest 栈上，避免写映射 image 使其静态 IR 失效。验证与性能状态见[迁移记录](../reports/refactoring-resources-2026-10-19.md)。
