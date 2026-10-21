# 重构收尾审计（2026-10-20）

生产代码整合基线 `85ced07`，随后加强音频 fixture 并加入独立诊断探针。按原计划 R0–R5 与 R7 的交付及本轮验收已完成，R6 按用户要求排除。此前唯一剩余的真实音频验收已通过；保留的 legacy 和协调边界明确列于下表。

## 阶段核对

| 阶段 | 已交付 | 保留边界 |
| --- | --- | --- |
| R0/R1 | 当前文档入口、冻结输入/原创 V2 fixture、领域测试 harness、GUID 契约和两架构 adapter | 历史 binary/input hash 及失败轮次保留，不把旧游戏终点改写为当前结果 |
| R2 | 919 项 export 契约，DLL/别名/架构 ABI/执行策略在冷阶段绑定；Win64 普通 export 不再维护重复手工支持/arity/DWORD 表 | 原计划允许的 PE32 CRT、专用对象方法与 ABI 例外按下表保留 legacy |
| R3 | 文件、模块、环境、线程/同步、USER、COM/字符串、图形/音频等资源有 owner；普通 adapter 按实际资源借用 | 组合根仍负责跨资源控制；保留 PE32 KernelObjects32 混合句柄表和原数值空间 |
| R4 | Process32 拥有运行、调度、USER callback、DLL/TLS、SEH 与 CRT 编排，CPU 单步/IR 保持独立 | Cpu 的公开委托兼容；DLL、SEH、CRT 各自的恢复和部分提交策略保留 |
| R5 | owned host 窗口/帧、事件消费、GDI/像素边界，AudioQueue/CoreText/Vulkan/Metal 的 Rust owner 和 FFI 已归 host；真实 PCM 回调消费/关闭/重开通过 | 真实窗口/GPU/字体/音频依赖可访问对应设备的宿主条件，默认 ignored 的配置保持 |
| R7 | 已形成职责的目录归位，测试 harness、CLI 与 perf/inventory/profile/probe 工具分类，当前/历史文档分开 | 公开 facade 保留，第三方/native 构建和 include 输入路径保持 |

## 最后音频验收项已关闭

原音频 fixture 与主线程 C 探针的打开失败日志保留。本次复查使用同一探针 binary 对照：当前工具的受限执行中，NewOutput 与三次 buffer 分配成功，Start 返回 −66680（InvalidDevice）；沙箱外全部成功，提交/队列计数守恒。

加强后的 `host::audio::tests::native_pcm_output_accounts_samples_and_closes_once` 在沙箱外通过：回调在两秒上限内消费全部 2048 个静音样本，逐次统计保持 consumed+queued=accepted，关闭幂等、关闭后操作及重新打开的独立计数通过。生产 Rust owner、AudioQueue C 和 DirectSound 未改。原创诊断源码归 `tools/probes/audio.c`；原始/新日志与验证范围见 [音频复查](refactoring-audio-validation-2026-10-20.md)。

## 明确保留的 legacy 与协调代码

这些条目遵循原计划 R2 的“未迁移条目明确 legacy”和架构专用控制策略，继续提供原有行为。今后的能力扩展可逐家族迁移，不以全部改名或清空旧目录作为本轮完成条件。

| 范围 | 权威来源与保留理由 |
| --- | --- |
| PE32 CRT/cdecl | ABI 的 provider 策略归 `win32/abi/pe32.rs`，变参/返回适配和专用执行分开；不从宽松的零清栈规则推导实现能力 |
| DSound、WMI、D3D9、GDI+ 的对象方法 | 原专用 ABI/handler 保留；普通方法的状态借用已收窄，未替换 COM 槽位或扩展接口 |
| DXGI/D3D11 方法 | 冷绑定使用领域方法元数据，普通 handler 借 Graphics/内存及必要分配资源；跨 Runtime/Process/Presenter 的 present 协议保留 |
| 23 项 PE32 ABI override | 见下表；typed Wine spec、ordinal 范围和未知 ABI 策略保持，不补造未声明签名 |
| 根 API 的旧 facade/fallback、wire 伪入口与 CLR exit shim | 兼容旧公开调用及原未知行为；迁移家族已优先走绑定 ID，旧入口不构成第二份新契约 |
| Runtime64 的 close/duplicate/wait、APC、DLL/TLS、USER start/resume、异常、分配与 present 协调 | 涉及多个 owner、CPU/返回帧或失败后的部分提交；保留显式控制顺序，不强行塞入单一领域 owner |

23 项 override 由 `win32/abi/pe32.rs::override_stack_bytes` 维护：

| provider | 保留入口数 | 范围 |
| --- | ---: | --- |
| gdi32 | 3 | TransparentBlt、AlphaBlend、GradientFill 的旧 alias |
| oleaut32 | 1 | 直接 ordinal 拼写 #9 |
| ws2_32 / dbghelp / quartz / propsys | 4 | closesocket、MiniDumpWriteDump、AMGetErrorTextW、PropVariantToDouble |
| mfplat | 3 | MFShutdown、MFFrameRateToAverageTimePerFrame、MFStartup |
| mf | 6 | 旧 renderer/topology/session/source resolver ABI |
| ddraw | 3 | DirectDrawCreate、DirectDrawCreateEx、DirectDrawEnumerateA |
| dinput | 3 | DirectInputCreateA/W、DirectInput8Create |

## 收尾验证

本批 release 全目标 **695 passed / 0 failed / 8 ignored / 21 suites**，Python 工具 **31 项**通过，fmt、release all-targets/all-features strict Clippy 和 build 通过。8 项 ignored 为 7 项外部 Metal/native GPU fixture 加新增 PCM 输出 fixture；每项真实执行结果继续按原批次记录。

音频收尾仅改测试与探针。本次重新通过上述全目标、31 项工具测试、严格 Clippy、build 和 fmt，原生 PCM 1 项另在沙箱外显式复跑通过，不混进默认 695 项的计数。受限执行的全目标先在 Unicorn 差分套件收到 SIGILL，随后沙箱外完整通过；两份日志均保留。最终验证、源码和日志哈希见 `wire-out/refactoring-2026-10-14/r7-audio-closeout-verification.json`；[音频复查](refactoring-audio-validation-2026-10-20.md)记录命令及范围。

本批没有修改 CPU/IR、共享 SIMD、memory/cache 或原生算法；普通资源提取按用户调整后的粒度验收，不重跑长性能或游戏矩阵。最近的真实 native 图形验证为 `r7-host-graphics-layout-` 的两项 Metal 和两项 Vulkan fixture；原生窗口及 CoreText 证据见 [完整记录](refactoring-2026-10-14.md)。

对 host、两架构 CPU、PE32 IR 与共享 SIMD 做文本依赖检索，guest runtime 引用只落在已知 `x86/cpu/process_facade.rs` 兼容委托；host 内部可引用 MetalLayer 等 host 类型。这是有明确范围的源码检索，未作为完整 Rust 依赖图证明。明细见 `wire-out/refactoring-2026-10-14/r7-closeout-dependencies.json`。

当前 [状态页](../status.md) 记录交付与保留范围，领域规则在 `docs/architecture/`，历史过程在带日期报告。用户的设计稿、重构计划、IDE 与 ignore 改动未纳入提交；没有复制游戏、创建 worktree、清缓存或 push。
