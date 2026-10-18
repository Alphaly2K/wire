# 绘制区域与 paint DC 基线（2026-10-19）

状态：三项独立旧行为基线和完整正确性通过，六项契约和 typed paint 适配已接入，尚待性能验收。消息队列长样本补证期间仅编辑运行时源码和文档，结束后才开始 Cargo，编译及性能工具修改保持串行。

范围为 InvalidateRect、ValidateRect、GetUpdateRect、BeginPaint、EndPaint、RedrawWindow。保留 PE32 矩形 update/erase、GDI clip/DC 资源与 guest 回调协调，六项 Win64 export 保持 absent，不扩展复杂 region 或非客户区绘制。

两项 PE32 基线固定 RedrawWindow flags/region 验证先于 HWND/矩形访存、PAINTSTRUCT 输出预检失败时保留 update region、WM_ERASEBKGND 失败前已清除 update/erase 但未写输出，以及 EndPaint 释放 DC 而保留后来独立产生的 update region，重复 EndPaint 仍报告错误 6。Win64 一项固定 user32/kernel32/kernelbase 的原 absent 范围。既有真实像素、背景擦除、clip、合成 paint 消息及默认过程回归继续保留。

这些测试和未注册契约不属于消息队列此前核对的 388 Rust 文件冻结。先等待消息性能与游戏执行结束，再编译旧行为基线；运行时迁移必须在基线通过之后开始。

消息初筛与游戏已串行结束；在首次构建候选前固定 executable 增量预算为相对 `r2-messages-wire/wire` +24KiB。新增可选 `gateway-painting`：实际建立 32×32 窗口，每轮 InvalidateRect→BeginPaint→EndPaint，PAINTSTRUCT 位于 guest 栈，累加 EndPaint 返回值并最终销毁窗口。独立旧程序验证后再用于比较。

旧消息冻结上原创 fixture 的 IR/raw 首次全部校验通过：n=100，retired=1,341、API=303、checksum=101；工具 23 项及三项独立 painting 测试通过。证据 `r2-painting-fixture-baseline`、`r2-painting-tool-tests.txt` 与 `r2-painting-baseline-tests.txt`。此时旧 paint handler 仍在运行，测试先于 typed 注册。

基线提交 `0626057`。本批删除六项旧手工 ABI 和 `user32_paint` 名称入口，默认 WM_PAINT 的 Begin/End 也使用 typed ID；PE32 的返回/转移适配仍在原调用边界提交。矩形区域、PAINTSTRUCT、DC 和擦除回调原分支保留，边界见[绘制说明](../architecture/painting.md)。

完整 release 610 / 7 ignored、debug 456、显式 native 7，fmt、strict all-target/all-feature Clippy 与 build 全部通过，首次候选构建没有错误。391 个 Rust 文件（src/tests 383）live/frozen/manifest 和文件集合一致，target/frozen SHA 相同。冻结 `r2-painting-wire/wire` SHA256 `7342f483c9a5f7e6b20789eae2674d413e3aeaf55ddf96754038ab7736951a13`，5,973,144 bytes，较消息冻结 +2,704 bytes，在 +24KiB 预算内。日志 `r2-painting-validation-*`，来源核对 `r2-painting-validation-source-verification.json`。

正式初筛 28 条路径：双架构基础执行器与 FP/bit/cache-alias；Win64 控制/消息/GUID；PE32 实际 paint 循环、窗口过程控制、消息、GDI/GUID 的 IR/raw。每条三对，超过门槛再定向扩样；Alice 仍使用同一 200M 预算和现有 runtime，全部计时与编译、诊断串行。

首版冻结正确性通过但性能拒绝：AMD64 FP 的 300M 三对 CPU +29.207%/+27.579%/+23.775%，中位数 +27.579%，达到持续回退门槛；停止其余矩阵和游戏，保留退出 130 与已完成轮次。data-alias 中位数 +3.299% 也待复核。`nm` 显示原 Single/Double 的 `mul_r` 内联变成独立调用，`simd_arithmetic` 的生成代码随模块拆分改变；ISA 源码没有变化。后续仅恢复原编译模块命名，源码仍位于 `user32/paint.rs`，typed 契约和唯一实现保留，重新验证该边界版本。

原编译模块命名尝试冻结为 `r2-painting-layout-wire`；它没有恢复目标 APFloat 代码形态，未继续计时，模块命名尝试已撤回。反汇编进一步确认 before 的 `simd_arithmetic<Double>` 内含 accept 检查和乘法主体，首版改为调用 `simd_accept` 与 `mul_r`。单独的性能修正只为 `SimdState::simd_accept` 添加显式内联，保留所有 MXCSR、denormal、FTZ、异常和结果提交规则，先验证真实生成代码及完整正确性，再测性能。后续对话框四项旧行为基线已通过；该未注册草稿和测试不属于首版绘制冻结。

