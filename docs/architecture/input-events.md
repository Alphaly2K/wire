# 宿主键鼠事件

PE32 和 Win64 使用同一个 AppKit 输入桥。native 回调只把四个 DWORD 的 owned event 放入有界队列；Presenter 在主线程轮询，guest USER adapter 负责完整 HWND、坐标、键盘状态和消息入队。回调不执行 guest 代码。

| 输入 | 交付行为 |
| --- | --- |
| 键盘 | 字母、数字、OEM 标点、控制/导航键、F1–F20、数字键盘的物理 VK 和扫描码；保留 extended、repeat、previous、transition 与 Alt context 位 |
| 修饰键 | 左右 Shift/Ctrl/Alt，Command 对应左右 Win 键；CapsLock 每次切换产生一对 down/up |
| 文本 | `NSEvent.characters` 的 UTF-16 经 WM_CHAR/WM_SYSCHAR 暂存，TranslateMessage 后交付；保留代理对，按窗口 A/W 策略编码 |
| 鼠标 | 左/右/中键、两个侧键、移动及三类拖动；wParam 包含所有已按下的鼠标键及 Shift/Ctrl |
| 滚轮 | WM_MOUSEWHEEL/WM_MOUSEHWHEEL，有符号高 WORD delta；普通滚轮按 120 单位/格，精确设备按像素累计小数；水平正值向右 |
| 失焦 | native view 释放它记录的所有按键和鼠标键，避免在窗口外释放后继续保持按下 |
| 激活/焦点 | 获得 native key window 时交付 WM_ACTIVATEAPP、WM_ACTIVATE、WM_SETFOCUS；失去时交付 WM_KILLFOCUS、非激活通知。两端 USER 同步逻辑 foreground/active/focus；首次点击可激活并交付给 guest |

物理键码使用 US 键位对应的 VK，文本使用宿主提供的实际字符；两者不相互推导。退格与数字键盘 Enter 分别规范为 U+0008/U+000D，AppKit 功能键私用字符不作为文本交付。Win64 另支持宿主事件驱动的前台 Raw Input 逻辑键鼠设备，见[raw/IME 策略](input-profile.md)。IME composition、dead-key 状态机、SendInput 或 hover/leave 尚未新增；布局 API 仍为[虚拟 US profile](keyboard.md)。

native 鼠标位置统一为逻辑 client 像素，以带符号的两个 WORD 打包，窗口缩放不改变 guest 像素单位。两个 guest adapter 用窗口的 client origin 转为 guest screen 位置，更新逻辑游标并保存当次 MSG.pt；滚轮的 lParam 使用 screen 位置，其余鼠标消息使用 client 位置。Win64 继续应用 ClipCursor 并按有符号 DWORD 扩展 lParam，guest HWND 不经过 u32 截断；PE32 保持原来的无裁剪游标策略。后续移动不会改写已入队消息的位置。

Metal drawable 使用 native view bounds 转换后的实际 backing 像素尺寸，每次获取保留图层时同步当前屏幕的 backingScaleFactor。它与 guest client 像素单位分开：Retina 上 1280×720 点的窗口对应 2560×1440 drawable，输入中心仍为 guest 的 (640,360)。旧实现把 guest 尺寸写成 drawableSize，在 Retina 呈现中造成画面裁剪；人工试用后已修复。

共享 Keyboard owner 在收到硬件事件时更新 GetAsyncKeyState。只有取出消息（GetMessage 或 PeekMessage 的 PM_REMOVE）才更新当前线程的 GetKeyState/GetKeyboardState；PM_NOREMOVE、输出故障和 PostMessage 不模拟一次硬件消费。左右修饰键单独存储，通用键的 high bit 为两侧的 OR；自动重复不重新设置异步的“自上次查询后按下”低位。中键和侧键按 VK_MBUTTON/VK_XBUTTON1/VK_XBUTTON2 处理，滚轮不会伪造按键变化。

队列容量维持现有策略：native ring 为 512 个槽（最多保留 511 个事件），PE32 USER 为每线程 10000 条，Win64 USER 为 4096 条；native ring 满时丢弃后来的事件。每次 Presenter 最多消费 512 条，轮询间隔上限 16ms，实际输入延迟仍受解释执行与 host 调度影响。当前不承诺任意输入洪泛无损，也未增加 child focus/capture 重定向。

验证入口：

```sh
cargo test --release --locked --test win32 --test x64 input_events
python3 -B tools/probe_input.py --out NEW_OUTPUT_DIR
```

前者通过真实 guest 窗口创建和消息 API 验证左右修饰键、五键状态、消息移除边界、重复按键、坐标快照、双轴滚轮和系统字符代理对。后者在独立 NSView 上调用生产 AppKit 回调，使用原创事件检查准确的消息字段、缩放、功能键过滤、Unicode、修饰键组合、拖动、小数滚轮及失焦释放；不向其它应用投递事件。输出保存构建/运行日志、命令、源码和 binary SHA-256。这些是回调及 guest 消息契约验证，不代表已完成真实游戏菜单的交互验收。

