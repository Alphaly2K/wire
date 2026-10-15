# 路径辅助与通用句柄契约

基线为 `cfe475d`，运行时与目录枚举契约 `240e4ac` 相同。两组独立回归分别在旧实现通过并提交：路径辅助为 `3bb016f`，句柄为 `cfe475d`。本批收敛 19 个 export 的 provider、参数与执行策略、绑定 ID；保留原资源 owner、路径算法、故障提交和架构能力范围。

## 范围

| 家族 | export 数 | PE32 | Win64 |
| --- | ---: | --- | --- |
| FileApi（已有） | 45 | 32 Typed / 13 ABI-only | 39 Typed / 6 Absent |
| PathApi | 13 | 13 Typed | 13 Absent |
| HandleApi | 6 | 2 Typed / 4 ABI-only | 6 Typed |

路径辅助包含 SHLWAPI 的 Exists、IsDirectory、RemoveFileSpec、RenameExtension、AddExtension、UnquoteSpaces A/W 与 imagehlp 的 MakeSureDirectoryPathExists。主 PE32 gateway 直接执行绑定 ID；file_api 的公开冷 facade 保持原 provider 大小写与 GetFileType 排除范围。互斥 Core/Helper ID 代替 Option<FileApi> 与名称组合，A/W、Add/Rename、Exists/Directory 由 ID 选择。NULL、CP932、UTF-16、root/prefix、MAX_PATH、checked output、LastError 和 trace 算法保持；原 Hangover/Wine LGPL 来源和版权完整保留。

句柄包含 GetStdHandle、SetStdHandle、CloseHandle、DuplicateHandle、GetHandleInformation、SetHandleInformation。PE32 仅 GetStdHandle/CloseHandle Typed，其余保留 ABI-only 的零返回、LastError=120、输出与资源不变；provider 仅 kernel32。Win64 六项均 Typed，provider 为 kernel32/kernelbase。DWORD selector、access、BOOL/options、mask/flags 在原参数位置截断，HANDLE 与输出地址保留完整宽度。Wine 10.0 spec 用于核对参数数量；句柄宽度继续按原 Win64 adapter 与 Windows HANDLE 语义表达，不能将 spec 的 long 一律视为 DWORD。

typed HandleApi 在原 handler 位置选择分支，保留各资源的查询顺序、保护位、错误码、提前完成及 unblock 差异。PE32 CloseHandle 先关闭资源、更新 named object、写 EAX，再读取返回帧；返回槽 fault 时已有资源提交。Win64 则先保存返回 frame，再执行 handler；输出覆盖返回槽仍使用保存的 frame。没有抽取通用完成框架、统一 handle arena 或改变 CPU 控制结果。

Unquote 的返回元数据为 Void，保留现有零 EAX carrier。已删除本批的重复 PE32 手写 ABI 与 Win64 arity/DWORD 列表；Wine spec 继续为其余 legacy API 提供 ABI。普通调用只用 cold-bound ID，未增加逐调用字符串注册解析。

## 验证进行中

路径、句柄、Win64 absent 与 catalog 唯一性专项回归通过。完整 release 为 425 passed / 7 ignored（包含 Unicorn 参照），debug Win32/AMD64/FPU 为 273 passed，显式 native GPU/Metal 为 7 passed；fmt、strict all-target/all-feature Clippy、release build、工具 12 项通过。新增 PathRemoveFileSpecA(NULL) 与 GetStdHandle 的 PE32 raw/IR、AMD64 原创 fixture 均通过旧/新 binary 的 12K smoke，默认 case 集合保持。

before 复用 `r2-file-search-cold-route-wire/wire`，SHA256 `927662997fbc71b5102113ab1b5ca2f8f9f5c86d723010d177d7f56be659438f`；候选冻结在 `r2-paths-handles-bound-wire/`，SHA256 `4bb34fe205cc28376b9f0087f7e9ab0cb85271ad450cc78b1b469eda455edde9`，5,832,280 bytes。thread-id 与 PE32 SSE/IR 的 700M 三标签对照已通过，中位数最大 CPU 成本 +1.74%；SSE 首对同二进制 control 也明显波动，全部轮次保留。

后续按 42 个 case/path 执行三对 AB/BA：AMD64 integer/memory/SSE 和 PE32 integer/bit IR 为 700M；PE32 raw/FP IR、AMD64 gateway 为 100M；AMD64 FP/bit/两类 alias 为 300M；PE32 九项 gateway 的 raw/IR 为 50M。以上连同两项初筛构成完整矩阵。所有计时串行，与 Cargo、sample、runtime diagnostics 分开；PE32 使用 utility QoS clamp 与 null API log。

42 条路径现已全部通过 work/retired/返回值/API count 校验。memory、PE32 integer/bit IR、Win64 version/file-type/search 的初筛中位数超过调查阈值；六项正在追加相同 work 的 before/after/control 三标签对照，按所有六对合并，不选择性丢弃原始慢轮次。后续文件 section 的四项基线草稿尚未注册 test target，未计入本批 425/273 测试数。

六项补样已完成。整数、bit 与 search 的成本消退；memory 六对合并仍为 +5.93%，file-type 为 +11.09%，version 为 +3.98%。该版未接受，全部运行时代码暂时撤出，完整补丁和两个 catalog 源码已保存，等待独立的访存 helper 布局基线后再恢复。没有用正确性通过替代性能验收，也没有继续执行本版 Alice smoke。

独立 memory diagnostics 的 IR=12008、fallback=0、translations=16、invalidations=0 一致。CheckedSegment 的 2156 条指令与规范化操作数全部相同；相应文件为 `r2-paths-handles-{checked.asm,checked-code-summary.json}` 与 `r2-paths-handles-memory-diagnostics/`。常用 checked_access/read_int/write_int 的入口地址和缓存行偏移发生变化，这作为布局隔离的调查线索；尚未将它认定为唯一原因。

单独路径首版、网关边界三个候选、页缓存命中内联候选均因持续性能成本未接受，相应运行时代码已撤出本批。原始结果分别见 [路径首版](refactoring-path-helper-catalog-2026-10-15.md)、[网关边界](refactoring-gateway-boundary-2026-10-15.md)、[页缓存候选](refactoring-page-cache-inline-2026-10-15.md)，不能沿用它们的性能作为本批通过证据。

后续独立的四个访存 helper 布局候选也因 PE32 SSE 三对持续回退而拒绝，见 [helper 布局记录](refactoring-memory-helper-layout-2026-10-15.md)。本批 19 项迁移继续暂缓，架构说明恢复为已合入的实际状态；完整补丁、契约源码和测试保留。先推进可独立验收的八项文件 section 契约，后续再调整本批的接入结构。
