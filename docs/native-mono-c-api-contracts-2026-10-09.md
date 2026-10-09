# 候选 Mono C API 与 Unity callbacks 清单

`tools/mono/c_api_inventory.py` 使用候选runtime的实际config/补丁宏和Clang AST，解析公开及
Unity扩展headers，另解析unity-utils、unity-liveness、mono-publib、mono-logger、oop、mini-unity
六个实际C translation units。只读源码/原始query，输出ignored wire-out，不初始化任何VM。
失败的编译不采用partial AST，并清除旧成功报告，避免旧contracts.json冒充新证据。

最新293个实际Mono query中289个候选native C签名已找到，71个具有本轮提取的实际函数
定义。其余218个只证明存在声明，不代表已执行或实现了跨架构gateway。未找到的4个与
native symbol预检一致：heap validate频率、heap verifier callback、profiler startup、ETW。
293仍是包含可选能力的query集合，不是mandatory-call清单或使用频率；不补假成功stub。

清单含spelling/canonical type、参数、返回、varargs、旧式unspecified prototype、data/
callback pointer分类，以及候选callback表field类型。23个query涉及callback或table输入。
Win64/ARM64完整ABI、pointer方向/范围/所有权、线程、重入和callback生命周期全部未准入；
long/wchar_t差异和旧式prototype需另核对。不会据源码签名自动生成可启用生产gateway。
所有query的typed_game_gateway_admitted仍false。

特别确认两个不同名字：

- header旧声明 `unity_mono_install_memory_callbacks(MonoMemoryCallbacks*)`：四个callbacks，
  顺序malloc/free/calloc/realloc，无version。
- 实际导出 `mono_unity_install_memory_callbacks(MonoAllocatorVTable*)`：version、malloc、
  realloc、free、calloc。真实mono_set_allocator_vtable核对version=1后才安装libc等价语义。

它们不是同一个symbol的可互换声明。原始guest callback addresses不能直接放进native表，
也不能把普通owned guest pages的malloc返回值转成ARM64可解引用地址。即使typed allocator
fixture能分配bytes，也不能让整个Mono/GC绕过HostRegion lease直接访问这种backing。

MonoLogCallback为五参数，包括32-bit mono_bool fatal和user_data；plugin finder返回的
const char*涉及返回storage生命周期；vprintf_func含va_list，Win64与ARM64必须在caller侧
解析为owned格式化数据再交给guest，不能转交异架构va_list。liveness带refs/realloc/world
stop-start callback，不能只映射函数pointer。UnityTLS的candidate C声明仍是opaque struct，
不能据此认为完整接口表布局已知。报告只是下一步适配输入，不证明这些callbacks已绑定。

输出：`wire-out/native-mono-c-api-inventory/contracts.json`、queried-signatures.tsv、7个AST/
diagnostics/dependency files。报告冻结406个transitive编译依赖hash，source/config/Clang版本、
实际参数和AST哈希。系统SDK依赖单独冻结，不修改SDK。与M3失败关闭profile配套使用。

```sh
python3 tools/mono/c_api_inventory.py \
  --runtime-output wire-out/native-mono-checked-runtime \
  --queried-exports /Users/alphaly/Projects/wire/wire-out/unity-native-mono-plan-2026-10-09/evidence/mono-lookups.txt \
  --output wire-out/native-mono-c-api-inventory
```

下一轮优先把明确typed exports/callback owners接入实际Unity启动诊断，结合真实参数确定
首个阻塞契约；保持单VM，不按函数回退，未完成一般managed共享/roots/scheduler前不宣称
真实游戏native后端完成或获得性能收益。
