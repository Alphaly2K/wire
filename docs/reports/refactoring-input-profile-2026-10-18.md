# R2 raw input / IME（2026-10-18）

状态：旧行为独立基线通过，运行时契约尚未迁移。

## 独立基线

在 `386f38a` 的旧 raw-input/IME 实现上，新增五项行为测试通过：PE32 24 项已知 ABI-only 保持返回零、LastError 120、stdcall 清栈与输出不变；Win64 保留静态/动态 provider 差异、DWORD 截断与完整 HWND、count 输出覆盖 saved return slot 后仍使用已保存 frame、参数错误先于 payload 访问、无 IME/context 的错误优先级。既有 IME 输出 sentinel 测试继续保留。

Win64 raw input 静态支持 kernel32/kernelbase/user32，动态仅 user32；IME 静态支持 kernel32/kernelbase/user32/imm32，动态 user32/imm32。这些历史范围明确保留。PE32 raw input 仅 user32，IME 仅 imm32，全部 ABI-only。

原创可选 `gateway-raw-input`、`gateway-ime` 在已接受键盘冻结 `r2-keyboard-wire/wire` 上完成 12K 双架构预检，PE32 raw/static IR 均验证。无设备 count 写零与 PE32 未实现时 sentinel 保留分别进入 checksum；核对退休数、API 次数及 Win64 work。单一 import、stack 输出、不依赖宿主输入，不改变默认 fixture 集合。工具 20 项通过。

证据根 `wire-out/refactoring-2026-10-14/`：`r2-input-profile-baseline-tests.txt`、`r2-input-profile-baseline-tools.txt`、`r2-input-profile-fixture-baseline64/32`。本批 24 项尚不计入已验收 400 exports。
