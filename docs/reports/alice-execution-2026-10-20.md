# Alice 原始 guest Mono 执行接续（2026-10-20）

基于重构交付 `85ced07` 和音频验证收尾继续长启动重放。已依次越过 SRW 条件变量、文件定位、64 MiB 同步读取、256 MiB 纹理创建/映射、OpenThread、FPREM 和 shader varying 子集匹配。`varying-subset` 重放已显示标题菜单，随后在约 15.56B work 遇到 MFENCE 缺失。补齐内存屏障及 PE32/Win64 宿主键鼠事件后，有界 `input-events` 重放持续到 600 秒时间上限，最后采样为 24.05B work，没有记录新的执行错误。标题显示不代表可玩性或菜单交互已验收。

## 输入和逐阶段证据

继续使用 `wire-out/native-mono-probe-state/runtime`，执行游戏自带的 PE Mono，不重建原生 Mono 桥接，不复制或硬链接游戏资产。EXE、UnityPlayer、Mono 和 MoltenVK 的 SHA-256 保持既有输入；前四次使用原 native graphics 库，从 `atlas` 起显式使用本轮新建的库，原库未覆盖。

| 重放目录 | retired / work | 完成 API | Draw / Present | 本次终点 |
| --- | --- | ---: | --- | --- |
| `baseline` | 4,571,896,502 / 4,580,185,429 | 7,623,481 | 391 / 140 | SleepConditionVariableSRW 未实现 |
| `condition-srw` | 4,904,044,310 / 4,912,931,711 | 8,157,493 | 470 / 157 | SetFilePointer 未实现 |
| `file-position` | 5,032,243,648 / 5,041,360,428 | 8,364,952 | 541 / 165 | 64 MiB ReadFile 超过旧限制 |
| `large-read` | 5,048,153,428 / 5,057,280,491 | 8,373,620 | 535 / 165 | 256 MiB texture 分配失败，随后 NULL resource 创建 SRV |
| `atlas` | 5,042,736,612 / 5,051,864,981 | 8,375,439 | 509 / 154 | texture Map 超过旧 512 MiB guest 总提交限制 |
| `texture-map` | 6,622,090,529 / 6,632,637,616 | 9,783,822 | 1250 / 235 | OpenThread 未实现 |
| `open-thread` | 7,306,683,252 / 7,318,206,239 | 10,602,013 | 1517 / 272 | FPREM 未实现 |
| `fprem` | 11,433,751,277 / 11,447,129,331 | 12,290,251 | 1762 / 300 | shader I/O location mismatch |
| `link-diagnostic` | 11,298,525,278 / 11,311,872,720 | 12,261,990 | 1767 / 301 | 定位 float4 → float3 接口被错误拒绝 |
| `varying-subset` | 15,540,894,105 / 15,556,029,395 | 14,010,345 | 12540 / 1122 | 标题菜单已显示，随后 MFENCE 未实现 |
| `input-events` | 24,032,027,498 / 24,050,000,000（末次采样） | 16,781,650（末次采样） | 无最终汇总 | 600 秒时间上限，SIGTERM；未记录新执行错误 |

所有目录位于 `wire-out/alice-resume-2026-10-20/`，分别冻结小型 wire binary、source diff/hash、新增源文件、准确 command/cwd、输入 hash、Users 前后 hash/大小、Player.log、原始报告及最近一次实际 Present 的 PPM；正常诊断退出另有聚合 API inventory 与最终汇总。运行使用排他锁，前六次预算 10B work/240 秒，`open-thread` 至 `varying-subset` 为 50B/900 秒。前十次均因具体错误提前退出，没有触发预算或 wall timeout。`input-events` 使用 25B/600 秒预算，先触发时间上限，因此表中数值来自最后一次周期采样，并非退出时的最终计数；没有最终 Draw/Present/清理汇总。该阶段的冻结源码与后续仅涉及匹配语法及 F20 键位的收尾差异分别保留。

这是带真实窗口、GPU 读回、API inventory 和真实时钟的功能验证，不作性能 AB、FPS 或逐指令一致性结论。各次已完成路径会受线程和计时影响，不能按 work 差值判断性能。

