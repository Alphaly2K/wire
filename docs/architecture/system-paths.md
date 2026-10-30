# 系统与临时目录查询

新 Wasm 入口接入十项服务，两种 ABI 均接受 kernel32/kernelbase：GetSystemDirectoryA/W、GetWindowsDirectoryA/W、GetSystemWindowsDirectoryA/W、GetTempPathA/W 和 GetTempPath2A/W。共享目录中的旧 PE32 ABI-only 和 Win64 ANSI temp absent 策略仅作用于旧入口。

系统目录为虚拟 profile 的 `C:\Windows\System32`，两类 Windows 目录均为 `C:\Windows`。临时目录依次选择客体环境中非空的 TMP、TEMP、USERPROFILE，均不存在时使用 Windows 目录；当前普通用户的 GetTempPath2 使用相同选择。路径在虚拟命名空间按当前目录做词法解析，临时目录带末尾反斜杠，不要求目录已经存在，也不创建目录或访问宿主文件系统。

系统目录参数顺序为 buffer/count，临时目录为 count/buffer，容量截断为 DWORD。ANSI 使用 CP932 字节数，Wide 使用 UTF-16 code unit；不足容量返回包含 NUL 的所需长度并保持输出和 LastError，成功写入 NUL 并返回不含 NUL 的长度。非法虚拟路径返回零及对应错误码。完整输出统一预检，故障可重试；返回槽与输出重叠时仍使用保存的调用帧。服务完成计一 work、不计客体退休，不采样时间。
