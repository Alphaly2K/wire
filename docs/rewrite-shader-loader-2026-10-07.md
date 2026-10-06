# Rewrite：shader、字体与资源 DLL 检查点（2026-10-07）

继续使用用户给出的 `Rewrite.exe`，运行目录为 `wire-out/games32/rewrite/runtime/`。
原游戏目录、EXE、资源和保护判断均未修改。黑帧、shader 创建、纹理上传不代表游戏已跑通。
下文的 API/图像证据都只保留在忽略的 `wire-out`，不分发游戏资源或 shader bytecode。

## PS2 / VS2 的实际执行范围

新增 `src/d3d9/shader.rs`，独立解析公开 D3D9 token，再由 CPU 或静态 Metal 程序执行。
每个程序最多 8192 DWORD、96 条可执行指令，12 个 temp、32 个 float4 常量、v0、t0/t1、
2D s0/s1 和 oC0。支持 MOV、ADD/SUB/MAD/MUL、RCP/RSQ、DP3/DP4、MIN/MAX、SLT/SGE、
EXP/LOG/LRP/FRC/POW/ABS/NRM/SINCOS/CMP/DP2ADD、DCL/DEF、TEXLD（含 project）和 TEXKILL。
源 swizzle、neg/abs、destination mask、saturate 和 DEF 遮蔽 device 常量均实际执行。
partial precision 仍使用 F32；不支持 relative/predicate/flow control、更多 sampler/input、
depth output、cube/volume 或 MRT，这些程序创建时明确失败。

真实 Rewrite inventory 共 202 次 PS 创建、58 个不同程序；双 sampler 和双 UV 补齐后全部被支持。
VS inventory 共 192 次创建、2 个不同程序，均只使用 DCL、MOV、DP4。
新增 `src/d3d9/vertex_shader.rs` 在每个顶点上执行这些程序，支持 16 input、12 temp、
256 个 float4 常量以及 POSITION0、COLOR0、TEXCOORD0/1 语义；输出 oPos、oD0、oT0/1 后
执行齐次裁剪、viewport 和透视插值。其他 VS2 指令/relative/flow 明确拒绝。
这属于经过验证的 SM2 子集，不能据此宣称完整 Shader Model 2.0 或任意 D3D9 游戏兼容。

Create/Set/Get shader、浮点常量、QI/AddRef/Release/GetDevice/GetFunction、绑定 pin 和 Reset
均有真实对象和生命周期。FLOAT4 position 声明、双 UV FVF/declaration、两套 sampler state 已接通。
CPU shader 保留 float RGBA 到最终输出，纹理采样后不会提前量化为 8 位。

Metal 不生成 guest x86 机器码，也不按 guest shader 生成 MSL。
静态 `raster.metal` 解释宿主已验证的定长 PS2 records。既有 200-DWORD 参数不变；
buffer(5) 使用 count、8-DWORD texture1 元数据、128 个常量 DWORD 和每条 8-DWORD record。
第二张纹理与第一张组成 checked 宿主快照，payload 带 offset/size/address/filter；bridge 再检查长度和范围。
只在 GPU command 成功后提交 checked 输出；错误仍走软件回退。
fixed-function 真实设备 fixture 保持颜色 <=1 LSB、D24 <=4 units；新增 PS2 双纹理 fixture
覆盖/颜色 mask/模板完全相同、颜色 <=1 LSB，因 FP32 GPU 与 F64 CPU 对照允许 D24 <=8 units。
软件行分区仍要求逐位一致。

## GDI、Reset 与 viewport

`GetTextMetricsW` 通过公开 CoreText 实现所选逻辑字体的实际度量，使用字体表和
ascent/descent/leading 等值，缓存到 font 对象，checked 写出 60-byte TEXTMETRICW。
缺少 Windows 字体时使用系统字体 fallback，不分发字体。非 macOS 明确不支持。
这没有完成 glyph/outline/text extent；`SetMapMode/GetMapMode` 当前仅支持 MM_TEXT。

