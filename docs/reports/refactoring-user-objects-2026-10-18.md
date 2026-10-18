# R2/R3 USER 注册名称与窗口属性（2026-10-18）

状态：十四项首版正确性通过，但 Win64 性能复测触发拒绝门槛，首版冻结不验收，游戏前缀尚未执行；保留日志并调查修订，不计入已验收累计数。

十四项范围：RegisterClipboardFormatA/W、RegisterWindowMessageA/W、SetPropA/W、GetPropA/W、RemovePropA/W、EnumPropsA/W、EnumPropsExA/W。PE32 前十项已有处理，四项枚举保留 ABI-only/error120/原清栈且不调用 callback；Win64 全部 absent。

名称保留原 clipboard-format/window-message 共享的进程逻辑 namespace、Unicode lowercase、非空与 255 UTF-16-unit 限制，以及包括 0xffff 的单调容量；已存在名称在表满后仍可查询。窗口属性保留 atom/name 区分、原大小写规则、空字符串名称、零值与窗口销毁生命周期。访存解码先于 HWND 检查，HWND 检查先于零 key 检查，LastError/trace/frame 顺序不改。

注册表已交给私有 RegisteredNames owner，每个窗口组合 Properties owner。属性键使用显式 atom/name 类型，替代原字符串前缀；枚举仍未实现，没有增加可观察的属性排序规则。受检 adapter 只借 User32、guest memory 与 LastError，不访问整个 ApiState；删除旧字段、重复 PE32 ABI 与名称分发，详见[架构说明](../architecture/user-objects.md)。

before 为 `r2-classes-wire/wire`，SHA256 `a5625b33fe03942673e5e7838914d6bda5d66669470d5dcdce55cd599805871c`，5,965,272 bytes。候选预算 +24KiB；证据根 `wire-out/refactoring-2026-10-14/`。

旧 runtime 上 PE32 四项、Win64 一项独立测试通过，覆盖解码/HWND/key 检查顺序、注册名共享与 Unicode lowercase、16384 个 atom 全容量及满表已有名称查询、属性零值/空名/键种类/销毁、枚举 ABI-only 清栈及 Win64 静态/动态 absent。日志 `r2-user-objects-baseline{32,64}-tests.txt`。工具 20 项通过。

原创可选 `gateway-properties` 使用一次 GetDesktopWindow、一次 SetPropA，循环 GetPropW，最终 RemovePropW 并消费返回值；仅一个逻辑窗口与属性，不进入 host 回调。旧冻结程序 12K PE32 IR/raw 均实际验证：retired=6n+19、API=n+3、EAX=42(n+1)。原默认性能 case 不变，见 `r2-user-objects-fixture-baseline32`，此轮不作计时结论。

候选五项独立测试与已有真实窗口/属性/注册名生命周期一项回归通过，证据 `r2-user-objects-candidate{32,64}-tests.txt`、`r2-user-objects-lifetime-tests.txt`。完整 release 588 passed / 7 ignored，debug Win32/AMD64/FPU 434 passed，显式 native 7 passed，工具 20 passed；fmt、strict all-target/all-feature Clippy 与 release build 通过。

冻结 `r2-user-objects-wire/wire`：SHA256 `4ca4c9efe2fd806c4a435ed6af7fceea23b3bab120108fca4f5f86f8600ea73a`，5,968,232 bytes，相对 before +2,960 bytes，满足 +24KiB 预算。370 个 Rust 文件（src/tests 362 个）的 live/frozen/manifest SHA 和文件集合一致，target/frozen executable SHA 一致，见 `r2-user-objects-source-verification.json`。正式 48 条路径三对矩阵使用该冻结程序，编译/native 检查均已结束。

首版 48 条路径结果全部校验，属性读取 PE32 IR/raw CPU 中位数 −15.504%/−12.361%。多个未改动 Win64 网关中位数回退超过 3%，八项追加相同 before executable 的 control 三标签、三对对照；线程 ID 查询补证 +5.809%，三对均超过 5%，因此拒绝首版，不用属性改善抵消。GDI profile 补证 +5.982%，其余对照与全部初筛保留于 `r2-user-objects-stable-final-summary.json`、`r2-user-objects-stable-amd64-controls-summary.json`。

静态调查单独在正式计时结束后执行：catalog_call 8208→8215 条指令，dispatch_bound 10749→10745；入口及内联 hot 路径的前部主要指令/栈帧相同，但函数地址改变。证据 `r2-user-objects-gateway-codegen/`；这些检查尚未确定回退的具体机制。后续尝试隔离原有完整分发函数的布局，保留算法和验证顺序；窗口查询草稿仍未接入。