## 已补齐的行为

- `SleepConditionVariableSRW` 使用合作线程队列，按原模式释放/重取 SRW；single/all wake 不积累信号，通知与超时都需等锁可取才能返回。PE32 保持 ABI-only/error 120 和 16 字节清栈。见[同步边界](../architecture/inline-sync.md)。
- `SetFilePointer` 正确合成有符号低/高 DWORD、处理 origin、高位输出和 `0xffffffff`/LastError 约定；输出检查先于 seek。`SetEndOfFile` 在 cursor 处真实截断/扩展，保留 cursor，并拒绝存活 section/view。见[文件边界](../architecture/files.md)。
- 普通文件的大块同步 `ReadFile` 在完整输出预检后使用 1 MiB scratch 分段读取；短读/EOF 返回实际数量。管道和 OVERLAPPED 限制保持，未吞掉 I/O 错误。
- 单个 texture 的总容量上限统一为 256 MiB，包含 mip/layer/depth。大于 64 MiB 的原生传输按不超过 8 MiB 的行带同步完成，通用 buffer 上限保持。NULL resource 创建 view 返回 E_INVALIDARG，并清零有效输出槽。见[host 图形边界](../architecture/host-graphics.md)。
- guest 总 committed 容量统一为 2 GiB，单次 map/commit 仍为 256 MiB；普通 map、image merge、VirtualAlloc 和内存状态查询使用同一容量检查。纯写 texture Map 直接使用已清零的新页，省去整块零数据的临时副本。见[虚拟内存边界](../architecture/virtual-memory.md)。
- `OpenThread` 打开真实的 guest 线程身份，使用独立的 alias/继承标志，并复用既有 live/lifetime 容量。活动线程可在原句柄关闭后重开；退出线程需有保留的句柄。未知 ID、容量不足或不支持的权限位明确失败。沿用现有 profile 的权限检查限制，见[线程边界](../architecture/thread-properties.md)。PE32 保持 error 120/12 字节清栈。
- AMD64 `FPREM` 使用共享 APFloat FP80 状态，保留扩展精度，按指数差分步约简并设置 C2，完成时写商绝对值的低三位条件码；不依赖 CW 精度/舍入模式。特殊值沿用现有异常掩码和状态处理，未增加延迟异常投递；新的未屏蔽异常仍停止并回滚 CPU。
- native shader 接口在设备实际支持并启用 maintenance4 时接受同 scalar 类型的 producer 向量子集，如 float4 → float3；缺失 location、类型不同或组件不足仍拒绝。
- AMD64 `MFENCE/LFENCE/SFENCE` 可正常退休。当前 guest 线程合作执行，访存在下一条指令前完成，没有需要排空的 guest store buffer；不额外引入 host 原子内存状态。
- PE32/Win64 共用的宿主键鼠桥补齐物理键码/扫描码、左右修饰键、五键鼠标、拖动和双轴滚轮；文本保留 UTF-16 并在 TranslateMessage 时交付。两端处理 client/screen 坐标并保存 MSG.pt，状态消费保持在消息移除后。实现范围、容量与剩余输入限制见[输入边界](../architecture/input-events.md)。

当前累计 export 契约 **922 项**：重构交付 919 项，接续新增三个 export；FPREM 属于 CPU 指令能力。

## 原生构建和验证

大纹理库 `libwire_graphics-atlas.dylib` 保留，SHA-256 `a09544af5a1e4885234ab166f777a914342fe90d8dad519017d7f89ee1e0d287`；当前使用 `wire-out/alice-resume-2026-10-20/libwire_graphics-link.dylib`，SHA-256 `25863b0b48a28b719bb433205ab47230a6c91f4240547c9ef9c34ac9e18d6e6a`。相邻 `.build.json` 保存完整构建命令、native 源码及静态库 hash。依赖为 dxbc-spirv `c7f069701227dcb800a01bd93e42f8001b7543e7`、SPIRV-Headers `c8ad050fcb29e42a2f57d9f59e97488f465c436d` 和 Vulkan-Headers 1.4.357；归档 hash、Meson 配置、构建和上游 199,608 项断言零失败的日志位于 `native-deps/`。

