# R2/R3 线程控制（2026-10-17）

## 独立行为基线

起点 `3bd446a`，before 为 `r2-thread-properties-wire/wire`，SHA256 `8bf857f2dbfe1953bc9e841b4da127c6508395531e5bb7335e9d816d5f9fb39f`。范围 CreateThread、ExitThread、QueueUserAPC、GetThreadContext、SetThreadDescription、GetThreadDescription 六项 export。

独立基线提交 `46bec23`，新增八项迁移前回归通过。PE32 固定创建忽略参数、flags/线程 ID、晚输出 fault 已提交资源、返回槽别名、参数/EAX 两种退出，以及四项 ABI-only 和错 provider 清栈。Win64 固定 Unicode description/LocalFree/HRESULT/LastError、输出与返回帧别名、guard/分配先后、scheduler admission、完整 SIZE_T、失败 mapping 回滚/ID 不消耗、security、DWORD flags、CONTEXT 错误/trace/control 提交、APC 容量/权限/完整参数/整队列预检、退出和错 provider。证据 `r2-thread-control-baseline-tests.txt`。

新增可选 `gateway-thread-control` 性能 fixture，循环请求无效目标 APC，末尾读取 LastError，独立核对结果、退休指令和 API 次数，Win64 另核对 work。Win64 和 PE32 的 raw/static IR 均在旧 executable 验证，9800 work 预检证据 `r2-thread-control-fixture-baseline64`、`r2-thread-control-fixture-baseline32-fixed`。初次 PE32 fixture 将不退休的 INT3 多算一条，保留失败记录；修正预期后通过，迁移前提交已经包含修正。

## 实现

六项 export 接入唯一 ThreadControlApi 声明，删除 PE32 重复 ABI、Win64 arity/DWORD 和根名称分支。PE32 只有 Create/Exit 为 typed，其余保留 Wine ABI 已知但 error 120；内部 wire worker-return stub 单独保留。Win64 冷绑定 ControlImport::Thread，Value/Exit/Context 分别由原进程提交规则处理，不引入逐调用字符串解析。

Threads 私有拥有 description/APC queue，负责身份/状态校验、描述读取成功后替换、APC 入队及全部 callback 校验后取出。Runtime64 保留 heap/LocalFree 分配协作和创建资源编排；Process64 执行 guest callback 和 CONTEXT 保存寄存器读取。PE32 的晚输出提交及两架构的创建/退出顺序保持，见[调用与 owner 边界](../architecture/thread-control.md)。

## 正确性与初筛

迁移后八项独立回归及完整 release **502 passed / 7 ignored**、debug Win32/AMD64/FPU **349 passed**、显式 native **7 passed**、工具 **20 passed** 均通过，fmt、strict all-target/all-feature Clippy 和 release build 通过。冻结 `r2-thread-control-wire/wire`，SHA256 `fbe7f6ab88066a4a536327df97a7dd94a02099990a04af111bb779dbc79ef532`，**5,995,816 bytes**，增加 **22,688 bytes**。

稳定启动协议下 46 条初筛路径各三对全部结果校验，RSS 最多增加 **160KiB**。新增 APC 错误返回网关 CPU Win64 **+0.489%**、PE32 IR/raw **−23.845%/−19.707%**；PE32 的 ABI-only 提前路由省去后续 legacy 分发。该样本循环无效目标，不覆盖有效 APC 执行的总吞吐。

Win64 SSE 初筛 **+3.242%**，PE32 SSE/raw **+3.315%**，后者三对分别 **−4.079%/+3.315%/−1.232%**。完整初筛轮次保留于 `r2-thread-control-stable-initial-summary.json`，两项后续按三倍 work、同哈希 control 与三种运行顺序扩样；不同 work 独立汇总。

## 扩样调查与接受决定

两项扩样直接引用 before 原路径作为同哈希 control，按 after/before/control、before/control/after、control/after/before 三种顺序各一对。完整 `r2-thread-control-stable-summary.json` 保留 **48 个 workload protocol**（46 条初筛及两项不同 work 的扩样），未跨 work 合并：

| 路径 | work | after/before CPU | control/before CPU | after/control CPU |
| --- | ---: | ---: | ---: | ---: |
| Win64 SSE | 2100M | +3.473% | −0.221% | +3.702% |
| PE32 SSE/raw | 900M | +0.683% | −0.355% | +1.042% |

Win64 SSE 三对 after/before 为 **+3.258%/+3.473%/+3.145%**，保留为本批局部成本。PE32 SSE/raw 三对为 **−5.822%/−0.253%/+0.683%**。其余初筛路径均未超过 3%，完整原始轮次保持。

正式计时后独立 diagnostics，Win64 两版 IR **7,000,003**、translations **13**、fallback/invalidations **0**。七个 AMD64 CPU 源文件哈希保持；CheckedSegment/RegisterSegment 分别 **2156/1968** 条生成指令，助记符序列 SHA 相同；该比较未核对全部操作数。证据 `r2-thread-control-sse-investigation.json` 及原始反汇编。局部成本的具体机制尚未确证，没有修改 CPU 热循环或新增 JIT。

Alice 200M 三对全部通过，CPU **−0.949%**、wall **−0.952%**，继续到预算出口、隐藏 **314×211** 窗口及 Draw/Present **0**；该启动前缀尚未进入实际绘制。原游戏 runtime 直接复用，严格串行完成编译、正式计时、diagnostics 和 native 游戏。证据 `r2-thread-control-stable-alice`。

冻结源码、当前源码和 executable 重新核对一致：**283 个 Rust 文件**（src/tests **275**）、**398 个源码/配置文件**，证据 `r2-thread-control-source-verification.json`。接受本批，明确保留 Win64 SSE 局部成本；未出现三对均回退超过 5% 的路径，功能与故障顺序回归保持。契约累计 **319 个 export**，Mono 继续排除。日历批次 Local/Global **+4.570%**、线程属性批次 HeapSize **+4.063%** 等历史成本继续按各自原基线保留。
