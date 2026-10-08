# M3 后端选择、单 VM 生命周期与预检

当前源码增加默认关闭的 `native-mono-backend` feature及 `src/mono/session.rs`，只实现后端
选择和生命周期契约，没有加载native runtime、拦截Unity Mono导出或启用真实游戏。
默认解释执行/原始Mono路径继续承担游戏运行。所有代码仍在独立worktree，不合并主分支。

## 已实施的生命周期规则

初始化前一次性选择OriginalX64或NativeArm64。native必须同时满足：用户/配置请求、
已知binary fingerprint、完整native ABI、checked memory/roots、生产Windows服务与
scheduler、Unity初始化callbacks、required typed exports以及无动态代码配置。
缺任一项在初始化前选OriginalX64；fixture通过或native symbol存在不能代替这些证明。

开始初始化后不再选择VM。初始化失败或晚到的未支持调用将会关闭新call/resource准入，
记录第一条故障；不能按单个函数回退，更不能在同一对象图上启动另一VM。恢复要经过
隔离进程重启，状态模块不实现进程重启或实际Mono C API调用。

Active时每个native/guest调用持有CallLease，真正的root、metadata、callback、shared
backing owner分别持有ResourceLease。shutdown先停止准入，owner再释放真实资源/撤销
mapping，并等待in-flight调用结束。任何一个lease仍存活都会阻止cleanup-ready。
该模块仅计数和拒绝错误状态；caller必须先释放实际资源再drop其lease，并在实际VM
cleanup成功后记录Closed。它不会自动free handle或native storage。

五项状态回归覆盖所有8个preflight缺项、初始化失败不切VM、运行中失败不丢活跃root、
四种资源阻止cleanup，以及remote in-flight调用与callback owner在shutdown期间的保留。
模块尚未绑定到production provider，因此不能把这些状态测试当作已完成游戏生命周期。

默认Rust回归336项通过、native-mono-backend feature回归349项通过，各7项需要真实GPU的
ignored。feature Clippy `-D warnings`通过。完整debug回归另发现原有registry root handle
使用then_some提前计算减法而溢出，改为惰性then closure；既有Win64 registry用例及两套
全量回归验证修复。该一行修复单独提交，没有改CPU/SSE/IR。

## 当前实际预检

工具：`tools/mono/profile.py`。输入最新M2 results和先前冻结的实际Mono query列表。先核对
runtime/原始DLL/IL/source/patch hashes与clean noexec guard，再输出不可启用的profile及
`queried-mono-exports.tsv`。symbol presence、原始DLL是否有export、已测leaf ABI和typed
production gateway是不同列；所有production gateway准入为0。

当前样本Mono SHA256：
`86a5a4de6bcd74a637d63c96d822dc989d4ffaed6ef1c15c0e6e1f528ae6708f`。
293个Mono query中289个native symbols存在，21个query落在已核对leaf子集。缺失的4个：
`mono_gc_set_heap_validate_frequency`、`mono_gc_set_heap_verifier_callback`、
`mono_profiler_startup`、`mono_profiler_init_etw`。前三个在原始DLL也不存在；ETW属于
Windows条件功能。query包含可选能力查询，不是mandatory调用清单、使用频率或阻塞证明。
不要为了把计数做齐而注册假成功stub。

生产准入仍有5项明确未完成：完整ABI、general checked memory/roots、production Windows
services/scheduler、Unity init callbacks、required typed exports。生成的profile选择
`original_x64`，M3及native game backend均false。当前output位于ignored
`wire-out/native-mono-m3-preflight/`，不是可发行/可执行游戏profile。

```sh
python3 tools/mono/profile.py \
  --bridge-results wire-out/native-mono-m2-services/results.json \
  --queried-exports /Users/alphaly/Projects/wire/wire-out/unity-native-mono-plan-2026-10-09/evidence/mono-lookups.txt \
  --output wire-out/native-mono-m3-preflight
cargo test --offline --locked --features native-mono-backend
```

## 继续实施边界

后续需要把typed exports、原生服务owner、shared object ownership、Mono TLS与guest scheduler
绑定到此生命周期，而非只把state设成Active。Unity allocator/TLS/log/plugin/liveness
callbacks需逐项保留真实ownership、参数、返回和重入语义；源文件中的旧
`MonoMemoryCallbacks`声明与真正`MonoAllocatorVTable`不是同一布局，实际函数接收后者。
不要直接采用那个旧声明，也不能把guest callback pointer直接传给ARM64 Mono。

当前M2已有精确reference槽、pinned Int32[]、owner queue、闭包static入口池、domain generation
及74项method/signature/type叶ABI证据；general objects/arrays/helpers和正式共享allocation
仍需要完成。HostRegion要求所有native访问持lease；native Mono/GC对整块metadata/heap的
直接写入不自动满足这个条件。不能通过映射整个native heap或永久root所有分配绕过去。
先解决这些边界，再启用真实provider/Unity生命周期与端到端性能测试。