显式 accept 内联候选完整 release 614 / 7 ignored、debug 460、native 7 及 fmt/strict Clippy/build 全部通过。395 个 Rust 文件（src/tests 387）和冻结 executable 完整核对，SHA256 `fd91d14a223b16ad8de531b7fad4eaecd6a07d093a437bf6c02a8ccd3a2f33c6`，5,989,272 bytes，相对 before +18,832 bytes，仍在 +24KiB 预算内。生成代码中的独立 `simd_accept` 调用消失，Single/Double `mul_r` 仍存在，先运行三标签浮点筛查；该候选尚未验收。冻结 `r2-painting-inline-wire`，日志 `r2-painting-inline-validation-*`。

仅 accept 内联的三标签筛查仍有约 8% 的持续浮点成本，候选拒绝，保留 `r2-painting-inline-fp-screen`。进一步为 generic arithmetic 边界添加普通 `#[inline]`，使调用者可跨编译单元优化该纯语义 helper；先构建并运行独立浮点测试和定向筛查，成功后再完成阶段完整检查。该尝试仍使用原 +24KiB 体积预算，不删除任何受检语义。

普通 arithmetic 内联提示候选通过 9 项独立浮点测试，但三对仍持续慢约 8%，未接受；保留 `r2-painting-inline2-wire` 与 `r2-painting-inline2-fp-screen`。下一候选将 arithmetic 明确内联到架构调用者，避免保留独立的通用运算调用；仍先按相同浮点筛查和体积预算判断，再决定是否继续完整验收。

强制 arithmetic 内联也未过门槛，`r2-painting-inline3-*` 全部保留，三个 SIMD attribute 试验均已撤回。随后单独串行抓取 before/首版各 3 秒 sample，两次均验证 retired/work=900000013、checksum=0x5f5e100。首版独立 accept 的 Single/Double leaf 样本共 189，before 的检查融合在 arithmetic；同时可见乘法/转换跨函数边界变化。profile、manifest 和运行结果在 `r2-painting-fp-profile`，不计作正式计时。

下一独立构建配置试验为 ThinLTO，通过临时 `CARGO_PROFILE_RELEASE_LTO=thin` 运行，尚未修改 Cargo 配置；目的是允许跨编译单元联合优化，避免用更多源代码内联标记弥补边界变化。保留 +24KiB 预算，先测浮点与基础执行器，只有完整正确性和性能门槛通过才考虑采用。

ThinLTO 的 9 项浮点回归通过，executable 为 6,408,456 bytes，较已验收基线 +438,016 bytes，超过纯结构预算，尚未计时或采用。将其明确分离为编译配置优化试验，在首轮计时前设独立 +512KiB 上限；需要实测收益与完整基础矩阵支持这笔代码体积成本，纯绘制结构原 +24KiB 预算继续单独记录，不宣称 ThinLTO 满足该结构预算。

ThinLTO 浮点三对仍持续慢约 8%–10%，配置试验拒绝，默认 Cargo 配置保持原状。profile 还显示转换/整数结果的状态桥接调用；下一候选只内联 accept、convert 和 to_integer 三个较小的状态/转换包装，保持大 arithmetic helper 的原属性。该候选恢复默认构建，继续原结构 +24KiB 预算，独立验证实际浮点结果和耗时。

三个小 helper 内联仍未通过门槛，最后一组三标签还出现显著 control 漂移，原始样本保留于 `r2-painting-state-inline-*`；该试验已撤回，SIMD 源码恢复原状。后续改为收敛 catalog 自身：52 层 `.chain()` 更换为固定的 slice-family iterator，保持家族/契约顺序、解析结果与冷绑定阶段；新增家族只改变数据，不再形成新的嵌套 iterator 类型。该结构候选重新执行完整检查和冻结，仍使用原 +24KiB 预算，性能保持待验收。

宿主负载核查发现 RustRover 派生的独立 Cargo PID 45657 持续运行（PPID 45425，观察时 CPU 约 80%）。最新 state-inline 的 control 大幅漂移数据保留为未准入计时，不能据此作最终接受；更早样本的宿主活动没有完整审计，保持候选未接受。后续正式计时先确认后台 Cargo 结束，计时期间也停止后续 Rust 草稿编辑，避免 IDE 自动编译与采样重叠。

固定 iterator 候选完整 release 614 / 7 ignored、debug 460、native 7 和 fmt/strict Clippy/build 通过。395 个 Rust 文件和 executable 核对一致，冻结 `r2-painting-family-wire`，SHA256 `6b0fbbb29533d504371a075f73b38cef8e2daa90ee8996d5ca70636cc9a54792`，5,973,720 bytes，较 before +3,280 bytes，满足原 +24KiB 预算。来源 `r2-painting-family-validation-source-verification.json`。后台 Cargo 退出后先运行旧程序/control/原绘制候选/家族表候选四标签浮点筛查，源码静止，确认是否还存在稳定成本。
