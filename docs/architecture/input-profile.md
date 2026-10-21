# Raw input 与 IME 边界

状态：24 项契约已接入并验收，实现提交 `f6f6fbc`，正确性、性能成本与游戏前缀见[本批报告](../reports/refactoring-input-profile-2026-10-18.md)。

| 家族 | export 数 | PE32 | Win64 静态 / 动态 provider |
| --- | ---: | --- | --- |
| raw input | 7 | user32 ABI-only | kernel32/kernelbase/user32 / user32 |
| IME | 17 | imm32 ABI-only | kernel32/kernelbase/user32/imm32 / user32/imm32 |

契约集中声明 API ID、参数宽度、返回约定、静态与动态 provider。原 kernel 静态导入以及 IME 的 user32 查询范围继续保留。PE32 使用已知 stdcall ABI、原未实现返回与 LastError 120，不新增 raw 设备或输入上下文能力。

Win64 raw handler 由 User 持有注册和有界数据包，完整读取每个 16-byte RAWINPUTDEVICE 后原子提交。支持 usagePage 1 的 mouse (2) 与 keyboard (6)，前台投递、NOLEGACY 和稳定逻辑设备的 DEVNOTIFY 标志；target 为零时投递到宿主输入窗口，否则保留并校验完整 HWND。REMOVE 要求零 target。后台 INPUTSINK、HID、排除页面等未实现模式明确返回 error 50，不伪造注册成功。PE32 raw 仍为 ABI-only，普通键鼠消息已支持。

AppKit 回调的真实事件生成 WM_INPUT 与可读 HRAWINPUT，mouse 包含五键、双轴滚轮和虚拟桌面归一化的绝对位置，keyboard 保留 scan/VKey、E0、make/break 与系统键消息。它是宿主窗口输入的逻辑设备桥，不是物理 HID 透传；使用 MOUSE_MOVE_ABSOLUTE | MOUSE_VIRTUAL_DESKTOP；宿主没有物理相对计数，不能从第二次移动才开始累计差值，否则会漏掉首个位置。归一化与显示查询共用同一个逻辑桌面尺寸。原来的普通消息仍交付，NOLEGACY 则抑制相应普通键鼠与字符消息。RAWINPUTHEADER 为 24 字节，mouse/keyboard 包分别为 48/40 字节；GetRawInputData 支持 INPUT/HEADER 和两阶段查询，读取不自行释放包，DefWindowProc 清理其 handle。

GetRawInputBuffer 按当前线程消费尚未从消息队列移除的包，以 8 字节对齐串接，并一并移除相应 WM_INPUT。已 Peek(PM_REMOVE) 的包留给 GetRawInputData，不被缓冲读取再次消费；输出失败不移除消息或包。NOLEGACY 的 GetKeyState 在 raw 消息/缓冲消费时更新。GetRawInputDeviceList、GetRegisteredRawInputDevices 及设备名称/32 字节 RID_DEVICE_INFO 均按真实逻辑状态返回；无效 handle、结构尺寸、容量及输出检查保持明确错误。最多保留 4096 个包，窗口销毁撤销其注册及剩余包；handle 单调分配，不复用过期值。

IME 的无 context 策略独立于 Runtime64、guest 内存和宿主输入，只接收 typed ID、参数与窗口存在性。它保留无 IME 的查询结果、完整 HWND 校验、零 context 关联/移除子集及错误优先级，任何组合字符串、字体、窗口、状态输出指针均不解引用。窗口生命周期仍归原 User owner；适配器读取该 owner 并按原顺序提交 LastError，不创建第二套窗口或 context 状态。

Win64 的输入 adapter 由 User 执行，内存按次传入；Runtime64 的私有入口仅组合资源。raw 的逻辑状态与字节适配分开，IME 策略保持独立，错误分支才通过 Threads 查询当前 TEB。

API 字符串只在冷绑定与查询阶段解析；调用进入 typed adapter。旧 raw/IME arity、DWORD、provider 特例、名称分发及两组 PE32 IME 重复 ABI 表删除。
