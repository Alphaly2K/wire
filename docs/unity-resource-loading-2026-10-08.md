# Unity logo 后的资源加载接续（2026-10-08）

接续 `9a22054` 的性能检查点。主路径仍是 checked AMD64 IR/解释器，
没有宿主执行 guest x86、扩大 FLAGS 丢弃或绕过数据/代码权限。
性能 AB/BA 结果仍见 [上一轮性能记录](performance-unity-logo-2026-10-08.md)；
本轮带 API 统计和 GPU 捕获的功能重放不是性能对照。

## 已越过的实际边界

| 隔离重放 | 退休指令 | 完成 API | GPU Draw / Present | 结果 |
| --- | ---: | ---: | ---: | --- |
| pass153 | 4,349,950,607 | 7,121,892 | 412 / 149 | 已越过 UINT_MAX SRV 范围，停在 PEXTRW |
| pass154 | 4,625,721,264 | 7,546,958 | 412 / 147 | SIMD 已越过，停在 procedural Draw 的旧布局/空 VB |
| pass155 | 6,490,787,492 | 8,518,833 | 6,649 / 840 | 已越过 Draw，最后呈现为蓝色背景及加载沙漏；到达 6.5B work 预算 |
| pass156 | 7,990,004,130 | 9,287,016 | 11,741 / 1,407 | 目录打开已越过；Naninovel 暴露 NtQueryDirectoryFile 动态导出遗漏，到达 8B work 预算 |
| pass158 | 7,989,746,694 | 9,513,079 | 11,177 / 1,344 | 目录枚举已实际执行，进入保存阶段，CreateDirectory 的安全属性校验过严 |
| pass159 | 7,989,741,627 | 9,518,310 | 11,135 / 1,340 | 默认安全属性已越过，extended DOS 路径被误当 UNC 拒绝 |
| pass160 | 约 7.99B | — | 11,058 / 1,351 | 有界逐阶段诊断确认真实清屏、颜色和 extended DOS 路径失败 |
| pass161 | 4,732,474,159 | 7,801,865 | 411 / 146 | Saves 已实际创建，工作线程停在 GetSystemTimes |
| pass162 | 4,725,923,603 | 7,799,612 | 379 / 136 | GetSystemTimes 已越过，停在 SleepConditionVariableSRW；透明混合已生效 |

窗口仍由 guest 自己创建并显示，HWND=`0x7ffd00630000`，客户区 1024×720。
用户此前已观察到 Unity logo 动画；pass155 是新的后续画面，尚非标题或可玩验收。
`Player-pass-155.log` 首次明确进入 Naninovel 服务初始化，但
`LocalResourceProvider.DiscoverResources` 因 `CreateDirectoryHandle` 返回“不支持”产生异步 IOException。
因此增加预算或单看 GPU 计数不能解决此时的加载阻塞。

## D3D11 view 剩余范围

SRV 的 MipLevels、2D-array 的 ArraySize 与 cube-array 的 NumCubes 支持 `UINT_MAX`：
先验证 first 不越界，再物化为资源中所有剩余项。RTV/DSV 的数组 slice 同样处理。
空范围、溢出及有限 count 超过剩余范围返回 INVALID_ARG，不生成对象或取得引用。
规范化结果进入 owned descriptor，`GetDesc` 返回有限范围，原始 guest 输入保持不变。

native image view kind=6 明确表示 2D-array SRV，单层 array 也创建
`VK_IMAGE_VIEW_TYPE_2D_ARRAY`。typeless 深度 view 的兼容检查涵盖该维度。
这是 view 创建支持；native Draw 的采样 shape 验证仍限制普通 float32 2D，
没有解除 array/cube/volume Draw 限制或把不同 view 类型互相冒充。

原生 COM 回归覆盖四族 typeless 的单层 array、UINT_MAX mip/slice、GetDesc、
输入不变、NULL 输出 S_FALSE、无效范围及输出 fault 的引用/对象原子性；
还实际创建非零 first mip/slice 的多层 SRV/RTV/DSV 和 cube-array view，
验证 cube 数到 6 个 face 的换算及 native 范围。
只读核对 DXVK 的剩余范围处理，未复制其实现，也未引入 QEMU 源码。

## SIMD 插入/提取

补齐 legacy XMM `PEXTRB/W/D/Q`、`EXTRACTPS`、`PINSRB/W/D/Q` 和 `INSERTPS`。
控制字按 lane 数屏蔽；窄整数抽取按 decoded GPR 宽度零扩展，memory form 仅访问元素宽度。
INSERTPS 的 register selector、destination selector 和逐 lane 零掩码独立处理；
memory form 只读取 m32，即使零掩码清空全部 lanes 也必须进行 checked 读取。
FLAGS、MXCSR 和未修改 lanes 保持；MMX/x87 alias form 仍明确失败。

两个新增回归覆盖 26 种编码/operand 形式的全部 256 个控制字（6,656 个差分状态），
与 Unicorn 比较 XMM/GPR、内存、FLAGS/MXCSR；还覆盖 source=destination、
高 XMM/GPR、SSE2/SSE4.1 PEXTRW、REX.W、页尾窄读取、跨页 readonly/guard、
故障回滚和 MMX 拒绝。Unicorn 只用于测试，不进入游戏执行路径。

