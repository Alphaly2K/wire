# R2 基础字符串、指针与进程查询（2026-10-18）

状态：首版候选因性能回退拒绝；修订版正在验证，尚未验收。

## 独立旧行为基线

起点 `0c136ad`，before 为 `r2-guid-allocation-wire/wire`，SHA256 `ec45ed673a43da225fb3f78541e4941e7d6bd19f1f4fc6ddc1e0d8`。范围为基础字符串 14 项、指针/cache flush/MulDiv 5 项、进程与处理器查询 10 项。

独立基线提交 `a535d81`，四项 PE32、两项 Win64 回归固定重叠字符串、signed count、边界读取失败输出、单 UTF-16 unit uppercase、null/count、权限与错误顺序、伪句柄、输出/返回槽别名、处理器 group carrier、完整 Win64 trace carrier、零长度指针检查及 MulDiv 的失败返回与 LastError。首次测试误将裸 `lstrlen` 当作未知 ABI，核对原 Wine spec 后纠正预期；它原本就是 4 字节 stdcall，运行时行为未变。原失败与正确结果分别保留在 `r2-basic-baseline-tests.txt` 和 `r2-basic-baseline-tests-corrected.txt`。

新增原创 `gateway-processor-count`（双架构）与 `gateway-basic-string`（PE32）可选负载，旧 executable 的 Win64 与 PE32 raw/static IR 12K 预检均核对 checksum、退休数、API 次数及 Win64 work，证据 `r2-basic-fixture-baseline64/32`。字符串负载不代表整个文本或 NLS 领域的吞吐。

## 实现与正确性

29 项 export 分别汇入 BasicStringApi、PointerHelperApi、ProcessQueryApi，删除混合 `common_api.rs` 模块、重复手写 ABI 和 Win64 名称/arity 分支。PE32 字符串及指针适配器只借明确参数与受检内存；进程适配器借句柄视图和调度器，OpenProcess 返回资源请求，由原入口分配句柄。WMI 内部 BSTR 分配直接借同一 Bstrings32 owner、进程 heap 与内存。具体输出、错误和资源顺序见[基础 helper 边界](../architecture/basic-helpers.md)。

Win64 只绑定原有四项 processor/group-count API；PE32 八项 process API typed，两项 group-count 仍为 ABI-only，字符串和指针家族在 Win64 保持 absent。新增 Uint16 元数据保留原始 carrier，由适配器消费低 WORD；PE32 继续检查完整 DWORD。参数和 ApiId 新变体追加在 enum 尾部，保留既有变体顺序。

领域回归 Win32 **175**、Win64 **196** 项通过。完整 release **533 passed / 7 ignored**、debug Win32/AMD64/FPU **380 passed**、显式 native **7 passed**、工具 **20 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。

冻结 `r2-basic-wire/wire`，SHA256 `b3f515acb34f8572a34bb83f986509ea113f55b7b605670087f5bdb070e2b97a`，**6,021,864 bytes**，较 before 增加 **104,720 bytes**。冻结时 **314 个 Rust 文件**（src/tests **306**）逐项与活动源码和快照核对一致，target executable 与冻结哈希一致，证据 `r2-basic-source-verification.json`。

## 性能与接受决定

正式比较使用稳定启动协议，编译、计时、native 游戏与 runtime diagnostics 串行。首版已完成的 AMD64 CPU/gateway/V2 共 28 条初筛路径全部结果校验；其中 code-alias CPU 中位数 **+6.355%**，三对分别 **+6.178%/+8.447%/+6.071%**，触发三对均超过 5% 的拒绝门槛。停止尚在执行的 PE32 批次，已有完整和部分日志均保留为 `r2-basic-stable-*`，不将部分矩阵记为通过。该冻结只作为被拒候选，不纳入已验收 export 总数。

