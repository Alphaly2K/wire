# 三个 PE32 样本：兼容修复与真实状态（2026-10-07）

当前按用户最新要求继续 Rewrite、totsulover，并恢复 x86-64 样本的基础支持。运行使用原目录的完整隔离副本，
没有修改原游戏目录、EXE 指令或保护判断。解释器为主路径，不引入机器码 JIT。
窗口创建、黑帧、退出 0、只有初始化计数均不算跑通。

## 样本状态

| 样本 | 实测状态 | 尚未通过的验收 |
| --- | --- | --- |
| 恋×シンアイ彼女 / koikake.exe | 已显示真实警告页、标题菜单；1,080 帧与 Return 输入重放通过 | 进入正文、持续可玩；标题 BGM 已经用户听验确认 |
| Rewrite.exe | 约 219.75M guest 指令后错误退出；0 Draw / 0 Present | Pixel Shader 2.0 执行与后续实际启动 |
| totsulover.exe | 指定 EXE 是 SdWrap / ソフト電池 包装器；约 1.61M 指令后报告外部运行时缺失 | 合法安装其要求的 COM server，再继续实际游戏路径 |

本轮增加 CPU MMX、SHUFPS/SHUFPD、CRT 数学与 qsort、静态 TLS 初始化、
版本资源、WMI 本地对象、Win32 varargs/CP932、窗口/线程消息生命周期、显示器查询，
以及实际 DirectSound PCM 混音、D3D9 顶点/索引缓冲、声明、变换、深度/模板与 Reset。
缺失的图形、音频与外部服务仍明确失败；没有仅靠修改能力位伪装支持。

## koikake：从启动错误推进到真实标题

CRT `_CIpow`、`pow`、`floor`、`ldexp`、`_ftol` 系列按 x87/cdecl 约定返回与弹栈，
保留 guest errno 和舍入模式。qsort 实际调用 guest comparator，处理 callback 预算与非局部退出。
MMX 与 x87 物理寄存器别名，成功指令重设 TOP/tags，EMMS 保留 payload；checked fault 不提前改变 FPU。
SHUFPS/SHUFPD 支持源/目标别名与 immediate，均纳入 raw/IR 与 Unicorn 开发测试差分。

`EnumDisplayMonitors` 执行真正 guest callback，使用逻辑 1024×768 单显示器与 clip；
`GetMonitorInfoA/W` 输出实际结构，解除游戏错误地将窗口缩到 1×1 的问题。
非空 HDC 的显示器枚举目前返回 ERROR_NOT_SUPPORTED。

D3D9 增加 POSITION/POSITIONT、D3DCOLOR、TEXCOORD 声明，XYZ/XYZRHW FVF，
world/view/projection、六个齐次裁剪平面、透视 UV/color 插值。
每次 SetRenderTarget(0) 都重设 viewport/scissor 到新 target 全尺寸，修复先前标题菜单的裁切。
Reset 实际替换隐式 back/depth surfaces；活跃 default-pool 外部引用/锁阻止 Reset，managed storage 保留。
资源引用、声明复制/容量、negative BaseVertexIndex、失败时像素不变均有行为回归。

DirectSound buffer/notify QI 使用稳定对象身份与引用计数，stop notification 唤醒实际 event。
PCM CPU mixer 通过公开 AudioQueue 输出；音频 callback 不执行 guest 指令。
标题 BGM 已完成下方 193 秒重放及用户听验；正文和其他场景的音画同步仍待验收。

ShowCursor 采用 per-thread signed counter；LoadCursor 当前支持系统 arrow，SetCursor/GetCursor 保存真实逻辑句柄。
宿主光标形状/隐藏尚未连接。跨线程 SendMessage 在窗口销毁或 owner ExitThread 时解除等待，
线程退出清理窗口、队列、timer 与 pending sends；有两线程 owner TEB 回调测试。

新键盘路径维护宿主 asynchronous state 与每线程取消息后更新的 queued state。
宿主按当前 layout 解析的文本延迟到 TranslateMessage 才排队 WM_CHAR/WM_SYSCHAR，
Unicode class 使用 UTF-16，ANSI class 使用 CP932；PostMessage 不伪造物理键状态。
posted keydown 只对 Backspace/Tab/Return/Escape 提供控制字符 fallback，不猜测 US layout。
完整 IME、dead keys、modifier/scan-code 映射与 keyboard-layout API 尚未实现。

此前 900 帧重放（修复 viewport 之前）达到 1,132,471,735 指令、6,323 Draw、900 Present、899 displayed，
截图确为标题菜单，但有裁切，不能作为正确布局验收。
viewport 修复后的 120 帧重放达到约 146.39M 指令、254 Draw、120 Present，
已查看 1024×768 最终截图：完整 CAUTION 页，16:9 内容居中，上下黑边正确。
最新修复布局与键盘后的重放达到 1,404,052,919 指令、8,459 Draw、1,080 Present、1,079 displayed。
Return keydown/keyup 均未导致崩溃；已查看最终截图，完整标题菜单与人物图像正常。
Return 之后仍处于标题菜单，因此不能声称已进入正文。

