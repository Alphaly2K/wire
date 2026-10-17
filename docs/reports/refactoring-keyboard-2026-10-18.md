# R2 键盘状态与布局（2026-10-18）

状态：独立旧行为基线通过，新契约与适配器尚未接入。

## 独立基线

before 为基础 helper 已验收冻结 `r2-basic-fields-wire/wire`，SHA256 `b6ac77e4eff26b9dc39540dfa69ee823383728f2105452db5f700b5ef4804b79`，5,917,192 bytes。范围为三个状态 API 与十三个布局/type/map/key-name/Unicode API。

四项独立回归在旧名称分发上通过：PE32 十三项 layout 继续 ABI-only、LastError 120、无输出及原 stdcall 清栈；状态 API 保留 LastError、handler 后读取返回槽、跨页输出 fault 与错 provider 的 unknown ABI。Win64 固定状态 API 在 kernel32/kernelbase/user32 的静态路由，但动态查询仅 user32；DWORD 高位截断、saved return slot、Unicode signed 容量优先于 flags/HKL/指针验证、完整 HKL 与输入/输出别名均固定。

首轮 PE32 测试直接修改 ApiState.last_error，遗漏入口从 TEB 重读，预期失败。改由 guest SetLastError 初始化后通过，运行时行为未改。两份日志保留为 `r2-keyboard-baseline-tests.txt` 和 `r2-keyboard-baseline-tests-corrected.txt`。

原创可选 `gateway-keyboard`（双架构）与 `gateway-keyboard-map`（Win64）在旧冻结上完成 12K 预检，checksum、退休数、API 次数及 Win64 work 全部核对，PE32 raw/static IR 均验证；证据 `r2-keyboard-fixture-baseline64/32`。输出使用 stack buffer，不写 fixture 代码页，不注入宿主按键。工具 20 项通过，默认 case 集合保持。

## 实现与验收

接入及后续验证另行记录；当前不计入已验收 384 exports。