独立体积调查发现五份 Wine ABI spec 从一份变成两份，重复数据合计 **97,520 bytes**，是首版 104,720-byte 增量的主要部分；`__TEXT,__const` 增加 98,816 bytes，而 `__TEXT,__text` 减少 6,680 bytes。证据 `r2-basic-spec-duplication.json`。这解释数据体积增量，尚未确证 code-alias 回退的具体机制。

修订版将 ABI spec 收敛为同一静态来源，ordinal 与 stack-table reader 共用，保留五个 ordinal provider、解析顺序和类型/架构过滤策略。完整 release **533/7 ignored**、debug **380**、显式 native **7**、工具 **20** 及 fmt/strict Clippy/build 再次通过。

修订版冻结为 `r2-basic-canonical-wire/wire`，SHA256 `35aad1fd6502c0db7b2ba5368d21ef01a5891618db37074976c037a72360f949`，**5,917,192 bytes**，较 before 只增加 **48 bytes**；上述五份 spec 在 executable 中均为单份。314 个 Rust 文件及 target/frozen executable 哈希重新核对一致，证据 `r2-basic-canonical-source-verification.json`。四项 V2 先行复测仍出现 code-alias **+9.254%**，三对 **+9.162%/+9.523%/+9.175%**，因此也拒绝该冻结，不继续其余长矩阵或游戏计时。

独立 diagnostics 中两版 IR **120,008**、fallback **0**、translations **20**、invalidations **0**，翻译原因计数也相同。顺序 sample 将恢复/保存缓存与 CLI 内联执行循环定位为主要热点；CLI 循环的指令数及助记符哈希相同，但 `reuse_translation` 从 **434 → 449** 条指令，栈帧从 **240B → 352B**，反汇编显示额外的临时 aggregate 复制。保存函数从 **225 → 242** 条指令。首个 codegen 脚本错误将 save 的 OnceCell 初始化 helper 覆盖到同名结果，正确 save 函数另保存在 `cache-save-corrected.asm` / `save-corrected-summary.json`，不沿用首次 save 统计。证据 `r2-basic-canonical-alias-diagnostics`、`r2-basic-canonical-alias-profile` 与 `r2-basic-canonical-alias-codegen`。

单独将 `take().unwrap()` 改成 Option 传播后机器码未变，七项既有 memory 回归通过，未另跑正式计时。随后按字段解构恢复的 SavedTranslation，保留 borrow 释放后保存 victim、两页 generation 验证、identity wrap、segment owner 移动和诊断顺序。该候选的恢复函数为 **432** 条指令、**208B** 栈帧，完整 release **533/7 ignored**、debug **380**、显式 native **7**、工具 **20** 及 fmt/strict Clippy/build 再次通过。

当前冻结 `r2-basic-fields-wire/wire`，SHA256 `b6ac77e4eff26b9dc39540dfa69ee823383728f2105452db5f700b5ef4804b79`，仍为 **5,917,192 bytes**，增量预算为相对 before 不超过 16KiB，实际 **+48 bytes**。314 个 Rust 文件和 target executable 核对一致，证据 `r2-basic-fields-source-verification.json`；随后新增的键盘独立基线及未接入实现草稿不属于该冻结或通过计数。

最终候选的 **68 条初筛路径**各三对全部结果校验，初筛 CPU 中位数最大回退 **+4.205%**，RSS 中位数最多增加 **128KiB**。code-alias 中位数 **−5.378%**，三对 **−5.618%/−5.378%/−4.574%**，原持续回退消除。新处理器查询网关 PE32 IR/raw **−0.048%/−1.009%**，新字符串网关 IR/raw **−1.066%/−0.762%**。

memory、SSE、bit、GUID、线程控制、PE32 COM/IR 和 critical-section/IR 共七条路径因中位数超过 3% 或单轮回退超过 5% 而扩样，三倍 work、同哈希 control 和三种顺序各一对。扩样及 Alice 前缀尚在执行，最终接受决定待完成后记录。初筛汇总 `r2-basic-fields-stable-initial-summary.json`。