`tools/probes/ui_geometry.m` 另在真实窗口验证启动尺寸→横屏→竖屏→横屏、保留同一个 MetalLayer、实际 backing drawable 与输入中心。可传入 native graphics 和 MoltenVK 两个绝对路径，使用原创四色/边框 texture 经过真实 Vulkan swapchain 呈现，并在每次 Present 后再次核对 drawable 尺寸。旧实现的失败日志为 `ui-geometry-backing-before.txt`（314×211 对 628×422），修复后日志为 `ui-geometry-gpu-final.txt`，均位于本轮证据目录。

`examples/guest_input_probe.rs` 使用原创 PE64 窗口过程、真实 Presenter64 和消息循环，在独立窗口绘制蓝色区域。真实 OS 点击依次经过 native poll、guest 入队、PeekMessage 移除和 DispatchMessage，guest WNDPROC 将原始参数写入有界 guest-memory log；工具读取该 log 独立检查抬起点位于蓝色区域。实际点击验证了 (200,130) 和完整 down/up 通路，日志为 `guest-input-focus.{out,err}`。这与直接调用 native 回调、手动向 guest 队列插入事件不同，但仍不能代替真实游戏菜单的行为验收。

可显式设置 `WIRE_INPUT_TRACE=1` 诊断 Win64 输入的 native/queued/peeked/removed/dispatch 阶段，输出完整 guest HWND、消息、wParam/lParam 和 tid。普通事件最多 4096 条、移动最多 512 条；默认关闭，不抓取屏幕或其它应用输入。激活事件也进入该通路，便于区分未获焦点、事件未交付和应用未响应。

`guest_input_probe --raw` 另由原创 guest WNDPROC 调用 GetRawInputData、检查真实鼠标 down/up 标志并调用 DefWindowProc 清理。真实点击 (200,130) 通过，日志为 `guest-input-raw.{out,err}`；队列诊断增加 raw-queued/raw-read/raw-buffer 阶段。该模式不使用游戏素材，仍需另行验收游戏菜单的实际行为。

Win64 可通过 EnableMouseInPointer 显式启用鼠标指针消息。该进程设置只允许调用一次，重复设置返回 ERROR_ACCESS_DENIED，默认仍使用普通鼠标消息。启用后，宿主移动、五键组合和滚轮转换成 WM_POINTERUPDATE/DOWN/UP/WHEEL/HWHEEL，保持单一鼠标 pointer ID 1，lParam 使用 guest screen 坐标。首次按下和最后松开改变 contact 状态，其余按钮变化使用 UPDATE；Raw Input 仍独立交付。NOLEGACY 抑制普通键鼠消息，不抑制已启用的指针消息。

GetPointerDevices 按当前没有笔、触摸或触控板 digitizer 的配置返回经过检查的零条列表，不改数据缓冲区。GetPointerType 返回已交付鼠标的 PT_MOUSE，GetPointerInfo 写出完整 96 字节 Win64 POINTER_INFO，包括完整 HWND、屏幕位置、按钮变化和消息时间。查询读取当前线程最近取出或窥视的指针消息快照；后续宿主事件不会覆盖正在处理的 down/up。取消息的输出检查成功后才发布快照，只有移除才提交线程按键状态；窗口销毁和线程退出清理快照。未交付/错误 ID 返回 ERROR_INVALID_PARAMETER，写出故障不改可访问前缀。该子集不提供触摸、笔、历史查询、enter/leave 或 DefWindowProc 向普通鼠标消息的回退；PE32 指针接口保留 ABI-only。

`guest_input_probe --raw --pointer` 由原创 guest WNDPROC 同时读取 Raw Input 和 GetPointerInfo。真实 OS 点击 **(200,130)** 已验证指针 down/up、各自快照和原始数据包，日志为 `guest-input-pointer-live.{out,err}`。回归覆盖快速入队的 down/up、窥视/移除边界、五键组合、负向滚轮、完整 HWND 和原子输出故障；真实应用交互仍单独验收。

`examples/guest_state_probe.rs` 可在执行切片之间只读观察指定 guest 字节，并仅在地址或内容变化时打印。`--watch 'NAME@ADDRESS/OFFSET:SIZE'` 先读取 ADDRESS 处的 64 位指针，再加 OFFSET；多级 `/OFFSET` 依次解引用，不带 `/` 则直接观察 ADDRESS。每条最多八级地址、256 字节，最多 64 条；使用 Memory::observe，不触发 guest guard 或写入状态。`--slice` 控制观察间隔，短于该间隔的状态可能遗漏。该工具需要显式提供 EXE、图形后端和 MoltenVK 路径，可用 `--desktop-size` 选择虚拟桌面，不抓帧；观察会改变执行速度，不能用来比较性能。
