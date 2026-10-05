# 真实 DLL、静态 TLS 与脚本初始化推进（2026-10-06）

本页保留该阶段的结果。后续已移植 Wine 的部分 Win32 逻辑并越过 GDI+ 依赖缺失，
最新阻塞和实现范围见 [Wine 移植推进记录](wine-win32-progress-2026-10-06.md)。

按用户要求恢复游戏功能推进，暂停扩大性能优化。此前检查点 `6bdfa74` 的短启动测量只有约 24M
guest 指令，不能代表持续游戏负载。本轮解除 `Not Susie Plugin`、栈探测和 KAGParser 装载阻塞，
最终推进到约 **288M guest 指令 / 830K API 调用**的绘图插件依赖边界。
游戏仍未建立可见主窗口或呈现画面，未进入持续主循环，**M2 尚未验收**。

## 已执行的真实游戏路径

1. `kinglove.tpm` 映射到 **0x50000000**，HIGHLOW 重定位避开 heap。
   guest DllMain **0x500013a3** 实际执行；V2Link/V2Unlink 返回真实地址
   **0x500010b0 / 0x500010e0**。缺失的 GetPluginInfo 等导出返回 NULL。
2. CRT `__chkstk` 曾访问 **0x0017f000**，越过旧 128KiB 主栈。
   主栈改为按 PE reserve 全量提交，TEB StackLimit 使用真实下界，解除该 fault。
3. 实际打开 `data.xp3`，读取 header 和 **732,296B** 索引；之后继续按 guest seek/read
   读取脚本内容。约 62M 指令时发现 KAGParser 静态 TLS 尚未支持。
4. `KAGParser.dll` 的 TLS 模板是 **8B**，无 TLS callback；实现静态 TLS 后映射到
   **0x50010000**，DllMain **0x50028e09**、V2Link **0x50027fc0** 实际执行。
   随后持续读取 `data.xp3` 内多个脚本，以及 `patch.xp3` 的 header/索引。
5. 250M 预算曾停在脚本初始化中。扩大到 **1B** 预算后，游戏在约 **288M** 指令时尝试
   `C:\plugin\layerExDraw.dll`；其静态 imports 包含 **gdiplus.dll 的 140 个导出**。
   当前没有 GDI+ 兼容模块，也没有可加载的 guest 系统 DLL，LoadLibrary 正确失败，
   日志为 `DLL gdiplus.dll not found`，guest 随后通过异常路径 `ExitProcess(3)`。
   CLI 的 “exited cleanly” 仅表示 guest 调用了退出 API，不表示游戏启动成功。

曾怀疑文件写失败造成 KAGParser 无法提取。补齐 CreateFile 失败路径的文件名日志后确认，
失败文件是 **`C:\savedata\krkr.console.log`**，属于报错时写出的日志，与插件装载原因不同。
保留 GuestFs 的目录 confinement，没有放开宿主文件系统，也未替换或伪造游戏插件。

## 装载器实现与明确边界

`src/modules.rs` 是独立编写的 PE32 guest 装载器；没有导入 QEMU/Wine 代码或游戏资产。

- DLL 使用独立 **0x50000000..0x60000000** arena，64KiB 粒度。首选地址必须位于该 arena
  且未占用；否则要求有效 relocation。仅支持 ABSOLUTE/HIGHLOW，先校验全部 fixup 再修改。
- 校验 PE32 DLL、sections、directories、exports；image 和 section 零填充。
  imports 递归链接 guest 依赖，已知 native 模块接入现有 ABI/API dispatcher。
  native 导出只接受已注册或 ABI 表明确识别的符号；识别 ABI 不代表所有功能已经实现。
- 先 link 整个依赖图，再按依赖顺序执行真实 stdcall DllMain PROCESS_ATTACH。
  名称导出区分大小写，支持 ordinal、data export、forwarder，hole/缺失导出返回 NULL；
  forwarder 循环失败并回滚，重复依赖边不重复保有引用。
- 模块引用计数、重复装载、GetModuleHandle/Ex、from-address、GetModuleFileName 和 FreeLibrary
  接入真实模块表；主 EXE exports 也已注册。限定路径按完整路径查询，同 basename 查询选择较低基址模块。
  detach 后撤销映射并使代码页版本失效；reload 不复用旧 image 数据。
- link/init 失败回滚本次映射和引用。DllMain FALSE 会先收到 PROCESS_DETACH，
  已初始化的本次依赖再按反向顺序 detach；原有依赖保持装载。
  guest 回调非局部转移或退出时保留 CPU 新现场、保留本次代码映射并标为 Abandoned，
  禁止把它再作为成功模块获取/装载。