显式原生验证通过：256 MiB 完整 pattern 上传/读回 SHA-256、超限 texture 拒绝、view 保留 texture/device；两个 DXGI fixture；资源 probe 的 3360 字节/512 clear pixels/真实 queue timestamps。扩展 DXGI fixture 另验证超过旧 512 MiB 总提交限制的 texture Map、8 MiB 传输边界两侧及末尾像素、Unmap/Release 清页和输出故障不分配，清理报告 0 MiB 残留。使用最终 link 库重新执行大纹理/DXGI fixture 和图形 probe 均通过；原创 varying probe 的 4096 像素全部正确，三种非法接口均拒绝且 target 不变，清理为 0 MiB。资源 probe 早先仍报告 1 MiB 残留，`varying-subset` 游戏重放报告 11 MiB；保留日志，原因尚未归因。

文件 20 项定向回归、内存容量和状态查询、OpenThread 的 owner/Win64/PE32 回归、FPREM 三项回归均通过，逐阶段 strict Clippy/build 通过。FPREM 有独立精度/大指数差/特殊值/回滚断言和 35 组有限输入的参照对照；参照引擎不能正确保留部分精确零余数的符号，因此该符号由独立架构断言验证，未以参照结果覆盖它。

关键日志为 `file-family-tests.txt`、`large-read-tests.txt`、`atlas-native-test.txt`、`atlas-dxgi-tests.txt`、`atlas-resources-probe.txt`、`commit-capacity-tests.txt`、`commit-map-native-tests-final.txt`、`open-thread-tests.txt` 和 `fprem-tests-final.txt`。初次 Clippy、context Release 测试名称以及负商条件位的失败日志保留；源码修正后的最终日志另存。最早 SRW-only 的 `verification.json` 作为历史记录保留。

输入与屏障加入后的完整 release 全目标为 **715 passed / 0 failed / 9 ignored / 21 suites**，工具 **31 项**通过，strict Clippy、build 与 fmt 通过。四个新增 guest 输入回归覆盖真实窗口、左右修饰键/五键状态、移除边界、重复、坐标快照、双轴滚轮和系统字符代理对；共享 owner 另检查完整 tid 隔离及非法键值，CPU 检查三个 fence 的退休和架构状态。原生输入探针检查生产回调的准确事件字段及失焦释放，通过 `-Wall -Wextra -Werror` 构建。日志为 `input-all-targets-tests.txt`、`input-tools-tests.txt`、`input-clippy-final.txt`、`input-build-final.txt`、`input-fmt.txt` 和 `input-native-f20-final/`。回调探针没有向真实游戏注入输入，尚未验收菜单点击。

已有 `host_ui_probe` 也使用最终 AppKit 桥显式通过主线程约束、真实窗口 upload/failure、resize/removal 和抓帧，日志为 `input-host-ui-test.txt`。本轮累计证据清单为 `continuation-verification.json`，记录源码、最终 binary、后端和验证日志 hash，并关联各阶段结果；时间上限阶段另外保存 `observed-progress.json`，明确区分周期采样和最终计数。

## 当前执行边界

`varying-subset` 已显示标题菜单，随后在 guest JIT 地址 `0x120c112da` 的 MFENCE 停止。MFENCE 已补齐，`input-events` 的末次采样达到 **24.05B work**，没有记录新的 API/指令错误，随后由 600 秒时间上限终止。尚未验收真实菜单输入。Settings.json 与 GlobalSave.nson 仍为零字节，未重置，也未验收完整保存。Player.log 仍包含音频设备不可用、部分 shader 不支持和网络初始化失败；不能由当前绘制次数推导这些能力已补齐。

## 人工试用后的修复

