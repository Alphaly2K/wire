# Windows printf 契约与 PE32 变参

`win32/catalog/windows_printf.rs` 声明 wsprintfA/W 与 wvsprintfA/W 的四个 export。PE32 保留精确 user32.dll provider 和原 named export；Win64 继续 absent。普通固定参数只包含目标、格式和可选 va_list，不把变参当作固定 scalar 签名展开。

| 入口 | PE32 调用约定 | 固定参数 | 清栈 |
| --- | --- | ---: | ---: |
| wsprintfA/W | cdecl | 2 | 0 字节 |
| wvsprintfA/W | stdcall | 3 | 12 字节 |

Contract 默认使用 Stdcall，cdecl 必须显式声明；原公开 stdcall_bytes 查询仍返回实际 callee 清栈字节数。两条 wsprintf 的旧 ABI override 删除，其余 legacy 和 CRT 策略保持。

主 gateway 按绑定 ID 选择两参数的 cdecl 前缀；wvsprintf 由三参数 stdcall 契约读取参数。`win32/x86/windows_printf.rs` 只借固定参数、GuestMem 和原 ESP：cdecl 变参从 ESP+12 开始，va_list 入口使用第三个参数。格式化器仍为原 `wsprintf.rs`，按格式需要逐项读取变参，字面量不会访问无用的 va_list。

CP932/UTF-16 转换、Windows 字符/数字规则、1023-unit 上限、错误文本和 guest 输出检查保持。输出仍先于 gateway 读取返回地址，因此允许输出覆盖原 return slot；LastError、日志和返回 frame 的提交顺序保持。

独立回归将调用栈的最后一个固定参数放在已映射区末端，并将 va_list 指向不可读区：四个字面量调用应成功，cdecl/stdcall 分别保留原 ESP，LastError 保持。另验证 wide 输出覆盖返回地址；既有混合变参、I64、CP932/UTF-16 与输出 fault 用例继续保留。