CreateDIBSection 支持游戏命中的 32 位 BI_BITFIELDS，RGB masks 必须为
0xff0000、0xff00、0xff；仍以 checked BGRA32 存储，GetObject 正确返回 compression 和 masks。

按固定 Hangover Wine `dlls/d3d9/device.c` 核查 Reset 和 SetViewport：
成功 Reset 返回实际 back-buffer width/height/format/count，修复旧实现清零这些参数的问题。
SetViewport 接受空 viewport 和有界的较大 viewport，最终 raster 与 target 裁剪；空 viewport 写零像素。
真实 Reset 后的 viewport 已从错误的 0×0 恢复到 960×540。

## 从错误资源读取定位到扩展 DLL

`pass-25` / `pass-26` 在约 1.9275B 指令、0 Draw / 1 黑帧 Present 后
读取 0x5f96f50c 失败；故障发生在 `mn_mw_mw00a00.g00` 读取之后。
离线解压函数差分的 raw 和 IR 各执行 3,886,117 条完整指令，所有 GPR/PC 和
1,433,912 输出字节均与开发测试依赖 Unicorn 相同；这不包含完整进程或所有 FLAGS 的比较。
该文件使用 MARK 编码，但资源扩展没有装载，旧解压函数将其当作普通 LZ 流。
随附汉化说明指定 SiglusEngineCHS.exe；另外验证它也在相同资源位置失败，记录在 `chs-pass-1`，
没有以它代替原 EXE 的验收。

真正的先行故障是两种启动器都无法 LoadLibrary `SiglusDebugger2.dll`。
其 MinGW PE 的 SizeOfImage=0xb22c，最后 `.reloc` 虚拟大小 0x22c、文件大小 0x400。
现在按页映射合法末页文件 padding，同时拒绝真实 virtual extent 越界、跨下一页的 raw 越界、
重叠和截断文件；没有接受任意坏 PE。

进一步补齐 bounded TLS callback table（最多 256 项、终止符和 executable image target 验证），
HIGHLOW 重定位后的函数在真实 owner TEB 上通过普通 CPU 调用。
PROCESS_ATTACH 的 TLS 在 DllMain 之前；新线程在实际运行前收到 THREAD_ATTACH，
有序 THREAD_DETACH 在 TLS storage 释放之前；卸载/失败回滚/正常进程退出有对应通知。
依赖先初始化，detach 按初始化顺序逆序；TLS callback 为 void，返回 EAX 不决定初始化成功。
DisableThreadLibraryCalls 对有 static TLS 的模块真实失败。
已有非局部 callback 控制转移和预算保持；完整 Windows loader lock、activation context、
所有异常处理及动态 TLS 表修改语义仍未实现，不能视为完整 Windows loader。

`pass-28` 已成功装载 SiglusDebugger2.dll 和 ucidec.dll。
扩展会把 x86 trampoline 写到 malloc 内存，两种启动器均没有 NX_COMPAT。
现在使用明确的 Windows PE32 opt-in DEP profile：NX_COMPAT 保持 NX，旧程序可从 readable guest data
解释取指；NOACCESS、GUARD、跨页权限及映射检查继续生效。
DEP policy 改变会失效 watched IR permissions。宿主 backing 永远是 RW 数据，
不分配宿主可执行页，不将这些 trampoline JIT 成宿主代码，也不修改 guest 生成的代码。

CRT `_winmajor/_winminor/_winver/_osver` 为共享可写 guest 数据，不再是 callable stub；
值与当前 Windows 6.1 / build 7601 profile 一致。memcmp 比较 unsigned byte，先检查完整范围；
toupper/tolower 支持当前默认 C locale 的 ASCII 与 EOF 保留。其他 locale 尚未实现。
解码 DLL 的 `.wir` sidecar 仅生成在隔离目录，加载仍校验 source hash、已重定位/修改的指令字节和页 generation。
`pass-29` 已越过旧资源崩溃并开始创建/填充纹理，但命中了未实现 CRT 调用，停止后改用修复后的 `pass-30` 验证。

## 许可与证据