本地证据（均忽略、不分发游戏资源）：

- `wire-out/games32/koikake/probe-title-visible-report.txt`、`probe-title-visible/last-present.png`：旧标题实测。
- `wire-out/games32/koikake/probe-target-report.txt`、`probe-target/last-present.png`：修复后的警告页。
- `wire-out/games32/koikake/probe-input-final-report.txt`、`probe-input-final/last-present.png`：最新布局与输入重放。

可重复重放命令（真实 AppKit/AudioQueue 需要在允许平台服务的环境运行）：

```sh
cargo build --release --bin wire --example startup_probe
target/release/examples/startup_probe \
  wire-out/games32/koikake/runtime/koikake.exe \
  wire-out/games32/koikake/game.wir \
  wire-out/games32/koikake/probe-input-final \
  0 4000000000 input 900 visible enter
```

`input` 在正式窗口和 worker 出现后排队一次 WM_MOUSEMOVE；`enter` 在 900 Present 后
排队 Return keydown/text，运行 60 帧，再 keyup、运行 120 帧。输入经过实际消息路径，
没有改游戏分支。不要全程用每 100k 指令分段：合法长 callback 会因此被预算中断。
初次模式对话框的 OK button ID 为 10001；保存设置后的重放传 0。

## 标题 BGM：回填期限与 Metal GPU 路线

用户报告“开头正常，后面断续和重复”。真实日志确认 BGM 的 DirectSound 环为
352,800 bytes / PCM16 stereo / 44,100 Hz，即 2 秒；两个半段各 176,400 bytes / 1 秒。
播放游标通知 0 和 176,400 分别唤醒两个 event。后半程回填超过 1 秒后，
自动复位事件积压，正常 wait-any 优先选择最低索引，导致同一半段再次被回填，另一半段重复旧音频。
混音也读到了 guest 仍处于 Lock 的区域。没有改 wait 返回顺序、暂停音频时钟或修改游戏分支。

AudioQueue 增加 consumed/underrun/callback/queued samples，DirectSound 增加
`dropped_samples`、`max_pump_gap_ms`、`locked_playback_frames`。既有完整重放的
宿主 underrun/drop 都为 0，不能把“宿主队列正常”当作游戏流式回填正常。
诊断 probe 已显式关闭 opcode HashMap 统计，与普通 `wire run` 一致；早期统计打开的 probe 不再作为性能对照。

正常配置的失败重放：

| 路线 | BGM 回填次数 | 回填中位数 / P95 / 最大 | 超过 1 秒 | 连续回填同一半段 |
| --- | ---: | --- | ---: | ---: |
| guest DLL 安装期 IR | 64 | 1.260 / 1.396 / 1.481 秒 | 61 | 16 |
| 行扫描范围与整块复制优化 | 64 | 1.257 / 1.332 / 1.346 秒 | 61 | 15 |
| 像素函数内联与 identity 分支 | 148 | 1.128 / 1.182 / 1.247 秒 | 144 | 17 |

第三次已连续播放约 166 秒，仍未解决；不以启动时正常或帧数作为音频验收。
各次 clock-driven 指令数、Draw 数和调度路径不同，这些数据描述回填期限，不能构造严格同路径加速比。

软件绘制已把 render/stage/sampler 状态移到每 Draw 快照，减少逐像素 map 查询、重复插值、
临时字节数组和无覆盖扫描。后半程 25 秒 profile 共 10,712 个运行期样本，D3D9 仍占 53.50%，
x87 为 22.75%。按用户最新要求转入 GPU 绘制，软件优化仅作为回退。

macOS 普通运行默认启用公开 Metal compute 后端，静态 `src/platform/raster.metal` 负责固定管线
三角形覆盖、透视 UV/color、点/双线性纹理采样、alpha test/blend、颜色写 mask、D24S8 深度/模板。
现有 CPU 裁剪/资源/声明验证保留；输入先经完整 checked read 复制为宿主快照，Metal 只接触宿主 buffer。
每个 GPU command 完成且状态成功后才读回，统一 checked write 提交。失败不提交部分输出，继续软件回退。
缓冲区按容量复用，GPU pipeline 在 guest 启动前初始化，避免首次 shader 编译阻塞音频。
当前逐 Draw 同步/读回；还没有 GPU resident texture/target 和直接 drawable 呈现，后续可继续减少复制与等待。
这是静态宿主图形程序，不是 x86 JIT；FP80、IR、FLAGS、guest 指令预算与单线程 guest 调度均不变。

