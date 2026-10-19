# 不可用音频与字体入口

`win32/catalog/audio.rs` 声明四项 MCI 和两项 wave 入口，`font_resources.rs` 声明六项字体资源入口。它们分别只在 PE32 winmm/gdi32 使用 typed 路由，Win64 保持 absent。typed 表示明确的调用策略，实际不可用状态继续通过原返回值、错误和 unimplemented trace 表达。

MCI 四项保留错误 274，不读取字符串或命令指针、不写输出/LastError。waveOutGetNumDevs 返回零设备并保持原已处理计数；waveOutOpen 由窄适配器先清零非 NULL 输出，再返回 MMSYSERR_NODRIVER=6 和原 unimplemented 诊断。输出 fault 在 trace/返回提交之前失败；输出覆盖返回槽仍按 PE32 原公共返回次序读取被覆盖的地址。现有 DirectSound 后端及 timer-resolution 家族独立，未改变它们的行为或公开 system facade 范围。

字体安装/卸载入口保留零返回、LastError=120、原 note 和 unimplemented 计数。指针仍被忽略，输出不变；逻辑字体创建继续由原 GDI 家族处理，没有新增字体安装后端。

独立基线 `3bf4860` 覆盖十二项 provider、清栈、输出/LastError/诊断，以及 wave 访存故障和返回槽别名。源码对照保持原错误与提交语义；普通契约迁移不跑性能或游戏矩阵。

release 完整 Win32 255、Win64 modules 13、catalog 一致性 1 项及 fmt/release lib/bin strict Clippy/build 通过。累计 915 exports；原始日志及源码/构建哈希见 `wire-out/refactoring-2026-10-14/r2-unavailable-endpoints32-{source-checks,verification}.json`。
