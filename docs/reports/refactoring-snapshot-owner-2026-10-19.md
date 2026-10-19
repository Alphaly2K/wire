# Win64 Toolhelp 快照状态归属

状态：正确性、性能及启动前缀验收完成，保留别名路径局部成本和 raw 漂移。直接运行时基线为 `a02cb28` token owner；三个独立旧行为测试和原创 fixture 先提交于 `0962939`。本批保留七项 legacy API 入口，累计 catalog export 仍为 **706**。

私有 `snapshot::Snapshots` 拥有记录、单调句柄、handle flags 和枚举游标，单个 Snapshot 及其字段全部收为私有。Runtime64 只提供 guest 文件名与未退出线程的 `(id, priority)` 迭代器，在原错误优先级及容量检查通过后才复制；枚举仅借 Memory、参数、原名称及当前线程 LastError 地址。通用句柄查找、保护关闭和提前提交仍在原协调边界，编码函数除可见性外逐字保持。详见[快照边界](../architecture/snapshots.md)。

三个独立测试先在旧实现通过，覆盖 DWORD flags/完整句柄、128 个存活与 4096 个累计编号、保护关闭错误 5、无效句柄先于 output、ANSI/wide 共用游标、size read guard、失败 First 不重置线程/进程游标、saved frame，以及创建后线程/优先级变化不影响快照。迁移后 release/debug 定向各 4 项通过；完整 `r3-snapshot-validation-*`：release **653 passed / 7 ignored**，debug **499**（Win32 237、AMD64 251、FPU 9、日志单元 2），显式 native GPU **7**、工具 **31**，fmt、strict all-target/all-feature Clippy、SIMD crate 检查与 release build 通过。

原创 gateway-snapshot 在循环前创建一个 process 快照，每轮真实 Process32FirstW copyout 并消费 BOOL、PID、dwSize，最后关闭并消费 BOOL。n=max(1,work/12)，RAX=570n+1、retired=11n+19、work=12n+21、API=n+2；单次和 12K 输入先在旧冻结校验，inventory 固定三项独立 kernel32 导入。150M 循环 CPU 中位数 **+0.864%**。

冻结 `r3-snapshot-wire/wire`：SHA256 `24e20ceaddf69dba59f7fb12c80b3923079fe13c6b56c3831fda403071a6859e`，**6,053,800 bytes**，较 token **−18,896 bytes**，较两批之前 ETW **−9,360 bytes**。302 个 src Rust 文件与 target/frozen/manifest 核对一致，仅 runtime.rs/snapshot.rs 改变；证明 `r3-snapshot-source-proof.json`。没有扩大缓存或资源限额。

三标签、同旧 binary control、stable launch/fixed executable slot，共 **19 条协议、180 次执行**全部校验；常规 AMD64/PE32 为 utility，V2 保持独立默认 QoS。初筛 integer/memory 的 **+5.029%/+6.110%**，追加 900M 后为 **−2.772%/+0.294%**；SSE 长样本 **−0.642%**。其余计算/网关初筛 −4.022% 至 +2.409%。data-alias 初筛 **+3.735%**，900M 复核 **+2.959%**，相对 control **+1.927%**，保留该局部成本。fixture RSS 中位数最多增加 **131,072 bytes**。

PE32 integer/raw 初筛 **+3.686%**；首轮 900M **+10.789%** 同时出现同旧 binary control/before **+11.414%**，after/control **−0.560%**。保留首对 +14.658%、第二对 +5.715% 及第三对 −2.921% 的完整记录。追加三对 900M 为 **+1.577%**，相对 control **−0.678%**；同协议六对汇总 **+2.391%**，相对 control **+1.338%**，没有三对均超过 5% 的持续回退。20 份 PE32 CPU/memory/IR/SIMD 源码保持，但编译器重生成部分执行器：raw 节 2238→2240 条、IR32 2203→2218 条；mix32、AMD64 两个独立 IR 节助记符保持。未比较全部操作数，具体性能机制尚未确证。全部慢轮和新复核独立保存于 `r3-snapshot-performance-*`，汇总 `r3-snapshot-performance-summary.json`，静态调查 `r3-snapshot-core-codegen-investigation.json`。

针对 token 单批曾记录的 PE32 integer/IR +3.320%，另以 ETW 冻结直接比较本批最终 executable，三标签三对 900M、**9 次执行**得到 CPU 中位数 **−0.061%**。这组输入/源 SHA 与结果独立保存在 `r3-token-snapshot-cumulative-pe32*`；不与单批 before 混合汇总，原 token 阶段成本记录保持。

三对 Alice 200M 在可访问 Metal/AppKit 的宿主环境全部到相同预算、hidden 314×211、Draw/Present 0/0。CPU/wall 中位数 **−0.247%/−0.598%**，RSS 中位数 **+409,600 bytes**。复用原游戏素材及其 SHA，解释器临时软链接退出删除；仍是未进入绘制的启动前缀，不作为游戏 FPS 结果。证据 `r3-snapshot-performance-alice`。