Metal 使用原生 FP32 插值，CPU 对照保留 F64；独立真实设备回归要求覆盖/未写颜色 mask/模板完全相同，
各颜色通道差异不超过 1 LSB、D24 深度不超过 4 units。fixture 覆盖重叠三角形顺序、scissor、
不同 RHW、变动 vertex color、wrap/clamp、点/线性采样和宽于 target 的 depth pitch。
软件行分区另要求所有颜色/深度/模板逐位相同；最多 4 个宿主 worker，只处理不重叠的宿主行区域。

加速后还暴露并修复 `Sleep(0)` / `SleepEx(0)` / `SwitchToThread` 的既有问题：
原来先切线程，再把旧调用的 EAX/ESP/return PC/LastError 写进新线程，造成警告页后提前退出。
现在先完成调用者的 stdcall 返回帧，再保存 context 并切线程。两线程和仅一个 runnable 线程的回归通过。

软件强制对照使用 `wire run ... --software-render`；probe 末尾增加 `software` 选择同一路线。
最终 6,000 Present 重放：4,569,703,863 guest 指令、46,788 Draw，全部由 Metal 执行，
GPU error=None；宿主实际显示 5,894 帧。已查看最后截图，标题菜单、人物和上下黑边正确。
BGM 连续观察 193.008 秒、193 次回填，中位 0.906508 秒、P95 0.964580 秒、最大 1.046667 秒。
仍有 4 次超过完整半段、6 次错过实际游标期限，但连续回填同一半段与 Lock 开始时处于活跃半段均为 0。
最后 30 秒中位 0.912630 秒、P95 0.960227 秒，1 次错过期限。
宿主 underrun/drop 为 0，max_pump_gap_ms=376，locked_playback_frames=5,081（约 0.115 秒）。
不能宣称完全零超时或所有场景音频均已通过。用户听验认为当前没有问题，按其指令转入其他样本。

证据：`probe-audio-metal-clear-report.txt`、`probe-audio-metal-clear/api.log`、
`probe-audio-metal-clear/last-present.png`、`probe-audio-metal-clear-stream.json`（均位于隔离 koikake 目录）。
`tools/dsound_streaming.py` 可从日志复算回填/期限/半段指标。

## Rewrite：真正的 Pixel Shader 执行是下一阻塞

`wire-out/games32/rewrite/pass-14-report.txt` 和 `pass-14/api.log` 记录 219,749,639 指令后退出 0，
仍为错误路径。深度/模板格式查询已通过，MessageBox 明确写出“ピクセルシェーダ 2.0”要求。
目前 shader caps 为 0，没有 shader bytecode 执行器。下一步需要 bounded token 解析、
寄存器/常量/采样语义、shader COM 生命周期和 raster 集成，再宣告相应能力。
不能只改 caps、返回空 shader 或把未执行的 draw 当作成功。

## totsulover：指定启动器的外部依赖

`wire-out/games32/totsulover/pass-6-report.txt` 和 `pass-6/api.log` 记录指定 EXE 查询
`HKCR\CLSID\{973558CA-B388-11D3-AEF6-0040264002C6}\LocalServer32`。
隔离游戏目录没有该 COM server，注册表返回 ERROR_FILE_NOT_FOUND，随后 MessageBox 要求安装“ソフト電池”。
这属于外部运行时依赖；没有换成另一个 cn.exe、制造安装状态或绕过保护。

## 回归、来源与边界

最新完整回归通过 **178 项**（`cargo test --release -- --include-ignored`）：
7 unit（含真实 Metal 设备回归）、37 CPU 差分、9 FPU、26 IR、1 Wasm、98 Win32。
真实 Metal 设备测试默认 ignored，必须在有设备的平台环境显式运行。
三项键盘行为回归覆盖：async 与 queued state、PM_NOREMOVE、keyup/低位清除、CapsLock repeat，
TranslateMessage 的文本延迟、Unicode/ANSI 编码、posted keydown 不修改物理状态和 checked 输出。
Clippy release all-targets `-D warnings` 通过。最新完整回归日志 `/private/tmp/wire-metal-all-final-tests.txt`；
Clippy 日志 `/private/tmp/wire-metal-final-clippy.txt`，format 与 diff whitespace 检查通过。

此前已渲染游戏“王様恋愛【体験版】”回归达到 3 Present、4 Draw、约 1.758B 指令；
已查看第二张 frame，警告页文字与人物图像实际存在。
这是兼容回归，不是新的性能 AB 对照，也没有用它证明三个新样本全部跑通。

Hangover 固定 `56e385b7ff490edd281b288f420677093563560b`，Wine 子模块固定
`3ea3a14941b6a122b892c84141ce9662c008081e`，使用逐文件核查的 LGPL-2.1-or-later 源码/公开 ABI，
选择 LGPL 2.1 并保留版权、快照与校验值。来源范围见 `third_party/hangover/README.md`。
没有复制或链接 QEMU GPL 实现；Unicorn 仅用于开发测试 oracle。
保留 checked guest memory、FP80、预算/线程边界与极度保守 FLAGS；Wasm 仍只是可用时的加速路线。
