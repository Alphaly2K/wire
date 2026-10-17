# R2/R3 BSTR（2026-10-17）

## 独立旧行为基线

起点 `84e7022`，before 为 `r2-winrt-wire/wire`，SHA256 `6eb15b034181ae17bb97fb26d5f4e389d89f9d62767920640ce9ded4baa35e55`。范围八项 BSTR export，PE32 七项 typed、一项 byte allocator ABI-only，Win64 八项 typed。

基线提交 `2050d81`，五项新增回归在旧实现全部通过，固定 ordinal/provider、foreign byte prefix、完整指针/DWORD、strict membership、heap 共享与失败 free、上限/LastError/输出保留、allocation 输出重叠及返回槽别名。证据 `r2-bstr-baseline-tests.txt`。

新增可选 `gateway-bstr` 原创 fixture，每轮对同一 guest prefix 取字符长度和字节长度，checksum 消费两次结果，无资源增长。双架构与 PE32 raw/static IR 均在旧 executable 完成 12K 预检，分别核对 checksum/退休数/API 次数，Win64 另核对 work，证据 `r2-bstr-fixture-baseline64`、`r2-bstr-fixture-baseline32`。

## 实现

八项 export 接入 BstrApi，Win64 ordinal aliases 移入同一契约模块，删除重复 arity/DWORD/provider、legacy bit 路由及调用时名称判断。PE32 adapter 只借明确资源；Bstrings32 私有登记成员，Variant/WMI 内部 facade 共用同一分配，保持删除成员/heap release 的先后与原错误处理。共用 UTF-16 reader 迁到窄 text helper。

Win64 Bstrings 私有拥有成员/base/长度，release 和 replacement 检查只借受检 Memory/backing map，分配仍由原 helper 协调。保留两架构重分配发布、free、返回帧及 LastError 的差异，详见 [BSTR 边界](../architecture/bstr.md)。迁移后全部 Win32/Win64 领域回归通过。

## 完整正确性与冻结

完整 release **519 passed / 7 ignored**、debug Win32/AMD64/FPU **366 passed**、显式 native **7 passed**、工具 **20 passed**，fmt、strict all-target/all-feature Clippy 和 release build 全部通过。冻结 `r2-bstr-wire/wire`，SHA256 `43c225f5103d14f867c07ff81d88300a652e9b256a7de099e685b133f8bb00dc`，**6,001,176 bytes**，增加 **3,664 bytes**。

冻结 manifest 的 **300 个 Rust 文件**（src/tests **292**）、合计 **399 个源码文件**逐项核对一致，target 与冻结 executable 哈希一致，证据 `r2-bstr-source-verification.json`。

## 性能与接受决定

稳定启动协议下 **53 条初筛路径**各三对全部结果校验，RSS 最多增加 **16KiB**。新增 BSTR 查询网关 CPU Win64 **−1.027%**、PE32 IR/raw **−1.331%/−1.912%**；该负载只覆盖可读 foreign prefix 的两个查询，没有测量 owning allocation、reallocation 或 Variant 的总吞吐。

PE32 SSE/raw 初筛 **+3.715%**，三对为 −1.672%/+3.715%/+0.255%；integer/IR 初筛 −9.962%，但一对 +6.592%，integer/raw 也有两对超过 +5%。三项按三倍 work、直接引用 before 原路径的同哈希 control 与三种顺序各一对扩样。完整 `r2-bstr-stable-summary.json` 保留 **56 个 workload protocol**，不同 work 不合并：

| 路径 | work | after/before CPU | control/before CPU | after/control CPU |
| --- | ---: | ---: | ---: | ---: |
| PE32 integer/IR | 900M | +0.003% | +0.018% | −0.015% |
| PE32 integer/raw | 900M | +1.168% | −1.085% | +2.277% |
| PE32 SSE/raw | 900M | +2.871% | +0.396% | +2.465% |

SSE/raw 三对为 **+2.274%/+3.473%/−0.129%**，保留扩样中位数 **+2.871%** 的局部成本；未跨工作量合并或用 integer/IR 的初筛负值宣称收益。其余初筛路径中位数均低于 3%，全部轮次保留。

Alice 200M 三对均保持预算出口、隐藏 **314×211** 窗口及 Draw/Present **0**，CPU **+0.028%**、wall **−0.325%**，原 runtime 直接复用。该前缀尚未实际绘制；编译、正式计时与 native 游戏串行，证据 `r2-bstr-stable-alice`。

接受八项 export，契约累计 **348 项**。不存在三对均回退超过 5% 的路径，语义与资源回归保持；历史线程控制批 SSE **+3.473%**、线程属性批 HeapSize **+4.063%**、日历批 Local/Global **+4.570%** 等继续按各自基线保留。后续 Variant 独立基线草稿尚未注册到 target，不属于本批验收，Mono 继续排除。
