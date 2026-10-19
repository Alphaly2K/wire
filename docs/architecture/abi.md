# PE32 ABI 来源

`win32::abi::pe32` 集中维护原 14 份 Wine/Hangover spec、typed i386 参数解析、ordinal 名称解析及显式 legacy 例外。原私有根模块 win32_abi 已迁入该目录；没有改第三方输入或引入新的 ABI 推测。

绑定优先级保持：catalog 的架构契约 → 原 DSound/WMI/D3D9/GDI+ 方法入口 → legacy 例外 → typed i386 spec → 原 CRT 零清栈策略 → 未知 ABI。ordinal 名称解析仍只依据原 spec 的 typed i386 范围，例外表和 CRT 策略不扩大 ordinal 可见性。

根 API 表原有 301 条手工声明。以 `0459298` 审计，270 条与 spec 完全一致，全部删除；其余 31 条保留在 override_stack_bytes，并按 cdecl varargs、未声明类型的 GDI 别名、未加载 spec 的 provider 和直接 ordinal 拼写注明原因。现有 spec 共解析出 5,614 个 named stdcall ABI；手工表与这些记录没有数值冲突。ABI 已知仍与实现能力分开，未知 CRT 名字仍沿用历史零清栈诊断策略。

native_export 继续只描述 ABI 可知性；旧 msvcrt 名称白名单已被更早的 CRT 零清栈分支覆盖，因此归并为同一查询。没有新增实现、改变 DLL 大小写、补造未知签名或改变 dispatch 的 cdecl 参数读取策略。

原 spec 输入、解析主体、ordinal 范围、COM 优先级及 31 项例外逐项核对保持。release 完整 Win32 248、fmt/release lib/bin strict Clippy/build 通过；不重复性能或游戏矩阵。证据见 `wire-out/refactoring-2026-10-14/r2-pe32-abi-{inventory,source-checks,verification}.json` 及 validation 日志。

随后六项 version.dll 文件资源入口接入 [唯一领域契约](version-resources.md)，删除对应 override；当前 legacy 例外剩余 25 项，原 ABI 字节数和 ordinal 范围保持。前文 31 项是来源收敛时的历史基线。
