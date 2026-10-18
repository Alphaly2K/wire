# 精确标量浮点编译边界候选

状态：独立结构候选，尚未验收。对话框的 22 项迁移完整 release 614 / 7 ignored 通过，5,974,936-byte executable 的 AMD64 FP 三对仍持续回退约 25%；该实现与绘制拒绝候选分别冻结，没有互相覆盖。

绘制候选已撤回，运行时 SHA 精确恢复消息冻结。随后独立对话框迁移再次复现相同的 APFloat 编译布局问题，说明它会继续阻碍无关领域的重构。本候选为已有纯语义模块建立实际编译隔离，取代继续枚举 root crate 的 CGU、LTO 和内联组合。它是计划“先证明模块依赖方向，再评估编译隔离收益”的有限调整，未把全仓拆为多个 crate。

`crates/wire-simd` 只依赖原有 iced Mnemonic 与原有 rustc_apfloat，拥有 MXCSR/RFLAGS 两字段运算事务；没有内存、CPU、IR、Win32、宿主或资源依赖。原 `simd.rs` 的 scalar 语义原样迁入私有 state 模块，根 SIMD 模块保留整数向量操作并重导出 scalar 类型。Single/Double 的封闭静态 trait 分发进入本库中的具体非泛型入口，没有 trait object 或运行期查表。库内编译单元固定为 1，根 crate 与其他依赖保持原 profile。

具体入口禁止跨库内联，原内部泛型语义内联到这些入口，使 APFloat 的大单态化实例留在本库。调用者仍按原位置完成源访存，提交 MXCSR、目标、FLAGS、RIP 与退休计数；未改变 DAZ、FTZ、NaN、舍入、掩码或异常规则，没有增加常量特判或宿主浮点捷径。该边界需要实测支持，并不预先承诺代码体积或速度。

候选与已接入的对话框迁移一起验证，二进制总增量仍使用先前 +24KiB 上限。先运行真实 FPU/Unicorn 与 scalar descriptor 提交边界，再冻结三标签 300M FP 筛查；通过后补齐 debug、native、strict Clippy 以及受影响解释器/网关和相同游戏前缀。冻结显式包含新库 Cargo.toml 与两个 Rust 源文件。失败轮次全部保留，不将拒绝的绘制六项重新计入范围。

首轮 FPU 9 项和 scalar descriptor 提交边界通过，冻结 `r2-scalar-isolation-wire`，SHA256 `db40ea3609cb42356759777dcc1314c08be5dc2cf7e7401783f5d608b22bffde`，5,992,408 bytes，较已验收消息基线 +21,968 bytes，满足 +24KiB 上限。395 个 Rust 文件和新库 manifest 核对一致。还独立核对原 scalar 文本：除可见性、函数名及内联属性外完全相同，整数向量代码逐字相同，证据 `r2-scalar-isolation-semantic-source-comparison.json`。

300M 三标签 FP 三对结果全部校验，CPU 中位数相对消息冻结 −7.400%；同哈希 control 单独保留，未与 before 混算。本结果只用于通过定向筛查，完整正确性、宽矩阵和游戏前缀仍待结束，尚未正式采用。

完整 release 614 passed / 7 ignored、debug 460、显式 native 7，以及 root/library fmt、strict Clippy、release build 全部通过。395 个 Rust 文件、内部库三个输入和 target/frozen executable 再次一致，验证 `r2-scalar-isolation-complete-verification.json`，同时保存实际 rlib hash。初筛 28 条协议 / 171 次执行全部结果校验；AMD64 integer/memory/SSE 中位数 −3.350%/−4.722%/−2.489%，PE32 FP IR/raw −0.976%/−4.246%，这些是结构候选的性能观测。