## 当前 shader 输入与旧布局

Unity 全屏 Draw 会解绑 VB/IB，而继续保留之前的 InputLayout。
Shader 编译时缓存经 native 验证的 SPIR-V 非 builtin input locations；
guest Draw 仅把当前 VS 实际需要的 layout attributes/slots 送往后端。
因此仅使用 SV_VertexID 的 VS 可以不绑定 VB；确实需要的输入仍进行全部验证。
诊断现在区分 VS/PS constant buffer 和具体 vertex slot。

新增原创 procedural DXBC fixture 生成真正的 SV_VertexID 全屏三角形。
原生 COM 测试逐像素比较 16×16 RGBA8 输出，覆盖空 VB、已映射但未使用 VB，
切换到需要 POSITION 的 VS 时拒绝已映射或空 VB，失败不增加 Draw 或改变像素；
最后逐个释放，检查对象/资源表为空。fixture 不包含游戏 shader。
GS 探针扫描也排除同目录的 VS/PS fixture。

## 同步目录句柄与批量枚举

CreateFile A/W 允许 FILE_FLAG_BACKUP_SEMANTICS 下 OPEN_EXISTING 的只读目录句柄，
保留真实 host descriptor、分享规则、继承/保护关闭与 DuplicateHandle 生命周期。
duplicate 共享枚举游标；目录不能作为 ReadFile/WriteFile 或 file mapping 数据流使用。
在 macOS/Linux 根据 live descriptor 找回当前目录路径，再验证 drive confinement 与 volume/inode，
重命名后不会错误地枚举旧路径上的替代目录。

新增 ntdll 专属 NtQueryDirectoryFile 的同步 FileFullDirectoryInformation(class=2)，
以及 GetFileInformationByHandleEx 的 FileFullDirectoryInfo/RestartInfo(14/15)。
记录包含 64 位时间、EOF/AllocationSize、属性、精确 UTF-16 filename 长度，
NextEntryOffset 按 8 字节对齐，NT IO_STATUS_BLOCK 使用 Win64 16 字节布局。
所有写输出先 preflight；写完记录与 IOSB 后才提交 snapshot/cursor。
越界 symlink 不进入结果，重启扫描重新取得 confined snapshot。
NT 调用保持 LastError，结束返回 STATUS_NO_MORE_FILES/Information=0。

当前仍是有界 snapshot：至多 65,536 条目、16MiB 输出，没有实时目录变更通知。
异步 event/APC、其它 NT info classes、reparse/EA/8.3 服务未实现；小到无法容纳完整首条记录的
buffer 保守返回 BUFFER_TOO_SMALL，尚未实现 Windows 的首条截断/BUFFER_OVERFLOW 协议。
不以空成功结果代替这些能力。基本 wildcard 沿用既有实现，不宣称覆盖所有 NT/DOS wildcard 规则。

## 保存目录、安全属性与系统计时

CreateDirectory A/W 现在只 checked-read SECURITY_ATTRIBUTES 的 lpSecurityDescriptor 字段。
NULL descriptor 使用真实 confined mkdir；显式非 NULL 安全描述符返回 ERROR_NOT_SUPPORTED。
不会因未使用的 nLength/bInheritHandle 拒绝默认安全属性，也不读取字段之外的尾部内存。
该语义只读核对用户指定 Hangover 的 Wine 子模块
`3ea3a14941b6a122b892c84141ce9662c008081e`，`dlls/kernelbase/file.c:668`；
源码是 LGPL-2.1-or-later，没有复制实现或加入 vendored 源码。
读取副本 SHA256 为 `813b4cc668ac744159f9138bd1aa32f4a4ed7bf4e9d0415f346367e2f68e6e3f`。

pass160 确认 Mono 实际传入 `\\\\?\\C:\\Users\\...\\Saves`。
GuestFs 只增加 extended DOS namespace 下绝对 C 盘路径解析；仍执行相同 confinement、
大小写解析和符号链接检查，拒绝 UNC、其它 drive、GLOBALROOT 和设备路径。
pass161 确认真正建立 Saves/Settings.json；当前文件仍是零字节，不能宣称已完成保存。

GetSystemTimes 使用真实 host CPU 累计计数转换成 100ns duration，kernel 包含 idle。
macOS 读取公开 Mach HOST_CPU_LOAD_INFO 并释放 host port；Linux 读取 /proc/stat。
三个输出允许 NULL，写入前一起检查；fault 不改变任何输出或 CPU。
这里报告 host 聚合 CPU 计时，不是 guest 单核的虚构计数；macOS 原始计数是 32 位，
长 uptime 的计数回绕尚未扩展。条件变量 SRW 等待仍是明确的未实现边界。

## 背景与 Logo 动画