- 静态 TLS：64 个 loader 索引，与 TlsAlloc 的动态槽分开。写 AddressOfIndex，
  每个存活线程建立 FS:[0x2c] vector、模板独立副本和 zero-fill；新线程启动前也建立副本。
  卸载/回滚清空 vector 条目并释放块；线程退出释放其 vector/块。
  当前仅支持不含 callback 的 DLL TLS，最多 16MiB/模块、至多 16B 对齐。
  非空 TLS callback、delay imports 显式拒绝；主 EXE 静态 TLS 尚未接入。
- 依赖图允许循环初始化，但没有 SCC 卸载，循环依赖会保留引用。
  DLL_THREAD_ATTACH/DETACH、完整 loader lock/嵌套装卸行为、完整 Windows 搜索策略尚未实现。
  LoadLibraryEx 只支持 flags 0/8 的有限搜索子集；0x800 等 flags 当前拒绝。
- 主栈尊重 reserve 并 eager commit，最少原 128KiB、64KiB 对齐，top 保持 0x1a0000；
  过大或与 image 冲突的 reserve 拒绝。尚无 guard-page 动态增长，worker 栈仍固定 1MiB。
- 新增 SetHandleCount 的 NT 兼容返回行为、WaitForSingleObjectEx 的现有协作等待、
  x86 SList init/flush，以及 FindFirstFileEx A/W 的基础名称搜索。
  无 APC 系统，alertable wait 没有 APC 递送；其他搜索操作/不支持 flags 拒绝。
  WriteConsoleA 本轮仅补 ABI 识别，console 功能仍未实现。

DLL 代码先使用现有 x86 fallback；本轮未引入运行时机器码/JIT，未更改 FLAGS 静态删除策略或 x87 精度。
模块级安装 IR 留待后续，解释器仍是主路径，Wasm 仍为可选加速。

## 验证与复现

完整 `cargo test --release` **90 项通过**：3 unit、34 differential、5 FPU、11 IR、1 Wasm、36 Win32。
新增 9 项独立回归覆盖：真实 DLL 指令/导出/重定位/refcount、依赖/失败 detach、guest forwarder/循环、
同名不同目录、native stub from-address、缺失/损坏 PE、静态 TLS 的 DllMain/既有及新线程隔离/回滚、
主 EXE exports/PE stack reserve，以及 x86 SList。既有目录和等待回归扩展到 Ex ABI。
Clippy all-targets、format 和 diff 检查通过。

自编译 MinGW DLL 不依赖 CRT，使用真实 GetCurrentProcessId import、DllMain 与导出函数：

```sh
sh tests/dlls/build.sh
cargo run --release --example dll_probe -- wire-out/dll-probe/probe.dll
```

成功重定位到 0x50000000，ProbeValue=4279（PID4242+37），执行 26 条 guest 指令后 detach/unmap。
fixture 和 probe 源码在仓库，生成 DLL 位于忽略的 `wire-out/dll-probe/`。

真实游戏使用原始完整目录，`--ir wire-out/game-v3.wir --max-instr 1000000000`，关闭 CPU trace/opcode stats。
分阶段日志位于忽略的 `wire-out/game-dll/`：`first`、`handle-count`、`stack-reserve`、`file-diagnostic`、
`static-tls`、`kag-tls`、`kag-1b`、`draw-boundary`、`final-ir` 和 `final-raw`。
这些是本机诊断产物，不提交游戏代码或资产。

最终 IR/raw 两条路径分别退休 **288,093,525 / 288,102,631** 条指令，API 调用
**829,964 / 830,052** 次；均读取 **37 段、合计 1,130,713B**，停在同一 GDI+ 依赖缺失处，
全部 GPR/EIP/EFLAGS 终态一致。计时相关的 guest 轮询次数不同，不能视为严格同路径性能对照。

样本已从 24M 扩大到 288M，但仍是初始化/失败路径。当前 CLI MIPS 包括大量脚本、heap 和 API 日志工作，
与旧阶段不是同一负载；本轮不据此计算优化收益，也不声称持续游戏性能改善。

## 下一步

优先补真实 GDI/GDI+ 兼容模块和绘图插件初始化所需的对象、内存及返回状态。
`layerExDraw.dll` 静态绑定 140 个 GDI+ exports，不能给所有未知函数返回零：GDI+ 的零值表示 Status::Ok，
会在未产生有效对象时伪造成功。应明确绑定每个 ABI，对实现范围外的功能返回实际失败状态，
并以独立绘图 fixture 验证，再推进后续窗口/输入/呈现。
进入持续主循环后，扩大运行时间、重复次数与场景，再重新 profile 解释器。
