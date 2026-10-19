# PE32 scratch 与 CRT 指针状态

`win32::x86::scratch::Scratch32` 私有拥有原 scratch 游标，范围仍为 0x00f00000..0x01000000。有效 power-of-two 对齐、溢出检查、size=0 至少占 1 byte、guest commit 和游标推进顺序保持。失败返回 0、设置 LastError=8，游标不变；成功保持 LastError。ApiState 的 scratch_alloc 与公开 SCRATCH_BASE 兼容入口保留，scratch_position 供只读查询。

`CrtState32` 私有拥有按 tid 保存的 errno 指针、懒 argv/envp 和 app type。errno 仅在成功初始化 guest 零值后登记，失败不写 registry；旧线程条目保留原寿命。argv 仍为 argc=1、argv=[cmdline_a,NULL]、envp=[NULL]，原宽字符 accessor 的现状保持。原来忽略的 argv 写入失败、两次分配及部分提交顺序均保持，未在结构迁移中改为原子事务。

ApiState 的 build_argv、errno 与 scratch 入口显式委托；crt_app_type 通过只读查询方法访问。环境空块和其它 scratch 使用者复用同一 owner，allocator 不重复持有状态。

普通 CRT 语义按依赖分离：`win32/x86/crt_memory.rs` 仅借 GuestMem，负责原 time、CP932 byte helper、memory、比较、C locale case 和 strlen；`heaps::crt_call` 仅借 GuestMem、GuestHeap 和 process heap ID，负责 malloc/calloc/realloc/free/strdup。CrtState32 的 startup_call 负责原 scratch 指针 accessor、mainargs 与既有零返回启动策略，strtoul 直接借本 owner 的 errno、Scratch32 和线程 ID。保留读取限额、原字节比较、NULL/overflow、宽字符 accessor 现状与分配后输出故障的提交顺序。

CRT CPU/FPU 返回、qsort guest callback、初始化函数数组、模块数据 export 和退出控制继续由原进程入口编排，数学、排序和 callback transfer 语义保持。cdecl 的 provider 判定、参数预读、零清栈及 trace 仍沿用原规则；本批没有把这些入口算作新增 catalog 契约。

旧行为基线 `53f620d` 独立固定懒分配、指针身份、对齐、缓存后耗尽与游标失败顺序。原 allocator、errno、argv 及 CRT dispatch 主体归一核对保持。环境耗尽测试通过真实分配填满 arena，使用只读游标查询，原错误/TEB/log/stack 期望值保持。release 完整 Win32 249、fmt/release lib/bin strict Clippy/build 通过，不跑性能或游戏矩阵。证据为 `wire-out/refactoring-2026-10-14/r3-crt32-{source-checks,verification}.json` 及 baseline/validation 日志。
