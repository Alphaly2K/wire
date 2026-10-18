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

完整 dispatcher 独立 4KiB 对齐节候选 `r2-user-objects-gateway-layout-wire/wire` 体积保持 5,968,232 bytes，三项网关回归通过；700M work、相同 before control 的五项早期筛选中，thread-id +6.133%、GDI profile +5.563%、desktop +5.410%，memory +5.509%、SSE +1.239%。线程查询仍三对均超过 5%，该修订也拒绝，linker 与运行时属性已全部撤回；不继续其完整检查或游戏计时。证据 `r2-user-objects-gateway-layout-early-summary.json`。

正式计时之后串行抓取旧版/首版各 3 秒 sample，实际运行均校验相同线程查询输入，证据 `r2-user-objects-profile/`。进一步反汇编发现 Trace::call_bound 从 142→152 条指令、frame 从 192→224B，首版在旧记录 drop 前构建完整 64B 新记录，随后出现额外 aggregate 复制。新修订提前拥有借用参数，在旧记录淘汰后组装完整记录，保持名称/参数所有权、4096 条容量与输出顺序；独立候选验证中，不预先宣称消除回退。

先调整构造顺序的 `trace-fields` 候选五条 700M control 对照结果：thread-id +2.855%、GDI profile +2.731%、desktop +4.859%、memory +4.611%、SSE +2.940%，全部轮次保留，尚未扩展完整验收。继续将原 0–4 个 trace 参数的动态 copy_from_slice 改为有界值复制，移除小参数路径的通用 memcpy，长参数继续拥有原 Box 存储。现有 0–16 参数、源参数后续修改、格式和容量/顺序测试继续通过。

`trace-scalar` 早期三对含同哈希 control：三条 Win64 网关按 100M work、memory/SSE 按 700M；thread-id/GDI profile/desktop CPU +0.925%/+2.287%/+1.896%，memory/SSE +3.278%/+1.639%。逐对和 control 成本保留于 `r2-user-objects-trace-scalar-early-summary.json`，memory 仍需调查补证，不将其它路径的改善抵消该成本。

最终修订冻结 `r2-user-objects-trace-final-wire/wire`，SHA256 `d1bbdf56abbdd2a83ddc487dd0c0ed3a461d6ef402dc5356cefe56ed7892c0e3`，5,969,080 bytes，较 before +3,808 bytes；与早期 scalar executable 哈希相同。完整 release 594 passed / 7 ignored，debug 440，显式 native 7，工具 20；fmt、strict Clippy 和 build 通过。375 个 Rust 文件（src/tests 367 个）与 target/frozen executable 核对一致，证据 `r2-user-objects-trace-final-source-verification.json`。首次 fmt 检查仅发现 trace 构造排版，原日志保留，最终检查为 `r2-user-objects-trace-final-fmt-v2.txt`。

正式 48 条路径矩阵全部结果校验，属性 IR/raw −17.143%/−13.694%；thread-id、GDI profile、rectangle 和 keyboard 出现 +193.579%/+140.224%/+123.683%/+103.720%，该独立冻结仍未验收。两个同哈希副本和重复采样未复现数倍成本；固定 executable 路径与 cwd 的三对诊断差异回到约 2%，见[测量调查](refactoring-fixed-executable-slot-2026-10-18.md)。保留全部旧协议轮次，未执行该冻结的 Alice 验收。

该冻结包含窗口查询独立测试与未接入的三个草稿文件；核对完成后 live 源码才接入查询路由。后续将以 Classes 为共同 before，对名称/属性、trace 修订和二十五项查询的集成状态执行完整检查、固定路径矩阵与游戏前缀。各家族独立旧行为回归保留；集成候选通过之前累计仍为 501。
