# R2/R3 COM 基础家族（2026-10-17）

## 独立旧行为基线

起点 `8e87121`，before 为 `r2-thread-control-wire/wire`，SHA256 `fbe7f6ab88066a4a536327df97a7dd94a02099990a04af111bb779dbc79ef532`。范围十项 COM 初始化、WinRT 初始化、class factory、task allocation 和 Ole ABI-only export。

基线提交 `7670c68`，六项新增独立回归通过：PE32 apartment 错误/LastError、引用/tid 隔离、进程 heap 共享、无效 free/超大分配、Ole ABI-only 与错 provider；Win64 COM/WinRT 分别计数、mode 冲突、完整 reserved/DWORD flags、静态/动态 provider、factory 输出/错误/trace/alias、task 归属/GUID/Local、返回帧预检，以及真实线程 slice 1/100000 下的 apartment 隔离。初次测试对 PE32 超大分配 LastError 及未注册 provider 的动态装载作了过强预期，按实测旧行为修正基线，未修正运行时行为。证据 `r2-com-core-baseline-tests-providers.txt`。

新增可选 `gateway-com` 原创 fixture，每轮两次 CoInitializeEx 和两次 CoUninitialize，checksum 消费第二次初始化的 S_FALSE。两架构及 PE32 raw/static IR 均在旧 executable 上 9600 work 预检通过，独立核对结果、退休指令和 API 次数，Win64 另核对 work。默认 fixture 集合保持，证据 `r2-com-core-fixture-baseline64`、`r2-com-core-fixture-baseline32`。

## 实现

十项 export 接入 ComApi 唯一声明，移除重复 ABI、arity/DWORD、COM provider 特例和根名称分支；保留静态/动态 provider 差异、PE32 Ole ABI-only 和 Ro absent、Win64 Ole absent。PE32 apartment/task adapter 只借明确的 owner/资源，WMI factory 继续原实现；根入口维持 CPU/返回提交。Win64 scalar typed route 委托所属 adapter。

纯 Apartment 状态由线程独占，移除 Win64 Runtime64 的活动线程 mode/refs 镜像；PE32 tid 映射收进 Apartments32。TaskMemory 私有拥有 task allocation 成员，原 backing allocator 仍独立，通过显式借用保证 free 的 fault 与提交顺序，GUID 和 known-folder 继续共享原登记。边界与限制见[COM 基础契约](../architecture/com-core.md)。

首轮全量检查后审查公开 facade，补回 `nls::dispatch` 的原 task allocation 接受范围，改为委托同一 COM adapter；追加直接入口回归，固定精确 provider、日志和 process heap 归属。该修正在正式计时之前完成，最终检查和冻结使用 `r2-com-core-final-*`，原首轮日志单独保留。

## 最终正确性与冻结

七项独立回归及完整 release **509 passed / 7 ignored**、debug Win32/AMD64/FPU **356 passed**、显式 native **7 passed**、工具 **20 passed** 全部通过；fmt、strict all-target/all-feature Clippy 与 release build 通过。冻结 `r2-com-core-final-wire/wire`，SHA256 `d627b51f5ff407ccc1a9b963036a04ecd6844512bad0417b8cf267fb530dc1c3`，**5,997,032 bytes**，相对线程控制增加 **1,216 bytes**。

冻结 manifest 内 **289 个 Rust 文件**（src/tests **281**）、合计 **386 个源码文件**与当前内容逐项核对一致，target 与冻结 executable 哈希一致，证据 `r2-com-core-source-verification.json`。后续 WinRT 独立基线草稿尚未注册到测试 target，不属于本批冻结与验收。

## 性能与接受决定

稳定启动协议下 **49 条初筛路径**各三对全部结果校验，最大 CPU 回退 **+2.129%**，RSS 最多增加 **64KiB**。新增 balanced COM 网关 CPU Win64 **−12.916%**、PE32 IR/raw **−2.754%/−2.063%**；该负载只覆盖同线程 init/uninit 与 S_FALSE，不代表所有 COM 操作或游戏帧率。

PE32 integer/IR 初筛中位数为 −8.329%，但三对为 −5.837%/−8.329%/+7.725%；Win64 HeapSize、PE32 系统信息/IR、HeapSize/raw 也各有一对超过 +5%。因此四项都追加三倍 work、直接引用 before 原路径的同哈希 control，并按 after/before/control、before/control/after、control/after/before 三顺序各一对。完整 `r2-com-core-stable-summary.json` 保留 **53 个 workload protocol**，不同 work 不合并：

| 路径 | work | after/before CPU | control/before CPU | after/control CPU |
| --- | ---: | ---: | ---: | ---: |
| PE32 integer/IR | 900M | +0.078% | −0.198% | +0.276% |
| Win64 HeapSize | 300M | +0.336% | −0.181% | +0.518% |
| PE32 系统信息/IR | 150M | −0.921% | −0.427% | −0.497% |
| PE32 HeapSize/raw | 150M | +2.367% | +0.500% | +1.858% |

Alice 200M 三对全部保持预算出口、隐藏 **314×211** 窗口、Draw/Present **0**；CPU **+0.024%**、wall **+0.229%**。该前缀尚未进入实际绘制，直接复用已有 runtime，证据 `r2-com-core-stable-alice`。编译、正式计时与 native 游戏串行完成，首轮冻结副本未参加计时。

接受本批十项 export，契约累计 **329 项**。所有扩样中位数低于 3%，不存在三对均回退超过 5% 的路径；保留初筛全部离散轮次，未将 integer/IR 的初筛负值当作可靠收益。线程控制批 Win64 SSE **+3.473%**、线程属性批 HeapSize **+4.063%**、日历批 Local/Global **+4.570%** 等历史成本仍按各自基线保留。Mono 继续排除。
