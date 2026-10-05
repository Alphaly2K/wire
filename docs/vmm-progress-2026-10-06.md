# Guest VMM 与渲染前置推进（2026-10-06）

基于 `53f658a`，补齐当前游戏所需的私有虚拟内存管理，并准备图形渲染阶段。
dirlist.dll 的 reserve/commit 阻塞已解除，dirlist 和 extNagano 真实初始化。
当前游戏因 menu.dll 缺少 InsertMenuItemW 导出而走异常路径 ExitProcess(3)，没有可见画面或持续主循环，M2 未验收。

## 内存模型与 API

`src/mem.rs` 的 4GiB host reservation 保留，但 guest 页改为 **4KiB**；每页有独立 commitment/protection。
宿主页只管理 backing，始终为数据 RW/PROT_NONE，没有 host 可执行页。
同一个 macOS 16KiB 页内，四个 guest 页的 reserve/commit/noaccess/readonly 状态互不放宽。
新页提交及 decommit 后重提交只清零对应的 4KiB，不破坏邻页；最后一个 guest 页撤销后可以释放宿主 backing。
权限由 CPU 和 API 的 checked memory 路径统一验证，而不是依赖 macOS mprotect 能表达 4KiB 权限。

`src/mem/vm.rs` 维护 allocation 区间、初始保护、MEM_PRIVATE/MEM_IMAGE 和归属，
先保留 heap、scratch、worker stack、DLL、API gateway/native handles、TEB 等现有 runtime arena。
实际 image/stack 分配从 arena 中划出独立区间；guest 分配不能覆盖这些区间。
scratch 分配现在检查对齐/溢出/arena 上界，提交失败不会继续返回未经提交的地址。

| API | 已实现行为 |
| --- | --- |
| VirtualAlloc | 64KiB reserve 基址、4KiB size/commit 向外取整；NULL 自动找空洞；MEM_TOP_DOWN；reserve 与 commit 分离；recommit 保留既有内容和保护 |
| VirtualFree | MEM_DECOMMIT 可作用于部分页或完整分配；MEM_RELEASE 必须原 allocation base/size=0；释放/重用清理旧数据并失效代码 |
| VirtualProtect | 一个 allocation 内所有目标页须 committed；NOACCESS/RO/RW/EXECUTE/RX/RWX；输出指针先验证，失败不部分改保护；old protection 是第一页原值 |
| VirtualQuery | x86 28B MEMORY_BASIC_INFORMATION；FREE/RESERVE/COMMIT、AllocationBase/AllocationProtect/Type；region 在保护变化或 allocation 边界截断 |
| 上述 Ex 变体 | 接受当前进程 pseudo handle -1；其他进程句柄明确失败，不做远程进程访问 |

PAGE_GUARD 是一次性逻辑页故障，触发后清除标志并失效代码缓存；IR 绑定检查不能消耗 guard。
PAGE_NOCACHE/WRITECOMBINE 标志被记录，普通 CPU 内存仍由解释器执行，不模拟宿主 cache policy。
WRITE_WATCH、RESET/RESET_UNDO、large pages、physical allocations、placeholder、mapped section/COW 等扩展未实现，明确失败。
PAGE_WRITECOPY/EXECUTE_WRITECOPY 不作为私有分配支持。guest 私有分配上限遵循当前 2GiB 应用地址空间。
运行时内建分配/image 的释放仍由对应 owner 管理，不能用 guest VirtualFree 绕过 loader/heap 生命周期；
MEM_COMMIT 当前只接受本 API 建立的 guest 私有 reservation。

## CPU、IR 与 PE 装载

raw x86 的 instruction fetch 单独验证执行权限，数据读写继续按各自权限检查；EXECUTE-only 可以执行而不能数据读取。
IR 绑定使用不消耗 guard 的代码检查；保护改变、decommit、release、重映射和写入均更新既有代码页 generation。
已绑定的热 IR 遇到保护改变会回退到 checked raw fetch，不能继续执行 NX/NOACCESS 页面。
代码 generation 保持 host-page 粒度：4KiB 权限变化可以保守失效整个 16KiB 区域，不会扩大 guest 访问权限。
单步、预算、退休计数、FLAGS 保守证明/恢复、线程和 callback 边界保持原规则，未启用 unchecked memory。

