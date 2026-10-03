# M2 进行中：进度快照与下一步（2026-10-04）

> 本文档是限额保护性快照。M2 未完成，不打 tag。代码已在磁盘上（工作树），**尚未 commit**。

## 已完成（本阶段）

### M2a：协作式 guest 线程 + 同步对象（已验证）

- `src/thread.rs`（新）：协作式调度器 `Sched`。单宿主 CPU 跑全部 guest 线程；切换点 = 阻塞 API / Sleep(0) / SwitchToThread / 解释器每 1M 指令的分时片（防自旋饿死）。
- `Cpu::save_ctx/load_ctx` 按线程换入换出（regs/eip/eflags/fs/gs/fpu/xmm/mxcsr）；`ThreadCtx` derive Default。
- 内核对象句柄表 `ApiState.handles: HashMap<u32, HandleObj>`（Event/Mutex/Semaphore/Thread），命名对象去重（ERROR_ALREADY_EXISTS）。主线程也有真实句柄（main.rs 注册，tid=1）。
- 已实现 API：CreateThread（guest 栈 1MiB @0x2000000+、TEB @0x7ffc0000+、返址指向新 stub `wire::ExitThread`）、ExitThread、GetExitCodeThread、Suspend/ResumeThread、Sleep/SleepEx、SwitchToThread、CreateEventA/W、OpenEventA/W、Set/Reset/PulseEvent、CreateMutexA/W、ReleaseMutex、CreateSemaphoreA/W、ReleaseSemaphore、WaitForSingleObject、WaitForMultipleObjects（any/all）、CloseHandle、GetCurrentThreadId（真 tid）、Set/GetThreadPriority（假）。
- 临界区从 no-op 升级为真实 owner/递归计数（写 guest CRITICAL_SECTION 字段；争用走 WaitTarget::CriticalSection 协作阻塞）。LockCount 只分 -1/0，不维护等待者数（简化假设）。
- 等待语义：信号唤醒统一 `Wake::Retest`（重测获取，all-or-nothing for wait_all），超时唤醒 `Wake::Completed(WAIT_TIMEOUT)`；`wait_deadline` 跨 Retest 重阻塞保留原始预算。无可运行线程时宿主睡到最近 deadline；全 INFINITE 死锁 → 报错并列出线程状态（`describe_deadlock`）；全部退出 → 停机。
- 阻塞式 API 的实现技巧：不推进 eip/esp 直接切走，被唤醒后重入同一 dispatch（参数从栈重读），日志只记完成那次，阻塞时用 `log.note` 不计数。
- **回归**：`cargo build` 零警告；difftest 24/24 绿；对照程序退出码 0。

### M2b：SSE/SSE2 指令子集（刚写完，仅初步验证）

游戏越过 CreateThread 后在 0x6f85b6 撞上 `0f 28`（movaps，CRT 的 SSE memcpy）。已实现：

- Cpu/ThreadCtx 增加 `xmm: [[u8;16];8]` + `mxcsr`。
- 传送：movups/upd/ss/sd、movaps/apd、movdqa/movdqu/lddqu、movlps/hps（含 movhlps/movlhps/movddup）、movd/movq、movnt*（按普通 store）。
- 逻辑：and/andn/or/xor ps/pd（按位）、pand/pandn/por/pxor。
- 算术：add/sub/mul/div/min/max ps/pd/ss/sd、sqrt*、rcp/rsqrt（用精确 1/x 代替硬件近似，已登记偏差）。
- 比较/掩码：comiss/ucomiss/comisd（ZF/PF/CF，清 OF/SF/AF）、movmskps/pd、pmovmskb、pcmpeqb/w/d。
- 移位：pshufd/hw/lw、psrl/psll/psra w/d/q、psrlldq/pslldq（0f 71/72/73 组，66 前缀）。
- unpack：punpckl/h bw/wd/dq/qdq、unpckl/h ps/pd（别名到 punpck 实现）。
- 转换：cvtsi2ss/sd、cvtt/cvt ss/sd 2si（截断/最近偶，无效→0x80000000）、cvtps2pd/pd2ps/ss2sd/sd2ss、cvtdq2ps/ps2dq/tps2dq、cvttpd2dq/pd2dq/dq2pd（0f e6）。
- pinsrw/pextrw、ldmxcsr/stmxcsr、fence 族 nop；0f 18-1f hint nop。
- 简化假设（已登记）：movaps 不查 16B 对齐 fault；MXCSR 舍入模式不建模（恒定最近偶）；MMX（mm0-7）未实现，遇 `0f 6f/7f` 无前缀报 unsupported。
- **SSE 尚未纳入差分测试**（difftest 模板目前只生成 GPR 序列）——这是正确性债，下阶段第一件事。