不带诊断抓帧和 wall timeout 的 `interactive-input-s_szph6g` 运行在 **13,075,195,990 work**、**5839 Draw / 602 Present** 后报告 `HeapReAlloc: unsupported Win64 heap/flags`。fault ABI registers 明确为 flags `0x10`，旧 block 大小 `0x417`、请求 `0x817`；这是合法的原地址扩容请求。当前 HeapReAlloc 已允许该专用标志：capacity 内保留地址并按 ZERO_MEMORY 清零新增部分，超出 capacity 则返回零且保留原 block、页数、内容和 LastError，供调用者选择其它分配路径。slab 和独立页 block 的边界、失败、缩小到零及清零回归已通过。

用户同时报告画面裁剪/只显示窗口一部分。独立窗口探针复现旧 drawable 为 **314×211**，但实际 Retina backing 为 **628×422**。native view/layer 的 point frame 本身会正确随 resize 更新；问题在于旧 `drawableSize` 使用 guest 像素尺寸，未匹配实际 backing 像素。现已改用 view bounds 的 backing 转换，并同步当前屏幕 scale，输入继续映射逻辑 guest client 坐标。真实 Vulkan/Metal 探针用原创四色和边框图验证 1280×720 → 480×640 → 1280×720，对应 drawable 2560×1440 → 960×1280 → 2560×1440，每次 Present 后均保持一致，输入中心分别为 (640,360)/(240,320)。这不会把 guest 桌面强制改成某个宽高比。

修复后完整 release 为 **716 passed / 0 failed / 9 ignored / 21 suites**，strict Clippy、build、fmt 和原生输入回调探针通过。日志为 `interaction-fix-{all-targets-tests,clippy,build,fmt}.txt`、`heap-in-place-tests.txt`、`ui-geometry-backing-before.txt`、`ui-geometry-gpu-final.txt` 与 `interaction-fix-input-native/`。新版本已在 `interactive-retina-rczhu244` 启动供再次试用，冻结 binary、变更、命令、PID 与 Users 启动前 hash；不开抓帧、不设 wall timeout。真实菜单点击仍需该次试用验证。

从仓库根目录使用当前构建继续，输出必须使用新的目录名：

```sh
CARGO_INCREMENTAL=0 cargo build --release --locked
WIRE_REPLAY_WORK=50000000000 WIRE_REPLAY_TIMEOUT=900 \
  python3 wire-out/alice-resume-2026-10-20/replay.py NEW_STAGE \
  wire-out/alice-resume-2026-10-20/libwire_graphics-link.dylib
```

修复版再次试运行已越过上一轮 13.08B work 的退出位置，继续运行且尚无报告的 API 错误，周期采样见 `interactive-retina-rczhu244/readiness-observation.json`。这不是退出汇总，也不替代用户的菜单点击验证。本次修复清单保存于 `interaction-fix-verification.json`；GPU 几何探针在 device teardown 报告 29 MiB 残留，未宣称清理为零。


## 激活通知与第二次人工试用

`interactive-retina-rczhu244` 后来在 **17,301,084,041 work**、**16327 Draw / 1423 Present** 停止，错误为 `FreeLibraryAndExitThread` 未实现，HMODULE 为 `0x180000000`、exit code 为零。这是新的执行失败，不能把此前越过旧 work 停点称为点击已成功。该入口现通过 Modules 释放一次引用，再走原来的非返回线程退出；不扩展最后一个未 pin guest DLL 的卸载能力。scheduler/返回帧失败不释放，引用只释放一次，完整 handle、DWORD exit code 和 PE32 ABI-only 清栈均有回归。

同时发现 native key-window 变化没有交付 guest 激活/焦点消息。现补齐 WM_ACTIVATEAPP、WM_ACTIVATE、WM_SETFOCUS/WM_KILLFOCUS，并在 PE32/Win64 USER 中同步逻辑所有权；view 接受首次鼠标点击。没有实现 child focus/capture 重定向。两端回归分别检查完整 HWND、owner 与消息字段；native fixture 检查通知顺序、去重和失焦释放。

原创 `guest_input_probe` 通过真实 OS 鼠标点击运行：蓝色区域为 guest client [100,80..300,180]，guest WNDPROC 实际收到 down/up 坐标 **(200,130)**，诊断明确记录 native→queued→removed→dispatch。初版探针只验证调用链，不能验证真实游戏的业务响应。该进程使用独立原创 PE64 和像素，不读取游戏画面。运行日志为 `guest-input-live.{out,err}` 和 `guest-input-focus.{out,err}`，构建与源码保留。

