# R2/R3 窗口类注册与查询（2026-10-18）

状态：六项独立旧行为基线与可选性能 fixture 已通过，十七项候选已接入且完整正确性通过，性能与游戏前缀验收进行中，不计入已验收累计数。

范围为 RegisterClassA/W、RegisterClassExA/W、UnregisterClassA/W、GetClassInfoA/W、GetClassInfoExA/W、GetClassLongA/W、SetClassLongA/W、GetClassWord、GetClassNameA/W，共十七项。PE32 全部已有处理；Win64 六项注册/注销已有处理，其余十一项 absent。Win64 静态接受 kernel32/kernelbase/user32，默认动态只有 user32，保留已有 gateway fallback。

两种架构的原差异保留：PE32 类名最多 255 UTF-16 code units、Unicode lower-case 匹配、CS_GLOBALCLASS 可跨 instance 查询/注销、允许非零 class extra 与菜单；Win64 最多 256 UTF-8 bytes、ASCII case-insensitive 匹配、注销要求完整 instance，class extra 非零或菜单非零返回 error50。PE32 atom 允许 0xffff，Win64 到 0xffff 即拒绝；均单调分配而不复用。

PE32 GetClassInfo* 与 CreateWindowEx* 原有系统控件隐式注册必须保留。持久 A/W 名称/菜单分配、失败不插入类或消耗 atom，以及既有错误/输出/返回帧顺序也属于边界。ClassLong 的已有索引/extra 限制和 GetClassWord 原有完整 32-bit 返回值保持，名称复制继续复用受检 CP932/UTF-16 helper。PE32 类表仍有窗口创建和公开兼容访问；本批不声称已完成其私有 owner。Win64 类表与 next atom 将交给独立 owner，由窗口创建借用只读记录。

原创可选 `gateway-classes`：Win64 先注册一次，循环相同类的重复注册，最后注销并消费返回值；PE32 注册一次，循环 GetClassInfoW，消费 BOOL 和复制出的 style，最后注销。两端资源数量恒定，不使用持续创建类的循环。

旧 runtime 上六项独立测试通过（PE32/Win64 各三项），覆盖隐式控件、provider、菜单访存失败与 atom 提交、全局类查找、字段/名称错误、Win64 完整 instance、extra/menu 拒绝与字节长度限制；已有窗口生命周期回归继续保留。证据 `r2-classes-baseline{32,64}-tests.txt`。工具 20 项通过，原创 12K fixture 由旧冻结程序实际执行，Win64 与 PE32 IR/raw 均通过：Win64 retired=5n+19、work=6n+21、API=n+2、RAX=1；PE32 retired=11n+18、API=n+2、EAX=4n+1。该轮只核对正确性，不作计时结论，见 `r2-classes-fixture-baseline{32,64}`。

before 为 `r2-desktop-wire/wire`，SHA256 `97689a99eb05cbf37372cbce693d3af16e9caa64fdb5ae317753cb3f035d7383`，5,956,232 bytes。候选文件预算 +24KiB；证据根 `wire-out/refactoring-2026-10-14/`。

候选删除重复 PE32 ABI/名称分发与 Win64 arity/provider/名称分发，类记录/输出 handler 归入同一领域模块，Win64 表与 atom 归入私有 owner。PE32 公开兼容表继续保留，详见[架构说明](../architecture/classes.md)。首次编译发现父模块窗口创建仍需访问持久名称 helper 与 name_ptrs，已仅恢复父子模块内部可见性，未扩大公开 API；原失败日志 `r2-classes-candidate32-tests.txt` 保留。

首轮完整 release/debug 通过；显式 native 中六项通过，一项 swapchain fixture 仍直接调用已移除 class 路由的私有 `user_call`，导致注册返回 0。该测试的 setup 改为调用 typed class adapter，断言、窗口回调与 GPU 测试主体保持。公开 gateway 的六项独立基线此前已通过，不为旧私有测试入口恢复逐调用名称分发。失败日志 `r2-classes-native-tests.txt` 保留，修订后完整检查另存 `r2-classes-validation-v2-*`。

候选六项独立测试、已有类元数据一项和真实窗口创建/回滚一项回归通过，日志 `r2-classes-candidate32-tests-v2.txt`、`r2-classes-candidate64-tests.txt`、`r2-classes-metadata-tests.txt`、`r2-classes-window-tests.txt`。修订后完整 release 583 passed / 7 ignored，debug Win32/AMD64/FPU 429 passed，显式 native 7 passed，工具 20 passed；fmt、strict all-target/all-feature Clippy、release build 通过。

冻结 `r2-classes-wire/wire`：SHA256 `a5625b33fe03942673e5e7838914d6bda5d66669470d5dcdce55cd599805871c`，5,965,272 bytes，相对 before +9,040 bytes，满足 +24KiB 预算。365 个 Rust 文件（src/tests 357 个）的 live/frozen/manifest SHA 和文件集合一致，target 与 frozen executable SHA 一致，见 `r2-classes-source-verification.json`。正式计时与编译/native 检查严格串行。
