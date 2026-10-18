# R2 raw input / IME（2026-10-18）

状态：2026-10-18 接受，实现提交 `f6f6fbc`，独立基线提交 `adac711`。

## 独立基线

在 `386f38a` 的旧 raw-input/IME 实现上，新增五项行为测试通过：PE32 24 项已知 ABI-only 保持返回零、LastError 120、stdcall 清栈与输出不变；Win64 保留静态/动态 provider 差异、DWORD 截断与完整 HWND、count 输出覆盖 saved return slot 后仍使用已保存 frame、参数错误先于 payload 访问、无 IME/context 的错误优先级。既有 IME 输出 sentinel 测试继续保留。

Win64 raw input 静态支持 kernel32/kernelbase/user32，动态仅 user32；IME 静态支持 kernel32/kernelbase/user32/imm32，动态 user32/imm32。这些历史范围明确保留。PE32 raw input 仅 user32，IME 仅 imm32，全部 ABI-only。

原创可选 `gateway-raw-input`、`gateway-ime` 在已接受键盘冻结 `r2-keyboard-wire/wire` 上完成 12K 双架构预检，PE32 raw/static IR 均验证。无设备 count 写零与 PE32 未实现时 sentinel 保留分别进入 checksum；核对退休数、API 次数及 Win64 work。单一 import、stack 输出、不依赖宿主输入，不改变默认 fixture 集合。工具 20 项通过。

证据根 `wire-out/refactoring-2026-10-14/`：`r2-input-profile-baseline-tests.txt`、`r2-input-profile-baseline-tools.txt`、`r2-input-profile-fixture-baseline64/32`。

## 实现候选

七项 RawInputApi、十七项 ImeApi 追加到 catalog，保留静态/动态 provider、PE32 ABI-only 和 Win64 参数宽度。旧 raw/IME arity、DWORD/provider 特例、名称分发与两组 PE32 IME 手写 ABI 删除。raw handler 只借 Memory，IME 无 context 策略只借窗口存在性与参数；入口提交 LastError，原 User owner 保留窗口生命周期。既有 raw unit test 随适配器迁移。五项独立基线在候选上再次通过。

before 为键盘已验收冻结 `r2-keyboard-wire/wire`，SHA256 `40d21a10e33677a7410e09255b83cc2e42960fde87797f8d3eeeedd5ca40dea5`。完整正确性、基础双架构性能矩阵、新网关及 Alice 200M 前缀按串行协议验收；超过门槛的路径保留全部轮次并独立调查。

候选完整 release **542 passed / 7 ignored**，debug Win32/AMD64/FPU **389 passed**，显式 native **7 passed**，工具 **20 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。领域 PE32 **178**、Win64 **202** 项通过。

冻结 `r2-input-profile-wire/wire`，SHA256 `8d5b89786c354e42a4587445078d57b0d5eb8d25463c32a78237dd3b8ed684c2`，**5,928,456 bytes**，较 before **+8,336 bytes**，在本批 +16KiB 预算内。**324 个 Rust 文件**（src/tests **316**）在冻结时与活动源码/快照逐项核对，target/frozen SHA 一致，证据 `r2-input-profile-source-verification.json`。随后准备的矩形测试、fixture 与未接入实现草稿不属于该冻结。

## 性能、调查与接受决定

**30 条初筛协议**均三对校验，RSS 中位数最多 **+336KiB**。AMD64 integer/memory/SSE 为 **+0.119%/−0.298%/−1.673%**，FP/bit/code-alias 为 **−0.235%/−1.087%/−2.895%**。新 raw-input/IME 网关 Win64 **+2.440%/−3.012%**；PE32 raw input IR/raw **−12.408%/−5.732%**，IME IR/raw **−7.775%/−4.826%**。这些是本批测量结果，未推导通用性能收益。PE32 integer/IR 中位数 −7.821%、raw −0.954%，raw 有一对 **+11.603%**；原始离散保留。

AMD64 data-alias 初筛 **+3.213%**，PE32 SSE/IR、FP/IR、FP/raw **+3.043%/+5.223%/+4.202%**，触发三倍 work（900M）扩样；每项加入 before 同哈希 control，三种顺序各一对。结果如下：

| 路径 | after/before CPU | control/before CPU | after/control CPU |
| --- | ---: | ---: | ---: |
| AMD64 data-alias | +0.340% | −1.229% | +1.588% |
| PE32 FP/IR | +2.172% | +1.055% | +1.105% |
| PE32 FP/raw | +2.986% | +7.305% | −4.025% |

SSE/IR 首组三种顺序的逐对差异为 **−1.499%/+20.934%/−5.511%**，跨轮中位数 **+19.803%**。第二轮同哈希 before/control CPU **6.284881/7.201823 秒**，第三轮 **9.100170/6.663868 秒**，存在明显漂移。追加三种顺序的复核，两端分别加入同哈希 control/clone，三对 **−0.458%/−0.002%/−1.422%**；这次 after/before 中位数 **−0.352%**，control/before **+0.054%**、clone/after **+1.970%**。原先三对与追加三对一起汇总后为 **−0.822%**，不删除原 +19.803% 或慢轮。证据 `r2-input-profile-stable-expanded-*`、`r2-input-profile-stable-sse-recheck-*`。

离线调查核对 SSE/IR、FP/IR、FP/raw 各九次既有报告，执行计数与最终 GPR/EIP/EFLAGS 分组一致。**27 个 CPU/IR/memory 源文件**在两个冻结之间哈希保持。按 Mach-O 地址/大小限定范围反汇编，七节中六节助记符序列保持，`__wire_0f32` 为 **6212→6213** 条，六个被调查的 AMD64 执行/访存 helper 助记符保持；未比较全部操作数。独立 data-alias 3M diagnostics 两端均 IR **3,000,007**、fallback **0**、translation/cold **13**、失效 **0**，最终 RAX/退休数一致。具体漂移与局部成本机制未确证。证据 `r2-input-profile-cost-investigation/summary.json`、`r2-input-profile-data64-diagnostics/counters.json`。

最终 **34 个 workload protocol = 30 初筛 + 4 扩样**，其中 SSE/IR 扩样共六对，追加三对另有 after 同哈希 clone。所有结果校验。初筛、扩样原汇总与最终汇总分别保留为 `r2-input-profile-stable-initial-summary.json`、`r2-input-profile-stable-summary.json`、`r2-input-profile-stable-final-summary.json`；不同 work 不混算。

Alice 200M 前缀三对保持预算出口、隐藏 client **314×211** 和 Draw/Present **0/0**。after CPU **2.733019 秒**、wall **2.820218584 秒**、RSS **417,873,920 bytes**，相对 before CPU **+2.813%**、wall **+2.631%**、RSS **+448KiB**。直接复用原 runtime，无素材复制。首次 heredoc 命令拼接出现 Python 语法错误，尚未启动游戏；修正后单独执行，原失败日志与 `r2-input-profile-stable-alice-retry.txt` 均保留。实际结果为 `r2-input-profile-stable-alice/compare.json`，该未绘制前缀不推断游戏 FPS。

在保留 FP/IR **+2.172%**、FP/raw **+2.986%**、Alice 前缀成本及全部离散轮次的前提下接受本批。没有三对均超过 5% 的拒绝路径；本批 **24 exports** 纳入验收，累计 **424 exports**。