七条初筛超过调查门槛，安排 900M CPU 和 150M gateway 的三标签补证。code-alias 已完成；随后宿主出现严重波动：900M bit/IR 的同哈希 before/control CPU 从 6.8056 秒到 15.9605 秒，after 也从 8.3989 到 15.4843 秒。SSE/raw 首组 control/after 为 29.7707/29.7202 秒，但下一 after 达 48.7654 秒，wall 77.6224 秒。该轮在退出 130 后停止其余计时，未执行后续 gateway 或正式游戏计时；保留 `r2-dialog-long-*` 的全部部分结果与 manifest，不将不完整协议纳入汇总。

目前所有正确性检查通过，性能仍待复核。初筛、完整的 code-alias/bit 扩样和未完成的 SSE 分别保留，bit 的大幅漂移数据不用于验收。宿主检查未发现残留编译或 Wire，观察到桌面 renderer、WindowServer 与用户应用活动；不改动这些应用。接下来可补充与当前 runtime 字节无关的下批行为基线，待宿主负载平稳后再继续性能准入。

计时退出后，菜单四项独立基线通过，确认 RedrawWindow 的真实 gateway 遮盖；root/src/crates、Cargo 输入及 executable SHA 与本冻结仍相同，证据 `r2-menu-baseline-runtime-identity.json`。这四项新测试不包含在原 395 文件来源核对中。随后串行执行 Alice 新旧各一次 200M 功能 smoke，均到相同预算、hidden 314×211、Draw/Present 0/0；临时二进制软链接退出后删除，复用现有 runtime，未复制或硬链接游戏资源。该 smoke 明确仅作功能检查，不用于宿主漂移期间的性能准入。

再次仅复核 900M bit/IR，三标签三对全部结束且结果校验通过，`r2-dialog-recheck-bit-ir` 和独立 metrics 保留。候选 CPU 中位数 −0.420%，但 before/control 同哈希仍从 5.1517 秒漂移到 8.4842 秒，首对 control/before 约 +34%；本轮仍不作为性能准入。计时退出后才编辑菜单新增测试。为继续职责迁移，后续菜单候选将独立冻结，并单独对照本报告冻结；本候选与消息基线之间未解决的性能验收继续保留，累计已验收仍为 595。

菜单候选完成完整正确性并冻结后，再次独立使用原消息/本报告冻结，串行完成此前未结束的 900M SSE/raw、150M 三项 IR gateway 和 GUID/raw 三标签三对。原完整 code-alias、最新 bit 及本次四项 gateway/SSE 与最初矩阵一同保留为 `r2-dialog-all-completed-metrics.json`：35 条完整协议、234 次校验通过的执行，不汇入此前中断的 SSE 或漂移严重的旧 bit 扩样。不同 work、脚本与协议均保留独立分组。

扩样 CPU 中位数：code-alias −6.360%、bit/IR −0.420%、SSE/raw +5.353%、dialog/IR −11.545%、messages/IR +3.925%、GDI/IR +4.105%、GUID/raw +0.337%。SSE/raw 相对同哈希 control +2.413%，逐对相对 before +15.540%/+5.353%/−5.733%；messages/IR 相对 control +4.505%，逐对相对 before +6.193%/+3.925%/−3.320%；GDI/IR 相对 control +2.064%，逐对相对 before +4.219%/+1.169%/+5.862%。同哈希噪声与候选的局部成本均保留，SSE/raw、messages/IR、GDI/IR 继续调查，未作性能准入。

前后 api::dispatch 的独立反汇编保存为 `r2-dialog-api-dispatch-{before,after}-assembly.txt`；总函数指令与局部栈布局发生变化，后续按实际消息/图形路径定位，尚未据此确定热点原因。随后串行完成 Alice 200M 三对，六次均到同一预算出口、hidden 314×211、Draw/Present 0/0；CPU/wall 中位数 −3.906%/−3.967%，RSS +160KiB。证据 `r2-dialog-final-alice`，复用已有 runtime，二进制软链接在每次退出后删除。此启动前缀未进入实际绘制，不推断游戏 FPS。
