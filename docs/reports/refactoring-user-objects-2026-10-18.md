# R2/R3 USER 注册名称与窗口属性（2026-10-18）

状态：五项独立旧行为测试与原创性能 fixture 已通过，候选草稿尚未接入，不计入已验收累计数。窗口类矩阵、补证与游戏已结束，本批在其后串行验证。

十四项范围：RegisterClipboardFormatA/W、RegisterWindowMessageA/W、SetPropA/W、GetPropA/W、RemovePropA/W、EnumPropsA/W、EnumPropsExA/W。PE32 前十项已有处理，四项枚举保留 ABI-only/error120/原清栈且不调用 callback；Win64 全部 absent。

名称保留原 clipboard-format/window-message 共享的进程逻辑 namespace、Unicode lowercase、非空与 255 UTF-16-unit 限制，以及包括 0xffff 的单调容量；已存在名称在表满后仍可查询。窗口属性保留 atom/name 区分、原大小写规则、空字符串名称、零值与窗口销毁生命周期。访存解码先于 HWND 检查，HWND 检查先于零 key 检查，LastError/trace/frame 顺序不改。

计划将注册表交给私有 RegisteredNames owner，每个窗口组合 Properties owner。属性键使用显式 atom/name 类型，替代原字符串前缀；枚举仍未实现，没有增加可观察的属性排序规则。

before 为 `r2-classes-wire/wire`，SHA256 `a5625b33fe03942673e5e7838914d6bda5d66669470d5dcdce55cd599805871c`，5,965,272 bytes。候选预算 +24KiB；证据根 `wire-out/refactoring-2026-10-14/`。

旧 runtime 上 PE32 四项、Win64 一项独立测试通过，覆盖解码/HWND/key 检查顺序、注册名共享与 Unicode lowercase、16384 个 atom 全容量及满表已有名称查询、属性零值/空名/键种类/销毁、枚举 ABI-only 清栈及 Win64 静态/动态 absent。日志 `r2-user-objects-baseline{32,64}-tests.txt`。工具 20 项通过。

原创可选 `gateway-properties` 使用一次 GetDesktopWindow、一次 SetPropA，循环 GetPropW，最终 RemovePropW 并消费返回值；仅一个逻辑窗口与属性，不进入 host 回调。旧冻结程序 12K PE32 IR/raw 均实际验证：retired=6n+19、API=n+3、EAX=42(n+1)。原默认性能 case 不变，见 `r2-user-objects-fixture-baseline32`，此轮不作计时结论。