最新完整 release 为 **719 passed / 0 failed / 9 ignored / 22 suites**，新增的 example 增加一个零测试 suite；strict Clippy、build、fmt、native 输入 probe 通过。日志为 `focus-all-targets-tests.txt`、`focus-clippy.txt`、`focus-build-final.txt`、`focus-fmt.txt`、`free-exit-tests.txt` 和 `focus-native-input/`。当前 `interactive-focus-dow6qnga` 开启有界 `WIRE_INPUT_TRACE` 运行，仍未把游戏菜单点击标记为验收。


## 第三次点击反馈与 Raw Input 修复

用户确认 `interactive-focus-dow6qnga` 仍没有菜单响应。该次 down/up 已完整经过 native→queued→removed→dispatch，激活通知也成功交付；因此不能继续把窗口过程收到普通鼠标消息当成交互成功。该实例人工终止前末次采样为 **44.95B work**，没有报告新的执行错误，日志和 Player.log 已保留。Player.log 明确包含三次 Raw Input 注册失败。检查 UnityPlayer 的实际调用点 `0x180a54f38`、`0x180a55120` 和 `0x180a65070` 确认注册是 usagePage 1 的 mouse/keyboard，flags 为零；程序同时引用单包读取和 GetRawInputBuffer，不可只让注册返回成功。

Win64 现由真实宿主键鼠事件生成 WM_INPUT，提供稳定逻辑设备、完整 target HWND、注册查询、设备查询、GetRawInputData 的 INPUT/HEADER 两阶段读取，以及当前线程的缓冲读取。窗口目标销毁和 DefWindowProc 清理实际数据包；输出故障不吞事件，已从消息队列移除的包不会再次被缓冲读取。NOLEGACY 抑制对应普通键鼠/字符消息并在 raw 消费时更新线程键状态。后台捕获和 HID 仍明确拒绝；PE32 raw 保持 ABI-only，已有 PE32 普通输入能力不受影响。详细布局及边界见[输入 profile](../architecture/input-profile.md)。

新增两个 guest 回归检查独立预期字节布局、五键/负滚轮/E0/系统键、完整 HWND、包生命周期、缓冲读取/消息移除边界和写出故障回滚。原创 `guest_input_probe --raw` 另通过真实 OS 点击：guest WNDPROC 调用 GetRawInputData 读取 down/up，并调用 DefWindowProc 清理；实际蓝色区域点击为 **(200,130)**。日志为 `guest-input-raw.{out,err}`。完整 release 为 **721 passed / 0 failed / 9 ignored / 22 suites**。普通沙箱运行的 Unicorn 参照测试被 SIGILL 终止，允许其 JIT 执行后全套通过，最终日志为 `raw-all-targets-tests-final.txt`；这不把第一次失败计为通过。真实游戏的菜单响应仍须新的执行结果确认。

修复版 `interactive-raw-1v_8mws6` 已启动供人工试用，PID 25455，保持排他锁、不抓帧、不设 wall timeout。输入初始化后的日志已不再出现 Raw Input 注册失败；就绪采样没有报告新的执行错误。真实菜单点击结果仍待反馈；当前不把启动成功或初始化日志变化记为菜单验收。strict Clippy、最终 release build 和 fmt 均通过，分别见 `raw-clippy.txt`、`raw-build-final.txt` 和 `raw-fmt.txt`，累计清单为 `raw-verification.json`。


## Raw Input 试用仍失败与桌面尺寸诊断

用户确认 `interactive-raw-1v_8mws6` 的菜单仍无响应且画面仍有裁切。该次 22 个实际 click down/up 数据包已能关联到 GetRawInputData/GetRawInputBuffer 消费，记录在其 `result.json`；注册失败消失不等于菜单已修好。真实最终 GPU 呈现的原创四色探针经过窗口截图核对，四边白框均完整，日志为 `ui-geometry-live.{out,err}`。该探针不读取游戏素材。