EXE/DLL 在装载、重定位、IAT/TLS 修补结束后应用 PE section 权限，headers 为只读、padding 为 NOACCESS，
共享 guest 页按 section 权限合并；主/worker stack 为 RW。VirtualQuery 能区分 image 与 private memory。
loader 注册 exports 时的内部 snapshot 可检查 image padding/execute-only 内容，此接口只用于 loader，CPU/guest API 不使用它。
主栈仍按 reserve eager commit；guard stack 自动增长和 CPU fault 自动递送 SEH 仍未完成。

VMM 解除后，Borland CRT 的 SEH 初始化使用 0x67 地址前缀。补齐 MOV/LEA 的 16 位有效地址：
完整 ModRM BX/BP/SI/DI 组合、8/16 位 displacement、16 位回绕和 absolute moffs；
有效 offset 先取低 16 位再加 FS/GS base，数据宽度独立遵循 0x66。
未覆盖的其他 16 位寻址指令明确拒绝；这些指令保持 IR raw fallback，不扩大 FLAGS 删除证明。

## 真实运行与验证

游戏 CRT 的 `VirtualAlloc(NULL, 0x400000, MEM_RESERVE, PAGE_NOACCESS)` 返回真实 reservation `0x00910000`，
随后 `0x00910000..0x0091ffff` 的 16 个独立 4KiB commit 成功。
VirtualQuery 对主栈返回 28B 真实 metadata。
dirlist.dll 映射到 `0x50130000` 并完成 DllMain；extNagano.dll 映射到 `0x50150000` 并完成初始化。
下一次装载 `C:\plugin\menu.dll` 因 `user32!InsertMenuItemW` 无可用导出而失败，guest 正常进入自身异常/退出路径。
CLI 的 “exited cleanly, guest code 3” 仍表示调用退出 API，不表示游戏启动成功。

真实目标仍使用原始完整游戏目录，不修改/提交游戏插件和资产。
忽略的 `wire-out/game-vmm/` 保存 `with-ir`、`moffs16`、`mov16`、`final-ir`、`final-raw` 和 menu 静态导入诊断。
最终两条路径的 report/API trace 用于功能对照；计时轮询会改变少量指令/API 次数，不据此宣称性能改善。

| 最终重放 | retired guest 指令 | API 调用 | IR 命中 / raw fallback |
| --- | ---: | ---: | --- |
| 主 IR | 289,176,536 | 835,585 | 250,643,946 / 38,532,590 |
| raw x86 | 289,185,350 | 835,682 | — |

两条路径全部 GPR/EIP/EFLAGS 相同：EAX/EBX/EDX=0、ECX=3、ESP=0x0019c8b0、EBP=0x0019c8b8、
ESI=0xffffffff、EDI=1、EIP=0x6fff06d0、EFLAGS=0x0246，均调用 ExitProcess(3)。
这是越过 VMM 后同一菜单依赖边界的功能对照，不是持续渲染或性能收益验收。

```sh
cargo build --release
target/release/wire prepare '/Users/alphaly/Downloads/王様恋愛【体験版】/王様恋愛【体験版】.exe' --out wire-out/game-v3.wir
target/release/wire run '/Users/alphaly/Downloads/王様恋愛【体験版】/王様恋愛【体験版】.exe' \
  --ir wire-out/game-v3.wir --max-instr 1000000000 --log-dir wire-out/game-vmm/final-ir
```

完整 `cargo test --release` **102 项通过**：3 unit、35 differential、5 FPU、14 IR、1 Wasm、44 Win32。
新增 7 项回归覆盖 reserve/commit/query/recommit、4KiB decommit/zeroing/release 邻页保持、
冲突/坏参数/跨 allocation/输出原子性/Ex/top-down、NX/execute-only/IR 失效、RO 数据 fault/guard、
FS 16位 MOV，以及与 Unicorn 对照的 176 条 MOV/LEA 序列（全部寻址组合、数据宽度与回绕）。
既有 PE 主映射测试增加 image/header/stack 权限断言，DLL/TLS/卸载/CPU/x87/FLAGS 回归继续通过。
Clippy release all-targets、format/diff 检查通过。

VMM 是依据 Win32 合同独立编写的 wire 后端，本轮没有新增 Wine/QEMU 源码移植。
下一阶段的菜单、GDI/DIB、GDI+ 及 Metal 呈现实施顺序见 [图形渲染设计](graphics-rendering-design.md)。
