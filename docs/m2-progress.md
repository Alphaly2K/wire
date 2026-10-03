# M2 进行中：最新进度与交接（2026-10-04）

M2 **尚未验收，不打 tag**。上一个 agent 的线程/SSE 工作已先验证并保存为本地提交
`75f90a6`。本轮补充了资源、堆、编码、文件 I/O、基础 SEH 和回归测试。
目标程序从原先 199,566 条指令推进到约 **1200 万条指令**，当前明确停在 x87 浮点加载。

## 已完成并验证

### 解释器与差分测试

- 原有 24 组 CPU 差分测试全部保留；现在共 **30 组**。
- 初始 XMM 状态随机化，终态比较全部 8 个 XMM 的 128 位；原有 GPR、EFLAGS、数据/栈内存比较继续执行。
- 新增 SSE 传送、整数/逻辑/unpack/shuffle、移位、浮点算术/比较/转换和标量边界访问测试。
- 修复 `UNPCKLPD/UNPCKHPD` 错用 32 位 lane、`MOVQ xmm,xmm` 未清高 64 位、
  多种标量 SSE 指令按 16 字节读取内存的问题。
- 新增 `SHLD/SHRD`（0f a4/a5/ac/ad，16/32 位、立即数/CL、寄存器/内存），
  并验证 count=0/1、按 5 位屏蔽计数和 ISA 定义范围内的标志。
- `examples/lockstep.rs` 现在也比较 XMM；`--xmm-hex` 可传入 8 个逗号分隔的 16 字节初值。
- Unicorn 2.1.5 对 `CVTPS2PD m64` 有 16 字节过度读取：普通转换仍做差分；
  精确 m64 边界访问用独立断言验证 wire，未把这个参照缺陷复制到解释器。

### 线程与同步

- 延续已有单宿主 CPU 的协作式调度，切换点为阻塞 API、显式 yield 和约 1M 指令的分时片。
- TLS/FLS 的值按 `(tid, slot)` 保存；LastError 按当前 TEB 的 +0x34 保存。
- 工作线程初始化 x87 控制字为 0x037f、MXCSR 为 0x1f80。
- 原 TEB arena `0x7ffc0000+` 的第 30/31 个槽会覆盖主 PEB/TEB；迁至 **0x7ff00000+**，
  并通过创建 40 个工作线程的回归检查。
- 分时片会处理等待超时，防止另一个持续 runnable 的自旋线程让超时永不唤醒。
- 修复 `InterlockedCompareExchange` 的 Exchange/Comparand 参数颠倒；
  `InitializeCriticalSectionEx` 的 SpinCount 使用第二参数。
- 新增真正执行 guest 代码的事件唤醒/线程退出测试，以及 wait-all 原子获取测试。

### 资源与 stdcall

- `LoadStringA/W` 使用 **16 字节**参数清理（旧交接写的 20 字节错误）。读取映射 PE 的真实 RT_STRING，
  支持 W 版本 capacity=0 的资源指针返回、截断和 NUL 终止，A 版本转换为 CP932。
- `FindResource[A/W/Ex]` 支持数字/名称键；`LoadResource/SizeofResource/LockResource` 可直接读取映射资源。
- 资源目录与字符串长度有边界检查。
- 补齐特殊目录、PathIsDirectory、WNetGetUniversalName、RtlGetVersion、GetClassInfoEx 等栈清理，
  并覆盖原静态导入清单内所有缺失项（视频/对话框等仍可保持功能未实现，但栈参数大小已知）。
- 单元测试检查目标程序 **346 个静态导入**都有清理记录；最新游戏日志的 unknown arity 警告为 0。
- `FormatMessageA/W` 支持少量系统错误文本、FROM_STRING 和 ALLOCATE_BUFFER；生成的块可由 LocalFree 真正释放。

### 真实堆

- 独立 guest arena **0x10000000..0x30000000**，16 字节对齐，分离 scratch/线程栈。
- HeapAlloc/Free/ReAlloc/Size/Validate/Create/Destroy 使用分配表和可合并空闲区；
  realloc 保留内容，支持原地增长、移动、HEAP_ZERO_MEMORY 和 IN_PLACE_ONLY 失败保留原块。
- 私有堆拥有独立句柄与分配归属检查；固定最大大小按分配容量检查。
- CRT malloc/calloc/realloc/free 以及固定型 Local/GlobalAlloc/Free 接入同一实现。
- DWORD 尺寸溢出或 arena 用尽返回失败，不再以 bump 分配冒充 realloc。

### 编码、路径与同步文件 I/O

- 系统 iconv 提供 CP932、Windows-1252、UTF-8 转换；实现 MultiByteToWideChar/WideCharToMultiByte，
  包括长度查询、显式长度、-1 的 NUL 计数、缓冲区不足及 CP932 非法输入的严格/替换处理。