额外的 `interactive-raw-6hr1e74i` 仅收集尺寸数据，确认 client 与 backbuffer 都是 **1024×720**，并非本轮 drawable 再次只呈现一角。原虚拟桌面仍固定 **1024×768**，不足以容纳程序默认的 1280×720 宽度；这一限制是裁切的待验证原因。新 `probe64 --desktop-size WIDTHxHEIGHT` 可在执行前选择统一的虚拟桌面，覆盖 Win32 显示查询、DXGI 输出/模式、GDI 分辨率、游标裁剪及初始窗口约束，不静默修改存档或原默认值。计划在同一隔离 runtime 以 **1920×1080** 继续，观察程序实际选择的窗口/纹理尺寸和菜单响应。尺寸诊断实例已保留结果并停止。

另确认首版 Raw Input 将宿主绝对位置从第二个事件才开始换算差值，会丢失首个位置。已改为 MOUSE_MOVE_ABSOLUTE | MOUSE_VIRTUAL_DESKTOP，并以同一虚拟桌面尺寸归一化；独立预期检查包含反解回原屏幕像素的位置。真实菜单尚未据此验收。

最终完整 release 回归为 **723 passed / 0 failed / 9 ignored / 22 suites**，strict Clippy、build、fmt 与 diff whitespace 检查通过。新 desktop 回归覆盖 Win32 metrics、ANSI/Unicode DEVMODE、monitor/work area、GDI 分辨率、clip、禁止执行后改尺寸及 DXGI desc/list/closest mode。绝对输入另按程序实际单精度解码路径检查 15,048 个像素位置，零偏差，参照常量经读取确认是 65535.0。日志见 `desktop-all-targets-tests.txt`、`desktop-clippy.txt`、`desktop-build-current.txt`、`desktop-input-tests.txt` 和 `absolute-coordinate-reference.json`。最后一次原创绝对输入 UI 试点未取得 PASS（自动化返回 noWindowsAvailable），不计入通过的真实鼠标证据；先前 guest 原始 Raw Input 点击通路验证仍保留。

最终实例为 `interactive-desktop-07wbm7c0`，PID 28150，显式使用 `--desktop-size 1920x1080`，无抓帧/无 wall timeout。仍需观察程序创建的实际尺寸及用户对裁切和点击的反馈，尚未标记游戏菜单验收。

最终实例已实际确认 **client=1280×720、texture=1280×720、Retina swapchain=2560×1440**，此前为 1024×720。尺寸改变来自显式虚拟桌面配置；仍等待标题加载完成后的菜单点击及视觉反馈。

## 2026-10-21 人工反馈与内部状态观察

用户确认 `interactive-desktop-07wbm7c0` **裁切消失，按钮仍无响应**。尺寸修复已获人工确认，菜单交互仍失败；普通消息和 Raw Input 的交付证据不构成按钮验收。该实例保存 `result.json` 和停止前 Player.log，核对 PID 28150 的完整命令后停止，没有重置 Users。

新增通用 `guest_state_probe` 读取指定 guest 字节，使用 Memory::observe，不向 guest 写入或替换函数。`interactive-state-322e8u46` 保持 1920×1080 虚拟桌面和原图形后端，按 1000 work 间隔观察内部坐标、鼠标 held/down/up 位与窗口状态，日志按变化输出。静态地址和指针链仅在本次启动命令中指定，未写入通用运行时。探针 release 构建与 strict Clippy 通过；其观察结果仍待实际点击，不能据启动成功推断原因。

用户随后完成点击。只读调试快照 `input-state-snapshot.json` 确认程序选择新输入后端，旧后端指针 `0x1820f6ca0` 为空，因此旧按键位不变化本身不能证明按钮丢失。沿新后端发现明确缺口：初始化在 `0x180a52fc8` 调用 EnableMouseInPointer(TRUE)，旧实现返回 ERROR_NOT_SUPPORTED；对应日志始终有启用失败。实际鼠标按钮处理 `0x180a55b34..0x180a55ba8` 读取 WM_POINTER 的按钮位，而 Raw Input 解析 `0x180a65180` 主要提供鼠标移动。此前仅补 Raw Input 未覆盖这条按钮路径。

