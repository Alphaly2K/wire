# NLS / locale 边界

42 个已存在 export 的 DLL、ABI、执行策略和 typed ID 来自 `win32/catalog/nls.rs`。这批迁移覆盖 codepage 查询/转换、字符分类、locale 查询/映射、日期格式与 locale 枚举；保留原有受限能力，没有填补 Windows NLS 数据库或 collation。

纯 locale 词汇、格式化和 C1 字符分类位于 `win32/kernel/nls.rs`；SYSTEMTIME/FILETIME 算术位于 `win32/kernel/calendar.rs`。两种架构的适配器各自实现受检内存、布局和错误输出；共享既有 `codepage` 转换，避免统一指针或强制合并故障策略。宿主 localtime 调用位于 `host/calendar.rs`。

| 原有行为 | PE32 | Win64 |
| --- | --- | --- |
| provider | kernel32 | kernel32/kernelbase |
| 无效输入指针 | 转换 API 返回 0/LastError=87；GetCPInfo 传播访存错误 | 传播访存错误，CPU/API count 不提交 |
| MultiByteToWideChar flags | 保留宽松接受，消费 strict decode bit | 非法 bits 返回 0/1004 |
| WideCharToMultiByte used-char 输出 | 在容量检查前写入，失败可能有部分输出 | 容量与指针 preflight 后才写 |
| input 上限 | 原 PE32 计数/终止边界 | 终止 32768 units、transfer 16MiB |
| CP_THREAD_ACP=3 | GetCPInfo 拒绝 | GetCPInfo/Ex 映射 CP932 |
| GetCPInfoEx、locale info/name/date | 部分仅 known ABI，原未实现策略 | 已有受限实现 |
| DBCS Ex | codepage 参数仍忽略 | 没有原有绑定，继续拒绝 |
| return slot alias | 输出后读取返回 frame | handler 前保存 frame |

`EnumSystemLocalesEx` 的 Win64 binding 选择专用 ControlImport。进程层负责线程/executor 检查、回调栈验证、分配/释放、CPU/trace 提交、stop-on-false 与跨 budget 继续；NLS adapter 只填充回调输入，host 层不运行 guest CPU。四种 locale、过滤顺序和回调参数保持原样。PE32 仍是 ABI-only。

公开 `wire::nls::dispatch` 保留旧函数范围并委托新 adapter；main gateway 直接使用冷绑定 ID，这个兼容入口不参与 typed 热路径。未迁移的 shell allocator 与 timezone API 仍留在旧 facade。Unicode 属性继续使用 Rust 表，排序、安装的语言数据库与自定义转换字符等限制按原路径处理。

`tests/nls_contract.rs` 固定两架构的 flags、部分输出、CP 别名、wrong provider、return-slot alias 和公开 facade。原有 locale callback、calendar、codepage、buffer fault 与 CRT character 测试继续运行。性能工具显式选择 `gateway-nls` / `nls-conversion`，后者同时校验转换的 UTF-16 字节与返回值累加，默认 case 集合保持原样。