## 当前阻断点（游戏实测）

51186→199566 指令、470→3995 次 API 调用。当前停在：

```
user32.dll!LoadStringW(0x400000) = 0 [UNIMPLEMENTED]（无 arity 记录，按 cdecl 清的栈——可能把栈清错了！）
kernel32.dll!RaiseException(0xe06d7363, 1, 3, 0x19f6cc) = 0 [UNIMPLEMENTED]
→ int3 breakpoint at 0x00462d03
```

分析：0xE06D7363 是 MSVC C++ 异常。游戏可能因 LoadStringW 失败（返回 0）走错误路径抛异常；RaiseException 未实现直接返回，执行流落回异常处理代码里的 int3。注意 LoadStringW 无 arity 记录导致栈清理可能错误（它是 16 字节参数：hmod, id, buf, max）——**先补 arity 再查**。

## 下一步（按序）

1. **commit 现有工作**（用户喊停时还没提交）：`git -c commit.gpgsign=false commit`，信息含 M2a+M2b。不必打 tag（M2 未验收）。
2. 修 LoadStringW：arity 表加 `("LoadStringA",20),("LoadStringW",20)`；实现返回空串+ret 0 还是真资源串先跑日志看后续。
3. 实现最小 RaiseException/SEH（M2 计划项）：0xE06D7363 走 TEB SEH 链（FS:[0]）或 SetUnhandledExceptionFilter 注册的过滤函数；MSVC EH 的 safe-handler 链解析可做最小版。先只记录+调用 UnhandledExceptionFilter 观察。
4. GetCPInfo(932)（MaxCharSize=2+lead bytes）、IsProcessorFeaturePresent（SSE 已实现→对 PF_XMMI(6)/PF_XMMI64(10)/PF_SSE3(13) 返 1）。
5. **SSE 差分测试补债**：difftest 模板扩展 XMM 状态比对（lockstep 工具目前只比 GPR/flags）。
6. 真实堆（HeapAlloc/Free/ReAlloc 替换 bump heap，free 语义要生效）——引擎进入主循环后必然反复分配释放，bump heap 会爆。
7. 文件 I/O + 沙箱（CreateFileW/ReadFile/SetFilePointer/GetFileSize，宿主映射到游戏目录；`wire run` 加 `--cwd`，默认 exe 所在目录；注意 CP932↔UTF-8 路径、`.cf`/`savedata/`/`plugin/*.dll`）。
8. 之后每实现一批重跑：`./target/debug/wire run "<游戏路径>" --max-instr 50000000`（日志在 wire-out/api.log）。

## M3 图形路线（用户提议 SDL3，已认可）

采用 **SDL3**，但藏在薄平台层后面：`src/platform/` 定义 trait（present 帧缓冲 / 音频队列 / 窗口事件），SDL3 是第一个后端，iOS 上若集成不顺可换 Metal/UIKit 后端而不动 GDI/user32 代码。理由：游戏图形导入只有 GDI32 → 我们维护软件帧缓冲做 BitBlt，SDL3 streaming texture 上传即可，无需 GPU 管线；音频 waveOut→SDL_AudioStream 直通；zlib 许可与 LGPL-2.1 兼容。注意点：SDL3 的 Rust 绑定用 `sdl3-sys` 自己薄封（不依赖高层 crate）；iOS 需 main-callback 模式，平台层从第一天按此设计；参考截图（Windows 原版）M3 再向用户要。

## 关键环境备忘

- 游戏路径：`/Users/alphaly/Downloads/王様恋愛【体験版】/王様恋愛【体験版】.exe`；工作目录须为游戏目录（data.xp3 803MB）。
- commit/tag 需 `-c commit.gpgsign=false` / `-c tag.gpgsign=false`。
- 常数：STUB_BASE 0x6fff0000；SCRATCH 0xf00000；STACK 0x180000；TEB 0x7ffdf000；PEB 0x7ffde000；线程栈 0x2000000+；线程 TEB 0x7ffc0000+；句柄从 0x100 起。
- 入口 0x67d3ba；崩溃时的 `wire-out/api.log` 尾部 + `--trace-cpu` 是标准排查组合。
