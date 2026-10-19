# comctl32 初始化与窗口类边界（2026-10-19）

状态：两项初始化已接入既有 class 契约与 typed 路由，完整正确性通过；性能待宿主负载稳定后复核。此前五项 paint 已提交 `79f1df4`，两批共七项新增接入，正式性能验收总数仍为 675。

范围为 InitCommonControls 与 InitCommonControlsEx。DLL、签名和执行策略合入 `win32/catalog/classes.rs`，class 声明从原十七项增加到十九项；原十七项的 provider、参数和架构范围保持。初始化只接受 PE32 comctl32.dll，Win64 继续 absent。总分发器移除两项重复 ABI 和控件注册循环，直接使用已有 ClassApi 路由；领域实现归入 `user32/classes.rs`，仍复用原 register_system_class。

独立基线提交 `166b400`，在切换实现前 PE32 两项、Win64 一项通过。固定 Ex 无效大小的 error 87/空 note/implemented trace，guest pointer 故障前无类注册及正常 API 提交，错误 DLL 的未知 ABI/零清栈/未实现诊断，以及仅读取 dwSize、忽略 dwICC 的现有行为。将记录置于已提交区最后四字节，下一页未提交，验证 Ex 没有新增整结构预检。

另一项先隐式注册 Button，再初始化全部十个基本类，验证原类记录不被覆盖、注册顺序、重复调用不分配新类/持久名称、成功 LastError 保留和原 note。InitCommonControls 虽然在 Windows 为 void，这里仍保留原 scalar return 1；本次不借重构修正可观察行为。Win64 及错误 DLL 没有新增别名。证据 `r2-common-controls-baseline.txt`、`r2-common-controls-focused.txt`。

新增可选原创 `gateway-common-controls` fixture，每轮实际调用两个初始化入口并累加其返回值，资源数量有界。n=max(1,work//7)，EAX=2n、retired=7n+7、API=2n，最终 int3 不计入退休数。Ex 使用 dwSize=8、dwICC=0xffffffff，保留已有 flags 行为。旧 fixture 与默认 case 集合保持。

鉴于本轮 Unity/WindowServer 活动及同哈希漂移，不启动新的长性能对照。保留完整正确性与小规模功能验证，待后台负载稳定后针对五项 paint 和两项初始化共同复核；不将短样本当成性能证据。二进制增量预算相对 `r2-paint5-wire/wire` 为 +24KiB。

迁移后 class 专项 PE32 五项、Win64 四项全部通过。完整 release **631 passed / 7 ignored**；debug Win32 229、AMD64 237、FPU 9 及日志单元 2，合计 **477 passed**。显式 native 图形 **7 passed**，工具 **26 passed**；fmt、strict all-target/all-feature Clippy、SIMD crate 静态检查及 release build 通过，日志 `r2-common-controls-validation-*`。

冻结 `r2-common-controls-wire/wire`，SHA256 `d1c17a558a6a35ff5b450daa2fd08a2380717ab4383c2fab3d23673fdf0c3362`，6,020,104 bytes，较 paint 冻结增加 **1,344 bytes**。290 个 src Rust 文件及 target/frozen binary 一致；只有 `api.rs`、`user32/classes.rs`、`win32/catalog/classes.rs` 三个运行时源码变化，未修改 CPU/IR/cache/SIMD，核对 `r2-common-controls-source-proof.json`。

新 fixture 先在旧 paint 冻结上以 n=100 验证 IR/raw，EAX=200、retired=707、API=200 全部符合独立计数。再做三对 12K 功能对照：PE32 新初始化、原 class 查询、paint 和 FP 的 IR/raw，另加 Win64 class gateway，共 **9 条协议、54 次执行**通过；证据 `r2-common-controls-fixture-baseline`、`r2-common-controls-functional-{pe32,amd64}`。这些耗时不作性能结论。

一对 Alice 200M 功能启动前缀均到同一预算出口，hidden 314×211、Draw/Present 0/0，无新增 API/访存错误。复用原游戏 runtime 与已锁定输入，解释器软链接退出删除；证据 `r2-common-controls-functional-alice`，不作时间或 RSS 准入。