用户此前要求 Logo 和加载页均为纯黑背景，并随后观察到 Logo 动画似乎消失。
2026-10-09 最新对照 CrossOver 后，用户确认加载页确实为蓝色，并明确要求背景不再排查；
背景颜色问题关闭，不强制把 guest 输出改黑。
pass160/161 的 Present 1/30/90 阶段帧仍有不同亮度/阶段的 Unity Logo；
进程在新的 API 边界退出会关闭窗口，不以单个末帧判断动画是否曾执行。

诊断确认 guest 首先 ClearRenderTargetView(0,0,0,0)，Logo 的背景 quad 随后写成 RGB(35,31,32)。
加载阶段 guest 实际清屏色为线性 (0.0307134,0.0742136,0.191202,0)，
最终大面积蓝色 RGB(48,77,120)。因此不直接把后端 Clear 或 Present 强制改黑。

发现并修复 CreateBlendState 对未启用的 RT1..7 与 disabled blend enum 的错误校验。
IndependentBlendEnable=FALSE 时只验证 RT0，BlendEnable=FALSE 时忽略 factors/operations；
仍检查启用字段与写掩码，GetDesc 保留输入描述。pass162 的 Logo Draw 从 blend=0 变为真实 blend=1，
消除错误退回默认不透明混合的原因。当前逐帧诊断已确认 Logo 淡入淡出，
加载动画与标题仍待 SRW 条件变量边界修复后验收。
临时日志和 shader dump 已从代码移除，证据只在 wire-out。

2026-10-09 冻结优化候选的独立逐帧诊断捕获 149 次画面更新：首帧在 wall 39.713 秒，
Logo 30/90 帧分别在 40.205/41.193 秒，存在实际淡入淡出，约持续 2 秒。
120–145 帧只是同一纯色过渡背景；149 帧在 97.607 秒出现蓝色加载页和沙漏，
之后工作线程进入 SleepConditionVariableSRW 未实现错误，没有继续提交第二个加载页帧。
因此 Logo 内容并未从渲染结果消失，沙漏首帧停住与当前明确的 API 阻塞一致；
尚未修复这个条件变量边界，也没有以关闭窗口/提前退出当动画成功。
证据在 `wire-out/perf-unity-resource-2026-10-09/animation-after/`：
`frames.json` 保存外部观察序号和时间，PPM/PNG 保存画面，report/Player.log 保存错误。
观察序号不是 guest 精确 Present 编号；capture/外部读取有额外开销，该运行不纳入性能 AB。

动态导出遗漏已补齐；回归实际检查 LoadLibrary/GetProcAddress 的 ntdll gateway，
并确认 kernel32 不能冒充该导出。NT FileFullDirectoryInformation 的 class 为 2，
与 Win32 FileFullDirectoryInfo 的 class 14 不同，class 4 返回 INVALID_INFO_CLASS。
目录回归覆盖大于 4GiB 的文件、日文名、子目录、外逃 symlink、sharing violation、
metadata-only access、duplicate 共用游标、rename 后 descriptor 身份、重启/耗尽、
NTSTATUS 与 LastError 区分、counted pattern、短 buffer，以及双输出 fault 不消费条目。

## 验证与复跑

本轮功能加内联/缓存布局/紧凑执行循环后的完整 release：336 passed、7 ignored。
新增 procedural Draw 和扩展后的资源范围两个 native COM/GPU ignored 测试已显式运行并通过。
日志：`/private/tmp/wire-srv-simd-directory-{all-tests,clippy}.txt`、
`/private/tmp/wire-srv-procedural-native-tests.txt`、`/private/tmp/wire-directory-tests.txt`。
原始游戏目录保持只读，功能重放仅使用 `wire-out/games64/alice/runtime/`。
每轮的 `linked-pass-N.txt`、`api-pass-N.tsv`、`Player-pass-N.log` 和
`guest-present-pass-N.{ppm,png}` 保存在 `wire-out/games64/alice/`，不提交游戏数据。
新增 mkdir/extended DOS、GetSystemTimes 和 blend state 回归覆盖真实副作用、忽略字段、
输出 GetDesc、故障原子性和逃逸拒绝。geometry 探针再次比较 34,013 words 相符；
Draw 探针 10 组 × 768 像素、6 组失败保持像素均通过。
最终完整回归与 Clippy 日志：`/private/tmp/wire-resource-final-{tests,clippy}.txt`。
新增 ADC/SBB 的四宽度 784 个进位边界差分状态全部通过；线程上下文回归同时覆盖
1-work 与 100,000-work slice，验证紧凑循环没有越过合作调度和暂停边界。
CPU 性能后续见 [性能接续](performance-unity-resource-2026-10-09.md)，不能把本表诊断计时当性能对照。

原生测试需先按 [图形设计的命令](d3d11-dxvk-moltenvk-design.md) 重建 backend 与原创 fixtures，
设置 `WIRE_NATIVE_GRAPHICS`、`WIRE_MOLTENVK`、`WIRE_GS_FIXTURES` 后执行：

```sh
cargo test --release --lib x64::runtime::dxgi::tests::native_ -- --ignored --test-threads=1
```

游戏退出时 MoltenVK 的 physical-device cleanup 仍报告数 MiB allocated；未归因，
本轮独立 native 测试的资源释放通过不能替代真实长跑的泄漏检查。
