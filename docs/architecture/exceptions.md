# 异常入口、注册状态与展开元数据

十一项 export 在 `win32/catalog/exceptions.rs` 唯一声明 provider、ABI、返回约定及 ExceptionApi。PE32 的 RaiseException、Set/UnhandledExceptionFilter 和 RtlUnwind 保持 typed；Add/RemoveVectoredExceptionHandler 保持 kernel32 ABI-only；五项 AMD64 function-table/unwind 入口保持 PE32 absent。RtlUnwind 的 PE32 provider 仍是 kernel32/ntdll，其余已知 PE32 项仅 kernel32。

Win64 保留九项 typed，kernel32/kernelbase 均接受；四项 function-table API 和 RtlUnwindEx 还接受 ntdll。UnhandledExceptionFilter 与 RtlUnwind 仍 absent。原 arity、DWORD 截断、完整指针/token、动态查询和诊断身份保持。

Win64 `exception::Registrations` 私有拥有有序 vectored handler、单调 token 和 filter identity。NULL target 先于容量检查，现存上限 64、累计 token 上限 256；删除不回收 token，first 标志经 DWORD 截断后决定插入顺序。filter 保留完整指针和旧值返回，仍未用于扩展 Win64 顶层异常搜索。

`unwind::FunctionTables` 私有拥有静态/动态数组和 callback 元数据。保留 128 项上限、按 guest 地址排序的查询、静态表保护、12-byte record、原输入验证与输出故障顺序。异常搜索只借静态值迭代视图，guest 内存中的表仍在查询时读取。callback 通过显式结果交回 Process64，owner 不执行 CPU。

Runtime64 继续保存返回帧，并提交 Raise/Unwind/FunctionLookup 控制结果、API 次数与 trace；guest handler、跨 budget 恢复、同栈帧 unwind 和 CONTEXT codec 保持原路径。PE32 原 SEH 编排仅改用 bound ID，不改变回调、FS 链和返回/清栈顺序。

PE32 的异常搜索、展开和普通 UnhandledExceptionFilter 执行现归 `x86/process/exceptions.rs`；公开 `seh::{raise,unwind}` 保留重导出兼容入口，API 直接委托 Process。`win32/x86/context.rs` 单独承担受检 CONTEXT 编解码，不借 ApiState、scheduler 或执行器。SEH 仍恢复异常入口保存的上下文，普通 filter 仍用 caller-context invoke，catch transfer、退出、fault 与 collided-unwind 拒绝策略均保持。

执行边界基线 `c42be9b`，两个 codec 和 raise/unwind 主体经路径替换后原文一致，callback helper 未改；完整 release Win32 259、IR 31、fmt/release lib/bin strict Clippy/build 通过。不跑性能或游戏矩阵，证据 `wire-out/refactoring-2026-10-14/r4-exceptions32-{source-checks,verification}.json`。

旧行为基线 `b014544`。新增 provider/ABI、完整 token/filter、坏返回帧和累计耗尽回归先在旧实现通过。原 function-table handler、静态注册、记录校验、callback 结果校验和 CONTEXT codec 经 owner/ID/借用及空白归一核对一致。验证记录为 `wire-out/refactoring-2026-10-14/r2-exception-verification.json`。本批保留既有能力范围，不修改 IR、执行器或缓存，不跑性能矩阵。

release 新增 Win64 边界回归 2、Process64 实际异常/展开和编排 8、PE32 异常 7、catalog 一致性 1，以及 fmt、release lib/bin strict Clippy/build 通过。累计 **867 exports**。