- GetCPInfo(932) 返回 MaxCharSize=2 和正确 lead-byte 范围。
- SSE/SSE2 检测 6/10 返回 1；SSE3 仍不广告完整支持。
- `wire run --cwd DIR` 指定文件根；默认 EXE 所在目录。根映射为 guest `C:\`，
  guest 当前目录独立于宿主 cwd。shell 的特殊目录暂时统一映射到这个根。
- 支持 A/W 路径、大小写匹配和反斜线；拒绝 `..` 越界、其他盘符、UNC/device 路径及指向根外的符号链接，
  创建新文件也检查最近存在的父目录。
- CreateFile、ReadFile、WriteFile、GetFileType、GetFileSize/Ex、SetFilePointer/Ex、SetEndOfFile、
  FlushFileBuffers、CreateDirectory、Get/SetCurrentDirectory、GetFullPathName、GetFileAttributes、
  PathFileExists/PathIsDirectory、FindFirst/Next/Close、CloseHandle 已落地。
- 最新游戏日志显示 `.cf` 配置文件成功打开、读取 **927 字节**，随后正确读到 EOF。
- QPC 计数修正为与 QueryPerformanceFrequency 一致的 10 MHz。

### 基础软件 SEH

- SetUnhandledExceptionFilter 存储回调并返回旧回调。
- RaiseException 构建 x86 EXCEPTION_RECORD、CONTEXT、EXCEPTION_POINTERS，沿 FS:[0] 遍历处理器，
  用 guest 回调执行，支持 ContinueSearch 与 ContinueExecution 及处理器修改 CONTEXT 后恢复。
- 未处理时调用已注册 filter；不能恢复的异常会明确报出异常码，而不再直接返回并落入 int3。
- 检查 noncontinuable、循环链与不支持的 disposition。
- guest 回调的返回哨兵绑定到调用线程 TEB，避免其他线程误结束当前宿主回调。
- **未实现完整 MSVC EH / RtlUnwind、nested/collided unwind，也没有把 CPU 内存 fault 自动转为 SEH。**
  不应把基础 dispatch 测试通过视为完整 C++ 异常支持。

## 验证结果

- `cargo test --release`：**47/47**（1 个静态导入检查 + 30 个差分族 + 16 个 Win32 行为测试）。
- `cargo build --release`、`cargo fmt -- --check`、`cargo clippy --all-targets -- -D warnings` 全部通过。
- 对照 EXE：退出码 0；CreateFile/WriteFile 成功，宿主输出内容确认为 `wire control program\n`，21 字节。
  它的窗口 API 仍是 stub，所以这个退出码只验证加载/解释/文件路径，不代表图形验收。
- macOS 沙箱内 Unicorn 参照执行可能 SIGILL；本轮在沙箱外跑差分测试通过。
  Unicorn 仅为测试/调试依赖，wire 发布可执行文件仍为纯解释器。

## 最新游戏阻断

本轮最终运行：**12,004,086 条指令 / 24,264 次 API 调用 / 191 种 opcode**。
线程等待和计时会让确切计数略有变化，阻断地址稳定。

```text
unsupported instruction at 0x00691284: x87 mem op dd /0
bytes: dd 02 c3 ...        ; FLD qword [edx]
```

调用链从 0x50978d 进入 CRT 数学函数 0x6cdfc0，再到 0x6cfd84 / 0x691275。
当前只有 x87 控制/状态类，没有 ST(0)..ST(7) 浮点栈与算术。
前面的窗口类注册、GDI、timeGetTime、GlobalMemoryStatus、LCMapStringEx 等仍有未实现日志；
此次还没有读到 data.xp3 或进入可见游戏画面。

## 下一步（按序）

1. 补 x87 浮点栈及实际路径需要的加载、存储、算术与比较；把完整状态纳入 ThreadCtx，
   明确 80 位精度/舍入处理方式，先做差分再跑游戏。优先处理 FLD m64 和随后出现的 FSTP/CRT 数学路径。
2. 补初始化所需 GetSystemInfo、GlobalMemoryStatus/Ex、timeGetTime、NLS 分类/映射等实质语义。
3. user32 类/窗口注册与消息队列、guest WndProc 回调；避免把窗口创建失败误判为正常主循环。
4. 完整 MSVC EH / RtlUnwind；用独立 SEH/C++ 对照程序验证搜索与展开，不能只用 game 不再 throw 的路径验收。
5. 图形推进后检查 XP3 大文件读取、存档目录、插件 guest DLL 装载与导出解析。
6. 达到可见主循环后再验收 M2，开始 M3 的 GDI 帧缓冲/音频/输入接入。

## M3 已认可路线

保留上轮与用户确认的 **SDL3 + 薄平台层**：`src/platform/` 定义呈现帧缓冲、音频队列、
窗口事件接口；SDL3 为首个后端，Rust 用 sdl3-sys 薄封。GDI 软件 blit 上传 streaming texture，
waveOut 接 SDL_AudioStream。iOS 从设计开始考虑 main-callback 模式；需要时可替换为 UIKit/Metal。
图形对照截图在 M3 再收集，本轮未开始引入 SDL3。

## 环境与复现

- 游戏：`/Users/alphaly/Downloads/王様恋愛【体験版】/王様恋愛【体験版】.exe`，data.xp3 约 803 MB。
- commit/tag 使用 `-c commit.gpgsign=false` / `-c tag.gpgsign=false`。
- STUB_BASE 0x6fff0000；scratch 0xf00000；主栈 0x180000；主 TEB 0x7ffdf000；PEB 0x7ffde000；
  工作线程栈 0x2000000+；**工作线程 TEB 0x7ff00000+**；真实堆 0x10000000+；句柄 0x100 起。

```sh
cargo test --release
cargo clippy --all-targets -- -D warnings
cargo build --release
./target/release/wire run "/Users/alphaly/Downloads/王様恋愛【体験版】/王様恋愛【体験版】.exe" --max-instr 50000000 --trace-cpu
# 最新全量日志：wire-out/api.log；之前的分步运行保留在 wire-out/m2-*/。
./target/release/wire run tests/control/control.exe --cwd "$PWD/wire-out/control-cwd" --log-dir wire-out/control-m2
```
