# 进程内注册表

新 Wasm JIT 运行时在 PE32 和 AMD64 接入 advapi32 的原有 17 项注册表接口：`RegCreateKeyA/W`、`RegCreateKeyExA/W`、`RegOpenKeyA/W`、`RegOpenKeyExA/W`、`RegQueryValueA/W`、`RegQueryValueExA/W`、`RegSetValueExA/W`、`RegDeleteValueA/W` 和 `RegCloseKey`。另外接入 `RegEnumKeyA/W`、`RegEnumKeyExA/W`、`RegEnumValueA/W`、`RegQueryInfoKeyA/W`、`RegDeleteKeyA/W`、`RegDeleteKeyExA/W`、`RegDeleteTreeA/W`、`RegFlushKey` 和 shlwapi 的 `SHDeleteKeyA/W`，共 34 项声明。返回值为 LSTATUS，不读写线程 LastError，不采样时钟；kernel32/kernelbase 的旧私有别名不进入新路径。

注册表由当前进程独占，线程与 DLL 共用同一状态，其他 VM 相互隔离；不访问宿主注册表、不持久化，也不预填软件安装记录。支持 HKCR、HKCU、HKLM、HKU 和 HKCC，只有 HKCU 可写。预定义根接受零扩展与符号扩展身份；普通句柄按完整指针宽度比较，只由 RegCloseKey 关闭。PE32 句柄位于 `0xa0010000..0xa0020000`，AMD64 位于 `0x7ffd04000000..0x7ffd04010000`，不与其他资源重叠。关闭后不复用身份，活动与累计句柄均最多 4096 个。进程终止及静态启动拒绝清理全部键值和句柄。

键和值名按 Unicode 小写匹配，并保留首次创建时的显示拼写；重复设置同名值不改变其拼写。A 接口使用 CP932，W 接口使用 UTF-16。路径最多 4096 个 UTF-8 字节，值名最多 16384 个 UTF-8 字节；拒绝重复或尾部反斜杠，HKCR 的 Open 允许一个前导反斜杠。进程最多 4096 个节点（含五个根）、4096 个值，归一化路径、显示名、归一化值名和数据合计最多 16 MiB。单值输入及转换后数据分别限 1 MiB；配额失败返回错误 8，超出输入长度限制返回 87。

创建自动生成缺失的中间路径；整个路径、句柄和输出均验证成功后才发布。成功 disposition 为 CREATED_NEW_KEY 或 OPENED_EXISTING_KEY。Open 的普通错误清零输出句柄，Create 的普通错误保持输出；访存故障保留全部输出和状态。普通 Open 的空路径返回原句柄，OpenEx 的空路径对预定义根同样返回原身份；这些别名不消耗新句柄。

访问掩码支持 generic rights 与 MAXIMUM_ALLOWED，query/set/create 分别检查查询、写值和创建子键权限；系统根拒绝写权限。WOW64 的 64 位标志选择同一逻辑视图，32 位视图返回错误 50，两个视图标志同时设置返回 87。支持普通及 volatile 创建；link、备份恢复、class 与安全属性尚未实现，已识别请求返回 50，非法标志返回 87。

值数据支持任意类型与二进制内容。REG_SZ、REG_EXPAND_SZ、REG_MULTI_SZ 的 A 数据转换为 UTF-16，保留嵌入 NUL；W 数据保留原始字节，包括奇数字节和未配对代理项。Set 对完整字符结尾检查紧邻的未计入终止符，若存在则一并存储；该读取失败也不会替换旧值。A 查询以替换字符处理无效 UTF-16，丢弃不完整的尾字节；W 查询返回原始数据。

QueryEx 的 NULL 数据缓冲区只查询类型和实际字节数，不读取原容量。容量不足返回 ERROR_MORE_DATA，更新类型/所需字节数，保持数据缓冲区。字符串缺终止符且有额外容量时补 NUL，报告长度仍是存储数据转换后的长度。旧式 QueryValue 的第二个参数为子路径，查询其未命名值；该值缺失时成功返回空字符串，缺失子键仍返回错误 2。

调用帧先捕获参数与原返回地址，查询再读容量并预检所有实际输出，写入顺序为类型、长度、数据。输出允许彼此别名以及覆盖原返回槽。任何首帧、输入、输出或 guard 故障均不完成服务、不增加 work，不发布键/句柄或覆盖原值；修复映射后可以重试。全部值更新保留在核心 Wasm 中，不引入客体执行桥。

枚举使用零起始索引，只列直接子键或当前键的值，越界返回 259。键和值名长度不含终止符，缓冲容量必须容纳终止符；A 的长度按 CP932 字节计，W 按 UTF-16 单元计。短容量返回 234 并报告所需长度，保留未容纳的名称和数据缓冲区。枚举值支持未命名值、可选类型/数据长度，数据使用与 QueryEx 相同的 A/W 转换，但不补存储之外的字符串终止符。索引顺序按归一化名称排列；增删后调用者应重新枚举。

QueryInfoKey 返回直接子键数、值数、名称和数据最大容量；最大名称长度按接口编码计数，最大数据长度按查询转换后的字节数计。当前键类为空，可查询长度或写入空终止符。没有键修改时间和安全描述符后端；EnumKeyEx 请求时间输出、QueryInfoKey 请求时间或安全描述符长度时，返回 50 并保留全部输出。RegFlushKey 验证句柄和查询权限后返回 50，没有持久化成功的占位返回。

RegDeleteKey/Ex 删除无子键的非空子路径，包括该键全部值；父句柄的访问掩码不限制这一操作，系统根仍只读。含子键返回 5，NULL/空子路径返回 87。Ex 仅接受零或 WOW64_64KEY 的同一视图，WOW64_32KEY 返回 50，其他标志/非零 reserved 返回 87。RegDeleteTree 和 SHDeleteKey 共用递归删除，要求 DELETE、KEY_ENUMERATE_SUB_KEYS 和 KEY_QUERY_VALUE，涉及值时还需 KEY_SET_VALUE。NULL/空子路径清空当前键的内容并保留当前节点；非空子路径删除整个目标子树。

删除会从名称空间移除节点，并把所有指向被删节点的句柄标为已删除。此后访问这些句柄返回 1018，RegCloseKey 仍可成功关闭；同路径重建不会恢复旧句柄。删除释放节点和值的存储额度，句柄累计身份额度仍不回收。树清空保留当前键的句柄，只撤销被删除子树的句柄。

逻辑状态与配额来自 `src/win32/registry_state.rs`，适配器在 `crates/wire-wasm/src/win32/registry.rs`。所有接口共用同一 owner 和输出预检；旧接口的 catalog 策略作为冻结迁移出处保留，新路径资格由实际服务表决定。

稳定的 ABI、转换、权限、配额、输出别名和故障检查由 `node crates/wire-wasm/tests/registry.mjs`、`node crates/wire-wasm/tests/registry-enumeration.mjs` 与共享状态单元测试运行。显式的 `node tools/probes/check_wasm_pe_entry.mjs --registry-only` 需要 MinGW 两种交叉编译器和支持 Wasm 的 Node，验证项目 C 程序的 PE32/AMD64、O0/O2 导入、线程共享与 LastError；不依赖外部游戏或本机资料。
