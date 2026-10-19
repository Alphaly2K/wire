# 系统与临时目录查询

十项 `SYSTEM_PATH_CONTRACTS` 统一目录查询身份、参数顺序、DWORD 截断及架构范围。PE32 均保留 kernel32 的既有 ABI-only 策略；Win64 支持六项 Windows/system A/W 查询及两项 Wide temp 查询，静态和动态 provider 为 kernel32/kernelbase。ANSI temp 查询在 Win64 继续 absent。

Windows、System32 和 Temp 默认路径移入不可变 GuestEnvironment；`win32::x64::system_paths` 只借 Memory、ID、参数和环境描述，独立完成计数与受检 copyout。系统目录参数为 buffer/count，临时目录为 count/buffer，长度以 A/W 字符单位计算。不足容量返回包括 NUL 的所需长度且不写输出；成功写入 NUL 并返回不含 NUL 的长度。保留 LastError，不检查或创建宿主目录。

独立基线 `dba0edf` 在旧实现上固定双架构行为：Win64 覆盖八项查询的 provider、NULL 长度查询、容量边界、A/W 字节、尾部哨兵、返回槽别名及跨页故障时 CPU/API count/output；PE32 覆盖十项 ABI-only 名称和 kernelbase 未知清栈。此批不涉及执行器或内存/cache 热路径，不做性能计时；证据位于 `wire-out/refactoring-2026-10-14/r2-system-paths-*`。

相关 release 双架构各 1 项、catalog 一致性、fmt、release lib/bin strict Clippy 和 release build 通过。八项现有 Win64 arity/DWORD 与两项 absent 名称对照旧元数据保持，十项 PE32 参数槽与原 Wine ABI 一致。源码/executable SHA 记录于验证 JSON，累计 **851 exports**。
