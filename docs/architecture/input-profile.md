# Raw input 与 IME 边界

状态：24 项契约已接入并验收，实现提交 `f6f6fbc`，正确性、性能成本与游戏前缀见[本批报告](../reports/refactoring-input-profile-2026-10-18.md)。

| 家族 | export 数 | PE32 | Win64 静态 / 动态 provider |
| --- | ---: | --- | --- |
| raw input | 7 | user32 ABI-only | kernel32/kernelbase/user32 / user32 |
| IME | 17 | imm32 ABI-only | kernel32/kernelbase/user32/imm32 / user32/imm32 |

契约集中声明 API ID、参数宽度、返回约定、静态与动态 provider。原 kernel 静态导入以及 IME 的 user32 查询范围继续保留。PE32 使用已知 stdcall ABI、原未实现返回与 LastError 120，不新增 raw 设备或输入上下文能力。

Win64 raw handler 只借 guest Memory：注册完整读取每个 16-byte RAWINPUTDEVICE，只有非零 usagePage、REMOVE 标志与零 HWND 可成功；HWND 保留 64 位。空设备/缓冲查询校验 cbSize 与 count 指针后写零，payload 不使用；错误先于 payload 访问，DWORD 参数由绑定计划截断。DWORD 输出先于入口完成返回，但返回 frame 仍在 handler 前保存。

IME 的无 context 策略独立于 Runtime64、guest 内存和宿主输入，只接收 typed ID、参数与窗口存在性。它保留无 IME 的查询结果、完整 HWND 校验、零 context 关联/移除子集及错误优先级，任何组合字符串、字体、窗口、状态输出指针均不解引用。窗口生命周期仍归原 User owner；适配器读取该 owner 并按原顺序提交 LastError，不创建第二套窗口或 context 状态。

Win64 的输入 adapter 现由 User 执行，只读借窗口状态与 Threads，内存按次传入；Runtime64 的私有入口仅组合资源。原 raw 算法和独立 fixture 保持，错误分支才通过 Threads 查询当前 TEB。

API 字符串只在冷绑定与查询阶段解析；调用进入 typed adapter。旧 raw/IME arity、DWORD、provider 特例、名称分发及两组 PE32 IME 重复 ABI 表删除。