`pass-31` 已执行 7,753,547,625 guest 指令、36 Metal Draw / 37 Present / 37 displayed，
随后游戏自己显示绘制错误并退出：effect Begin 调用未实现的 BeginStateBlock。
这不是标题菜单验收；退出时窗口销毁，没有保留最终窗口截图。

新增 `src/d3d9/stateblock.rs`，支持 Begin/End 录制、当前支持状态范围的 D3DSBT_ALL、
StateBlock9 GetDevice/QI/refcount/Capture/Apply。录制期 validated setter 使用私有 device snapshot，
getter 和绘制继续读 live state；重复 Begin、无 Begin 的 End、录制期 Create/Reset 真实失败。
只在 setter 成功后记录成员，Apply/Capture 只覆盖这些成员，render target / depth surface 不入块。
viewport、scissor、render/stage/sampler、transform、FVF/declaration、两张纹理、shader、浮点常量、
stream-zero 与 indices 均按现有 backend 语义记录；COM resources 保持独立、平衡的 pins。
Capture 保存 resolved defaults，不会因后来绑定 depth surface 改写已捕获的 ZENABLE。
typed PIXELSTATE/VERTEXSTATE、未支持的状态与 shader I/B 常量仍明确失败，不能声称完整 D3D9 stateblocks。
2D sampler 的 ADDRESSW clamp 现在真实存储；它不影响支持的 2D texture 取样。
两项行为回归覆盖录制隔离、选择性恢复、Capture、非法输出、资源存活/释放、ALL 默认值/常量和 target 排除。
`pass-32` 已越过 BeginStateBlock，随后 DrawIndexedPrimitive 返回 INVALIDCALL。
定位为 XYZRHW 预变换输入错误地被拒绝与仍绑定的 vertex shader 同时使用：
现在 POSITIONT/XYZRHW 直接使用屏幕坐标并跳过 VS；shader 绑定和引用仍可查询。
实际像素回归覆盖从 VS2 输入切换至 XYZRHW，保持已绑定的 VS 和 PS，验证颜色与覆盖。
`pass-33` 已越过该检查，进一步命中 effect 的默认 sampler 状态和空纹理绘制。
单层纹理 MIPFILTER NONE/POINT/LINEAR 均选择唯一一级，ELEMENTINDEX/DMAPOFFSET 仅接受默认0；
非默认值、displacement mapping、真正 mip chain 仍未支持。
固定管线缺少 stage-zero 纹理时，使用纹理的默认 stage 被禁用并保留传入 diffuse RGBA，
不会伪造纹理内容；PS2 明确使用未绑定 sampler 时仍失败。
新增行为断言检查默认 sampler roundtrip、未支持值拒绝、shader binding 保留和空纹理实际像素。
Draw 失败时日志包含有界状态与前4个顶点，正常热路径不添加这些读操作。
`pass-34` 使用原 Rewrite.exe 继续可见重放，先达到22B指令预算：
3,101 Metal Draw / 522 Present / 521 displayed、GPU error=None，未崩溃或错误退出。
实际查看第53帧，版权页文字完整；最终960×540截图显示森林/人物/Logo和四项标题菜单。
请求900帧并未全部完成；正文、输入、场景切换与持续可玩仍待验收。
标题阶段25秒 profile 中 IR/cache 41.09%、x87 30.13%、其他 raw fallback 21.27%，
统计范围/单次短采样限制见 games32-progress；不以此宣称严格加速比。

公开 shader token 和 ABI 参考固定 Wine 的 LGPL-2.1-or-later d3d9types.h/d3d9caps.h/d3d9.h，
保留未修改头文件与校验值，wire 选择 LGPL 2.1。PE mapping、TLS 和 Reset/viewport 只参考
逐文件核查许可后的固定 Wine 语义；没有复制或链接其 renderer/compiler、QEMU 或 Unicorn 运行时。
版权、固定提交与范围见 `third_party/hangover/README.md`。

实测：`pass-25/26/27/28/29/30/31/32/33/34-report.txt` 与同名目录 api.log；
离线对照：`decode-raw-unicorn.txt`、`decode-ir-unicorn.txt`。
诊断 shader dump 默认关闭，只在 startup_probe 写入限定目录，不提交这些 `.psbin`。