现补齐通用 Win64 鼠标 pointer 消息、GetPointerType 和 GetPointerInfo，不在运行时写入软件名或静态地址。默认不开启，只有 guest 显式启用后转换宿主鼠标消息；查询保持每个已取出消息的快照。原创蓝色按钮通过真实 OS 点击，guest 实际读取了指针 down/up 快照并同时读取 Raw Input，日志为 `guest-input-pointer-live.{out,err}`。完整 release **725 passed / 0 failed / 9 ignored / 23 suites**，strict all-targets/all-features Clippy 通过；证据为 `pointer-all-targets-tests.txt` 和 `pointer-clippy.txt`。尚未据此宣称真实菜单已修好。

启用新路径后的两次短运行（`interactive-pointer-cmlp_dqw`、`interactive-pointer-va7haj1l`）在初始化时发生零地址调用，分别保存失败结果，不能计作成功重放。完整错误报告显示 RCX 为 count 输出指针、RDX/R8 为零；静态调用点 `0x180a5699d` 为新增进入的 GetPointerDevices 枚举分支。补上真实空 digitizer 列表，查询和复制均不虚构触摸/笔设备，并验证 count 输出故障及数据缓冲区保持。

最终完整 release 为 **726 passed / 0 failed / 9 ignored / 23 suites**，累计 **925 项 export 契约**。strict all-targets/all-features Clippy、release build、fmt 和 diff whitespace 检查通过；最终日志为 `pointer-all-targets-tests-final.txt`、`pointer-clippy-final.txt`、`pointer-build-final.txt` 和 `pointer-fmt-final.txt`。新实例 `interactive-pointer-iudzqwil` 已越过上述初始化失败，确认 client 和 texture 同为 **1280×720**，保持原排他锁和 Users 数据。`pointer-verification.json` 区分原创按钮的真实点击通过、实际菜单内部状态观察和仍待反馈的菜单验收；当前不据消息入队或程序持续执行宣称菜单成功。

该实例随后实际取出并分发用户的 pointer down/up。只读观察记录新后端 `+0x78` 从 **0000 → 0100 → 0000**，内部坐标也随消息更新；这证明本次点击进入正在使用的输入后端。消息在约 179 秒入队、225 秒才由程序处理，该延迟发生于初始化执行期间。尚未取得修复版菜单实际响应的人工确认。

用户随后反馈“点击任意处直接闪退了”。同一实例在 **13,014,106,274 work** 报告 `user32.dll!GetDoubleClickTime` 未实现，保存 `result.json` 和 `Player-after-failure.log`。这次是指针按钮处理已进入双击判定后出现的新执行失败。现为 Win64 user32 启用已有 GetDoubleClickTime 契约，共用 PE32 的固定 **500ms** 逻辑 profile，保留 LastError，不读取或修改宿主偏好。该 API 原已计入目录，累计契约仍为 **925 项**；回归同时检查静态/动态 provider 范围和实际返回/错误槽保持。

该修复后的完整 release 再次为 **726 passed / 0 failed / 9 ignored / 23 suites**，strict Clippy、build、fmt 和 diff whitespace 检查通过，日志为 `click-timing-{all-targets-tests,clippy,build,fmt}.txt`。新实例 `interactive-click-timing-cfrjj8m4`（PID 35751）使用冻结修复版、原图形后端和 1920×1080 虚拟桌面，继续只读观察输入；最终清单为 `click-timing-verification.json`。用户已确认**按钮有响应**，随后将推进“卡住”修正为**可以推进，但有卡顿**；不再将菜单输入记为待验收，也不据此宣称完整可玩性。

按用户要求，对同一实例保留进度采集了 20 秒主线程 profile。解释执行/调度/内存约 76.0%，诊断启动器约 14.6%，原生图形约 8.0%；浮点、管线创建和同步等待的细分及优化优先级见[性能采样报告](alice-profile-2026-10-21.md)。主线程占比包括等待，不是 GPU 利用率。此次未修改执行器或图形代码，也未重启窗口。


## 执行器优化与新的上下文查询

