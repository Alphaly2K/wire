# Raw input 与 IME

新 Wasm 入口在 PE32 与 AMD64 接入全部七项 RawInput 和十七项 IME 服务，分别通过 user32.dll、imm32.dll 提供。参数按客体 ABI 捕获，DWORD 保持 32 位，窗口、数据指针和句柄保持完整客体宽度；成功或普通错误返回计一 work，不增加客体退休数，不采样时钟。

RawInput 提供两个稳定的逻辑设备：mouse 与 keyboard，数据来自[宿主输入](input-events.md)。注册接受 usagePage=1、mouse usage=2 与 keyboard usage=6，支持前台投递、NOLEGACY=0x30 和稳定逻辑设备的 DEVNOTIFY=0x2000。target=0 使用该次输入路由后的窗口，非零 target 必须是本进程有效完整 HWND，包和 WM_INPUT 归该窗口线程。后台 INPUTSINK、HID 和排除页面等模式返回错误 50。

RegisterRawInputDevices 的数组按四字节对齐，数量为 1..1024；RAWINPUTDEVICE 在 PE32 为 12 字节，AMD64 为 16 字节。完整读取和验证整批后才替换注册表，重复 usage 按数组顺序更新。REMOVE=1 要求 flags 仅为 1 且 target=0。参数错误为 87，无效窗口为 1400；失败不发布部分注册。GetRegisteredRawInputDevices 按 usage 顺序输出当前记录，GetRawInputDeviceList 输出两个逻辑设备及类型，元素大小分别为 8/16 字节。pcbSize/计数槽在两种 ABI 均为 DWORD。

GetRawInputDeviceInfoA/W 支持设备名称和 32 字节 RID_DEVICE_INFO；后者要求调用者先设置 cbSize=32。宽名称容量以 UTF-16 unit 计，其他输出容量以字节计。空输出查询返回所需容量，容量不足返回 0xffffffff、错误 122 并更新所需大小；无效完整设备句柄返回错误 6。设备名和输入包均属于当前进程的逻辑设备状态。

宿主 mouse 包含五键、双轴滚轮和虚拟桌面的归一化绝对位置，双击保留对应按下转换；keyboard 保留 scan/VKey、E0、make/break 与系统键消息。mouse 使用 MOUSE_MOVE_ABSOLUTE | MOUSE_VIRTUAL_DESKTOP，位置与逻辑显示、ClipCursor 和 MSG.pt 共用屏幕坐标。物理 HID、物理相对计数和后台投递尚未实现。

| 布局 | PE32 | AMD64 |
| --- | ---: | ---: |
| RAWINPUTHEADER | 16 字节 | 24 字节 |
| mouse 包 | 40 字节 | 48 字节 |
| keyboard 包 | 32 字节 | 40 字节 |
| 缓冲区包对齐 | 4 字节 | 8 字节 |

GetRawInputData 支持 RID_INPUT/RID_HEADER 和两阶段大小查询，输出按四字节对齐，cbSizeHeader 必须符合当前 ABI；读取不释放包。GetRawInputBuffer 仅消费当前线程仍在消息队列中的包，按上表对齐串接，同时移除相应硬件 WM_INPUT；已 GetMessage/PM_REMOVE 的包继续保留给单独读取。DefWindowProc 清理属于该窗口的包及对应排队消息。输出、大小槽或 LastError 访存失败均不提交部分输出、消息消费、键状态或调用者；返回槽和输出重叠时使用预先保存的返回地址。

NOLEGACY 抑制对应普通键鼠和字符消息；键状态的异步部分在宿主事件进入时更新，线程队列部分在 raw 消息或缓冲被实际消费时更新。PM_NOREMOVE、大小查询、单包读取和输出故障不模拟消费。启用 mouse-to-pointer 时仍可产生 pointer 消息，普通滚轮消息继续受 NOLEGACY 抑制。

每进程最多保留 4096 个包；已取出的保留包仍占容量。宿主批次在修改队列、键盘或游标前统一检查数据包、消息和句柄容量，失败不消耗身份或留下前缀事件。包句柄单调分配，PE32 身份不会截断回绕。窗口销毁或线程退出回收该窗口的包与显式目标注册；target=0 的进程注册继续有效。进程结束回收全部状态。

IME 复用无 context 策略，按完整 HWND 验证窗口，仅在错误分支写当前线程 LastError。它提供无 IME 的查询、零 context 关联/移除子集和明确的失败结果，不签发 HIMC，也不访问组合字符串、字体、窗口或状态输出。

注册、布局与 copy-out 处理体由新旧适配器共用；旧入口保留其 provider 和错误提交顺序，新入口使用当前 VMM、消息队列、键盘 owner 及 JIT 线程身份。
