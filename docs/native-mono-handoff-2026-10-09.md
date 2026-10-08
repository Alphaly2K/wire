# 原生 Mono 持续推进交接（2026-10-09）

用户已授权自主决策、自己测试修复和本地提交，继续推进原生Mono计划；不push。另一个agent
可能处理SSE，故本任务一直使用独立worktree，不改CPU/SSE/IR，不合并主分支代码。
主checkout仅新增本文，方便后续任务找到成果。

## 唯一工作区

- Worktree：`/Users/alphaly/.codex/worktrees/native-mono-shared-memory/wire`
- 分支：`codex/native-mono-shared-memory`
- 当前提交：`c383320`（主checkout保留原运行路径）
- 实施计划输入：主checkout `wire-out/unity-native-mono-plan-2026-10-09/plan.md`

已有阶段提交：

| 提交 | 结果 |
| --- | --- |
| d7f79c4 | 默认关闭的受控shared Memory及8项故障/生命周期回归 |
| 3940ceb | pinned真实数组、两native Mono threads、primitive checked IL、真实类库及object/array叶ABI |
| 9b513d5 | BDWGC解释器栈roots修复、typed Windows查询/流服务、MSVC MonoClass ABI opt-in |
| cd84974 | 原创PE↔ARM64 IL混合float/double/int64、24B hidden sret/ref/out及托管异常 |
| c321e7d | 选择性IL优化、缓存unchecked转换拒绝、wrapper/image inline边界、冻结AB/BA工具 |
| c383320 | 同image受检静态IL调用、真实Unity绑定metadata清单、完整patch stack重复构建 |

具体实现、边界和复现命令在worktree的 `docs/native-mono-services-and-gc-2026-10-09.md`。
历史M1/M2文档仍保留，不要把历史kernel32/Unix故障当作当前fixture状态。

## 当前已实测

真实Windows类库的File.Exists/Directory.Exists/FileStream、UTF-16、seek、EOF、180123B
分块读写、append、独立最终180124B字节核对，以及owner撤销/LastError两线程均通过。
BDWGC正确保留活跃IL locals，并在原线程仍attach时释放退栈locals；不是注册整个堆或stack
永久保活。原始MonoClass叶函数在MSVC位字段opt-in后匹配，完整metadata ABI仍未准入。

受检IL仅许可349/613个Mint opcode，scalar/struct/cpblk/initblk和同image静态托管CALL。
递归/parent byref、cached target拒绝、外部helper拒绝均通过；15个访存故障无部分写。
reference/array/general helper、virtual/delegate/calli仍有未覆盖项，不能当通用sandbox。

真实游戏153个Managed DLL扫描得到11537条InternalCall和1393条P/Invoke，unresolved=0；
7921条名称含_Injected。仅扫描声明，未执行游戏方法。所有条目ABI准入为0，不是使用频率。
原始用户Mono SHA256：`86a5a4de6bcd74a637d63c96d822dc989d4ffaed6ef1c15c0e6e1f528ae6708f`。

选择性IL优化纯托管对照：冻结新旧native Mono，同一IL/driver/类库、三对AB/BA、每配置21
个20000000迭代样本，checksum1523661319；中位数0.297357→0.138959秒，约2.14×。
这是配置microbenchmark，没有真实游戏native backend加速数据。游戏后端、M3准入仍false。

最新完整M2 guard `checks=410 denied=0`；M1 core/interop/threads/gc/layout通过，5个executable
memory反向用例以86拒绝；链接审计304对象/668依赖/0 unmatched。独立M2 Clippy clean。
默认/feature完整Rust回归在共享Memory阶段已通过336/344项、各7 ignored；之后未改核心Rust。

## 保留的输出

全部位于上述worktree的ignored `wire-out/`：

- `native-mono-checked-runtime`：锁定source、16个patch、无JIT ARM64 runtime与链接审计；
- `native-mono-game-classlib`：同runtime的symlink、真实类库及预编译interp支持；
- `native-mono-m2-services`：完整M2 driver、results、logs、隔离VFS、contracts及输入hash；
- `native-mono-scope-opt-perf`：冻结AB/BA的命令、hash、CPU/wall及原始样本。

主checkout `wire-out/native-mono-m1` 的源码archives/开发AOT builder、Unity SDK和游戏均只读。
不要重新下载，不修改SDK/游戏，不重新构建已冻结的开发builder。构建后等完成再audit/运行，
不得拿旧results.json证明新的patch。prepare如今认可完整patch stack的精确source prefix，
后续patch覆盖旧hunk仍可重复；未知source编辑保留并失败，不能reset它们。

## 下一阶段

继续完成受控managed reference slots/write barriers、array/helper检查与共享对象生命周期；
收敛生产guest execution token/Mono TLS/GC roots/scheduler；在actual binding清单上建立完整
Win64↔ARM64参数/return/marshaling分类与gateway生命期，然后才接provider/loader及Unity
VM初始化。不要因代码工作量大就宣布阻塞；确有外部输入或不可解决契约时记录证据。

不以同时启用两个VM、按函数回退旧VM、禁用GC、假API成功、开放未知native pointer或生成
宿主机器码绕过准入。大量未实现项仍是继续工作，不是已完成。保持独立分支并更新本文。

worktree写入需sandbox escalation，既有auto-review一直允许授权的本地开发。默认Git签名
因pinentry交互失败，采用单次 `git -c commit.gpgsign=false commit ...`，不改全局配置。