继续运行的 `interactive-click-timing-cfrjj8m4` 在 **44,455,752,095 work** 自行退出，错误为 `D3D.Context.OMGetRenderTargets` 未实现，结果和 Users 后置 hash 已保留。现补齐真实绑定查询、可选输出、重复 AddRef 与错误原子性，累计 export 仍为 **925 项**。

按用户要求，已接入 4–32 条混合 checked block，以及保留精确浮点异常语义的 ARM64 正常单精度快路径；跨调用链候选因性能退化撤下。完整 release **729 passed / 0 failed / 9 ignored / 23 suites**、独立 `wire-simd` **2 项**、strict Clippy、fmt 和 build 通过。冻结成对计时的混合 SSE/访存负载为 **1.733×**，各控制负载和波动见[执行器优化报告](executor-optimization-2026-10-21.md)，不当作游戏 FPS 收益。

新实例 `interactive-executor-6ftina92`（PID 39247）沿用原 runtime/Users、图形后端和排他锁，显式虚拟桌面保持 1920×1080；使用正常 wire 入口，关闭高频 watch、输入 trace 和 API inventory，无抓帧和 wall timeout。完整启动 manifest 独立保留；实际流畅度与新热点继续验证。

第一轮无高频探针版本已完成 20 秒采样：解释器/调度/内存 **71.09%**、图形 **27.02%**，真实 APFloat 运算调用仅 **1.03%**；55.17184 秒计数窗口为 **5351 Draw / 411 Present**，约 **7.45 次 Present/s**。场景与诊断配置不同，不能与旧实例推导受控收益。新的主要成本是块内与外层派发、访存，以及逐 draw 管线构建和 fence 等待。

随后精简 checked 标量事务的冗余备份，完整 release **730 passed / 0 failed / 9 ignored / 23 suites**，strict Clippy、fmt、build 通过；原独立 SIMD 两项测试仍适用。最终 600M work 三对长样本：混合 SSE/访存 **1.913×**、访存 **1.073×**、间接调用 **1.005×**；短样本的 wall/CPU 差异和波动全部保留。最终冻结为 `after-compact-rollback/`，新实例 `interactive-executor-lc2kgw54`（PID 40012）已沿用原 runtime/Users 重新启动。旧第一轮实例在 25.025B 周期观察后暂停以隔离计时，再恢复并以 SIGTERM 结束，保存 Users 后置 hash；不当作 guest 正常退出或完整保存。

随后验证了混合块预算前缀提交。所有块合并提交和允许内联的混合块候选分别使间接调用回退约 5% 与 7%，已撤下；将混合循环禁止内联并隔离代码段后，最终三版对照的混合长样本为原始版 **1.967×**、较精简回滚版 **1.052×**，控制负载较精简版约在 2% 内。完整 release **731 passed / 0 failed / 9 ignored / 23 suites**、strict Clippy、fmt 和 build 通过；独立 SIMD 两项源码未改，原验证仍适用。最终冻结为 `after-isolated-mixed/`，完整数据和代码生成取舍见[执行器报告](executor-optimization-2026-10-21.md)。

`interactive-executor-lc2kgw54` 的最后周期观察达到 **48.425B work**，未记录新执行错误，随后主动 SIGTERM，为最终构建让位，保留 Users 后置 hash；不能据 work 越过旧失败数值证明相同剧情路径已验收。最终实例 `interactive-executor-nscpv87h`（PID 41735）沿用原 runtime/Users、后端和排他锁启动，无高频探针和抓帧。

最终隔离版已完成 20 秒函数栈采样：解释器/调度/内存 **68.36%**、图形 **29.74%**；140.202733 秒独立计数窗口为 **14652 Draw / 1127 Present**，约 **8.04/s**。两者窗口不同，不能当作受控游戏 FPS AB。可明确归属 APFloat 的函数调用为 **1.15%**，含回退与快路径的包装入口为 **4.87%**，内联的软件代码可能仍在包装入口中。最新 `profile-summary.json`、原始栈、火焰图和 `readiness-observation.json` 均在 `interactive-executor-nscpv87h`，窗口保持运行，人工可玩性反馈仍待确认。
