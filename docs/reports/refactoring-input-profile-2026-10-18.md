# R2 raw input / IME（2026-10-18）

状态：旧行为独立基线通过，24 项契约及 typed adapter 已接入候选，完整验收进行中。

## 独立基线

在 `386f38a` 的旧 raw-input/IME 实现上，新增五项行为测试通过：PE32 24 项已知 ABI-only 保持返回零、LastError 120、stdcall 清栈与输出不变；Win64 保留静态/动态 provider 差异、DWORD 截断与完整 HWND、count 输出覆盖 saved return slot 后仍使用已保存 frame、参数错误先于 payload 访问、无 IME/context 的错误优先级。既有 IME 输出 sentinel 测试继续保留。

Win64 raw input 静态支持 kernel32/kernelbase/user32，动态仅 user32；IME 静态支持 kernel32/kernelbase/user32/imm32，动态 user32/imm32。这些历史范围明确保留。PE32 raw input 仅 user32，IME 仅 imm32，全部 ABI-only。

原创可选 `gateway-raw-input`、`gateway-ime` 在已接受键盘冻结 `r2-keyboard-wire/wire` 上完成 12K 双架构预检，PE32 raw/static IR 均验证。无设备 count 写零与 PE32 未实现时 sentinel 保留分别进入 checksum；核对退休数、API 次数及 Win64 work。单一 import、stack 输出、不依赖宿主输入，不改变默认 fixture 集合。工具 20 项通过。

证据根 `wire-out/refactoring-2026-10-14/`：`r2-input-profile-baseline-tests.txt`、`r2-input-profile-baseline-tools.txt`、`r2-input-profile-fixture-baseline64/32`。本批 24 项尚不计入已验收 400 exports。

## 实现候选

七项 RawInputApi、十七项 ImeApi 追加到 catalog，保留静态/动态 provider、PE32 ABI-only 和 Win64 参数宽度。旧 raw/IME arity、DWORD/provider 特例、名称分发与两组 PE32 IME 手写 ABI 删除。raw handler 只借 Memory，IME 无 context 策略只借窗口存在性与参数；入口提交 LastError，原 User owner 保留窗口生命周期。既有 raw unit test 随适配器迁移。五项独立基线在候选上再次通过。

before 为键盘已验收冻结 `r2-keyboard-wire/wire`，SHA256 `40d21a10e33677a7410e09255b83cc2e42960fde87797f8d3eeeedd5ca40dea5`。完整正确性、基础双架构性能矩阵、新网关及 Alice 200M 前缀按串行协议验收；超过门槛的路径保留全部轮次并独立调查。

候选完整 release **542 passed / 7 ignored**，debug Win32/AMD64/FPU **389 passed**，显式 native **7 passed**，工具 **20 passed**；fmt、strict all-target/all-feature Clippy 与 release build 通过。领域 PE32 **178**、Win64 **202** 项通过。

冻结 `r2-input-profile-wire/wire`，SHA256 `8d5b89786c354e42a4587445078d57b0d5eb8d25463c32a78237dd3b8ed684c2`，**5,928,456 bytes**，较 before **+8,336 bytes**，在本批 +16KiB 预算内。**324 个 Rust 文件**（src/tests **316**）与活动源码/快照逐项核对，target/frozen SHA 一致，证据 `r2-input-profile-source-verification.json`。性能尚未验收，不增加累计数。
