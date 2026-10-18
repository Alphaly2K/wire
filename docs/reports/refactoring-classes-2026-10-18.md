# R2/R3 窗口类注册与查询（2026-10-18）

状态：六项独立旧行为基线与可选性能 fixture 已通过；新模块尚未接入，不计入已验收累计数。

范围为 RegisterClassA/W、RegisterClassExA/W、UnregisterClassA/W、GetClassInfoA/W、GetClassInfoExA/W、GetClassLongA/W、SetClassLongA/W、GetClassWord、GetClassNameA/W，共十七项。PE32 全部已有处理；Win64 六项注册/注销已有处理，其余十一项 absent。Win64 静态接受 kernel32/kernelbase/user32，默认动态只有 user32，保留已有 gateway fallback。

两种架构的原差异保留：PE32 类名最多 255 UTF-16 code units、Unicode lower-case 匹配、CS_GLOBALCLASS 可跨 instance 查询/注销、允许非零 class extra 与菜单；Win64 最多 256 UTF-8 bytes、ASCII case-insensitive 匹配、注销要求完整 instance，class extra 非零或菜单非零返回 error50。PE32 atom 允许 0xffff，Win64 到 0xffff 即拒绝；均单调分配而不复用。

PE32 GetClassInfo* 与 CreateWindowEx* 原有系统控件隐式注册必须保留。持久 A/W 名称/菜单分配、失败不插入类或消耗 atom，以及既有错误/输出/返回帧顺序也属于边界。ClassLong 的已有索引/extra 限制和 GetClassWord 原有完整 32-bit 返回值保持，名称复制继续复用受检 CP932/UTF-16 helper。PE32 类表仍有窗口创建和公开兼容访问；本批不声称已完成其私有 owner。Win64 类表与 next atom 将交给独立 owner，由窗口创建借用只读记录。

原创可选 `gateway-classes`：Win64 先注册一次，循环相同类的重复注册，最后注销并消费返回值；PE32 注册一次，循环 GetClassInfoW，消费 BOOL 和复制出的 style，最后注销。两端资源数量恒定，不使用持续创建类的循环。

旧 runtime 上六项独立测试通过（PE32/Win64 各三项），覆盖隐式控件、provider、菜单访存失败与 atom 提交、全局类查找、字段/名称错误、Win64 完整 instance、extra/menu 拒绝与字节长度限制；已有窗口生命周期回归继续保留。证据 `r2-classes-baseline{32,64}-tests.txt`。工具 20 项通过，原创 12K fixture 由旧冻结程序实际执行，Win64 与 PE32 IR/raw 均通过：Win64 retired=5n+19、work=6n+21、API=n+2、RAX=1；PE32 retired=11n+18、API=n+2、EAX=4n+1。该轮只核对正确性，不作计时结论，见 `r2-classes-fixture-baseline{32,64}`。

before 为 `r2-desktop-wire/wire`，SHA256 `97689a99eb05cbf37372cbce693d3af16e9caa64fdb5ae317753cb3f035d7383`，5,956,232 bytes。候选文件预算 +24KiB；证据根 `wire-out/refactoring-2026-10-14/`。
