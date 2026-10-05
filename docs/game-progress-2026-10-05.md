# 越过 SEH 与窗口初始化，推进到插件加载（2026-10-05）

本轮按用户要求先推进游戏，再复测性能。目标从原来的约 **21.43M** 指令 RtlUnwind 阻塞，
推进到约 **24.40M** 指令的插件初始化失败路径。
`TVPQueueWindow`、`TVPTimerWindow` 已建立窗口对象，执行真实 guest WndProc 创建回调并注册定时器。
当前游戏自己的 catch 报告 **`Fatal Error: Not Susie Plugin`** 后退出。
这不是游戏运行成功：未进入可见主循环、未呈现画面，也未确认读取 data.xp3 内容。M2 不验收。
此前 x87 profile/优化保存为检查点 `f97ab8d`，见 [CPU profile](cpu-profile-2026-10-05.md)。

## SEH 展开与回调控制权

- `kernel32/ntdll!RtlUnwind` 检查 TEB 栈范围、4B 对齐、单调注册链及目标可达性，再执行清理。
  target 保持安装且不作为 global unwind handler 调用；每个内层 handler 看到当前 FS:[0] 指向自己，
  返回 ContinueSearch 后才从链上移除。
- 使用传入的 EXCEPTION_RECORD 并保留原 flags；添加 UNWINDING，null target 添加 EXIT_UNWIND。
  null record 构造 STATUS_UNWIND。处理器收到可访问的 CONTEXT 和 dispatcher 指针，返回 EAX 设置为 ReturnValue。
- 32 位 ABI 恢复调用者的 EIP 和 stdcall 栈位置；guest MSVC helper 再执行局部析构并切换到 catch 栈。
  `TargetIp` 在该 ABI 中未使用，不能套用 x64 continuation 行为。
- `call_guest` 区分 Returned、Transferred、Exited。回调边界绑定 owner TEB 和调用前的栈上界；
  当非局部转移丢弃回调栈，保留新的 guest 现场并退出宿主回调，不再恢复旧 RaiseException/CRT/窗口 API 栈。
  回调内 IR 逐条返回边界以便立即识别栈转移，普通主循环保留连续 IR 调度。
- collided/nested unwind 的特殊 disposition/注册链重写仍明确拒绝；没有实现自动 CPU fault → SEH、
  完整 Windows 栈增长或 x87 #MF 递送。

对照依据为 [Microsoft RtlUnwind 接口文档](https://learn.microsoft.com/en-us/windows/win32/api/winnt/nf-winnt-rtlunwind)
及 [Wine 的 i386 ABI 实现](https://github.com/wine-mirror/wine/blob/master/dlls/ntdll/signal_i386.c)。
后者文件头确认 LGPL-2.1-or-later；这里只核对 ABI、独立实现 wire 代码，未导入 Wine/QEMU 实现。
独立 guest 测试覆盖两个内层帧、target 排除、FS 链逐帧更新、记录 flags、null record/exit unwind、
非法目标/循环/栈外注册，以及 catch 同时跳出 RaiseException 和 `_initterm` 两层宿主回调。
尚未在原生 Windows 上跑专用 ABI 对照程序，不声称完整 Windows unwind 等价。

## 窗口与消息子集

`src/user32.rs` 增加进程类表、窗口对象、线程消息队列和定时器：

- Register/Get/UnregisterClass 的 A/W、Ex/非 Ex 结构，atom 与名称查询、模块归属、重复注册错误；
  类名/菜单字符串保存为稳定 guest 字符串，ANSI 使用 CP932。
- CreateWindowEx 将完整 x86 CREATESTRUCT 传给 WM_NCCREATE/WM_CREATE；检查 guest 返回值，拒绝时清理对象。
  A/W 调用与类编码不同则转换 CREATESTRUCT 字符串。DestroyWindow 发送 WM_DESTROY/WM_NCDESTROY 后移除对象和 timer。
- window long 的 WndProc、instance、parent/id、style/exstyle、userdata 及经过边界检查的 extra bytes；
  同线程 SendMessage、CallWindowProc、队列 DispatchMessage 真实执行 guest 回调。
- PostMessage/PostThreadMessage、Peek/GetMessage 的基本 hwnd/范围过滤、PM_REMOVE 和 WM_QUIT；
  Peek 无消息时保留输出内存，GetMessage 等待由协作线程调度处理，另一线程 post 可唤醒，未伪造退出消息。
- Set/KillTimer 保存 owner/id/周期/deadline，生成并合并 WM_TIMER；已入队的 WM_TIMER 不因 KillTimer 删除。
  DispatchMessage 对带 TIMERPROC 的消息传递正确 id/time 参数；销毁窗口清除所属 timer。

这是逻辑窗口/基础队列子集。平台窗口呈现、GDI、输入/键盘翻译、内建控件、完整 nonclient/z-order、
子窗口过滤/生命周期及消息优先级还未完成；没有将 ShowWindow 的逻辑 visible 位当成屏幕呈现。
跨线程同步 WndProc/DispatchMessage 暂明确拒绝，跨线程销毁按失败处理；PostThreadMessage 已支持线程队列。
基本 WM_NCCREATE 默认返回 TRUE，其他已使用的默认消息无应用动作；不是完整 DefWindowProc 实现。

完整 `cargo test --release` **79 项通过**：2 unit、34 differential、5 FPU、10 IR、1 Wasm、27 Win32。
七项新增 Win32 回归覆盖上述 SEH/回调、类元数据、窗口创建/转发/销毁、消息过滤/退出/计时器，
以及实际 guest 工作线程唤醒阻塞 GetMessage。release build、Clippy all-targets、format/diff 检查通过。

## 新阻塞及下一步

游戏现在执行到 `LoadLibraryW(C:\plugin\kinglove.tpm)`。旧兼容层仅生成一个模块句柄，
GetProcAddress 对所有请求都生成 API stub，因此 V2Link 未运行，还错误地提供了本来不存在的 GetPluginInfo 等导出。
游戏进入 Susie fallback 并报告 `Not Susie Plugin`，然后完成异常清理和退出；exit code 0 只表示程序选择这样结束。

只读 PE 检查确认该文件为 PE32 DLL，首选基址 **0x10000000**、SizeOfImage **0x10000**、有 base relocation，
实际导出仅 **V2Link/V2Unlink**，没有 GetPluginInfo。首选基址与当前 guest heap arena 冲突。
下一步应实现通用 guest DLL 装载：选择不冲突的地址、HIGHLOW relocations、imports/真实 exports、
模块表及 DllMain/CRT 初始化；缺失导出必须返回 NULL，不能继续伪造为宿主 API。
用自编译 DLL 先验证重定位、数据/代码导出、初始化、依赖及循环/失败边界，再推进真实插件。
新装载的插件代码应先走现有 raw fallback，后续再考虑模块级安装 IR。

日志保留于忽略目录 `wire-out/game-unwind/`；`first` 为 unwind 后的 Unknown error，
`classes`/`windows` 为中间阶段，`timers` 为首次 Not Susie Plugin 路径。
`kinglove-pe.txt` 仅保存 PE 元数据检查结果，未将游戏代码或资产加入仓库。

## 推进后的 CPU 复测

同一完整目录、release、50M 预算、关闭 CPU trace/opcode stats。三次 IR 的运行期 sample
只统计 Cpu::run_loop 后代，合计 **2,938** 个样本：

| 类别 | 当前阶段占比 |
| --- | ---: |
| 软件 x87 | 42.65% |
| IR 执行与缓存 | 34.96% |
| 其他 raw fallback | 10.99% |
| API（CPU stepping 外） | 6.47% |
| 外层调度/其他 | 4.94% |

未再采到反复解析浮点常量的热点。x87 与 IR 执行/缓存仍是主要优化方向。
因为游戏路径已经延长，这些占比不能与前一阶段直接构造成加速比。
原始样本、guest report 和火焰图位于 `wire-out/game-stage-profile/run-*` 与 `summary/`。

无 profiler 的 IR/raw 交替对照保存在 `wire-out/game-stage-profile/bench-*` 和 `bench-summary.json`。
guest MIPS 排除 PE/IR 绑定，wait4 CPU 时间包含整个进程；采用同一份最终可执行文件，仅切换 `--ir`。

| 对（顺序） | raw guest MIPS | IR guest MIPS | raw 进程 CPU 秒 | IR 进程 CPU 秒 |
| --- | ---: | ---: | ---: | ---: |
| 1（raw/IR） | 21.49 | 27.53 | 1.098 | 0.948 |
| 2（IR/raw） | 21.32 | 27.18 | 1.086 | 0.956 |
| 3（raw/IR） | 22.48 | 23.56 | 1.064 | 1.071 |

IR 中位数 **27.18 MIPS**，raw 中位数 **21.49 MIPS**。
三对全部最终 GPR/EIP/EFLAGS 相同，但 guest 计数在 24,396,743…24,398,606 之间、API 次数也略有差异，
不能称为严格同路径。每对 MIPS 优势约 5%…28%；第三对的全进程 CPU 时间没有改善，调度波动仍明显。
不把前一阶段的 18.76 与当前 27.18 直接相除作为这轮功能改动的加速比。
