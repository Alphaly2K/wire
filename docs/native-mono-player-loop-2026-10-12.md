# 配置克隆、Awaitable 与 PlayerLoop 接入

在本地 main 继续，上一批已提交为 `45c153a`。目标仍是 Alice 的可交互游戏界面，
不能把配置加载、过渡背景或 Present 当作完成。

probe-129 已实际执行 Internal_CloneSingle_Injected，返回原 Unity 的真实 v2 句柄。
普通克隆和带 parent 克隆共用 live engine pointer、Boolean 与结果类型检查。
接着完成背景配置构造；数组元素因旧六个 image 集合被拒绝。typed array 现按当前
持有且记录指纹的完整 live image family 检查，仍要求 concrete metadata 和完整值布局。
这不接受开放泛型、未知 image 或一般原生指针。

Awaitable 的十个固定声明先逐项核对签名与原 registration target，再注册已编译入口。
next frame、seconds、fixed update、end frame 和 AsyncOperation 转发原 Unity；完成查询、
cancel、release 仍由原引擎执行。原 engine allocation 只在私有注册表中，native IL 收到
永不复用的 token；释放后地址复用不会恢复旧身份。原 IL 创建的强 GCHandle 经 typed
target 核对后转交已有 guest handle registry，让引擎释放对应的真实 native handle。
不把宿主句柄值交给 Win64，也不假造异步完成。

PlayerLoop 的 default/current getter 与 setter 转发原始引擎。数组里的 Type 与 delegate
使用真实托管 roots/reference barriers。两个 IntPtr 字段只对冻结 CoreModule 的
PlayerLoopSystem/PlayerLoopSystemInternal 精确 declaring class、field、offset、type 准入。
它们是 Windows 引擎地址数据：仅原 getter 的作用域可登记，限定实际 UnityPlayer image
范围、可读地址及两个字段各自的已产生集合。普通值布局继续拒绝 IntPtr；这种 slot
也不能作为 blittable GPU scalar payload 或普通 native invoke pointer 使用。

mono_object_new 与 String/Array 分配一样拆为 prepare/execute/finish，实际 Mono 分配期间
不持 Metadata/Memory/Runtime 的 Rust 借用，临时真实强 root 覆盖返回前的生命周期。
旧直接 dispatch 的对象分配分支已移除；新增回归在实际分配函数里触发 profiler heap
resize，并执行原 guest 指令，覆盖同步 GC 重入。

真实 PlayerLoop/Awaitable 执行结果仍待后续 probe；安装和单元回归不算实际游戏调用。
full_game_backend=false。

probe-130 已越过完整 live image family 的数组布局检查，随后在嵌套
`mono_unity_gc_disable` 停止。GC scope 现由主路径与所有 Metadata 重入共享同一份
计数和真实 Mono 状态，probe-131 已实际越过此处。该轮继续到配置克隆中的
`mono_gc_wbarrier_set_field`：NULL owner/NULL value 写入真实 Unity 临时堆槽位，
旧重入路由只接受 callback stack，因此拒绝。

该临时槽位现按真实 live allocation ledger、8B 对齐、owned non-code access 和
完整引用身份检查准入；padding 与已释放分配仍拒绝。实际 HeapAlloc/HeapFree
回归通过。2026-10-12 当前完整 release feature 回归为 **521 passed / 7 ignored**，
all-target Clippy 与格式检查通过。临时槽位修复的下一轮真实游戏验证尚待完成。

测试不再每轮复制游戏：主目录中的一份隔离目录通过临时软链接复用，退出/失败/
超时均清理临时输入与 wire 快照。详见 `test-storage-2026-10-12.md`。

主目录 `wire-out/native-mono-m1` 现复用唯一一份 locked source，验证旧 9 个补丁前缀
后应用其余 14 个补丁。runtime/AOT 工具使用 fresh build objects；运行库仍禁用 JIT，
启用 MSVC Class ABI。构建审计仍为 304 linked objects / 668 dependencies / 0 unmatched，
十套 native fixtures 的普通与 Unity incremental=3 配置均通过，五个 executable
memory negative routes 在两个配置下均 exit 86。

- 新 runtime SHA256：`9c777efecc3066b3f574968e2566591cafa77a0d35306cb920dc77177f913b4c`。
- 新 support SHA256：`def3ecff994baa8542b1926b6b19875457224fe841dc13076a14670cf13c43c7`。

probe-132 在实际执行之前被旧 runtime 指纹拒绝；已完成上述重建验证后才更新精确
准入指纹。未放宽 runtime/support/game fingerprint 检查。该轮未进入 Unity 运行，
退出后临时软链接与 wire 快照均实际清理。

probe-133 使用新的精确指纹实际运行 Unity，17.27 秒后停在角色配置容器的
`value layout requires an owned metadata value class`。已越过临时堆写屏障和嵌套
GC scope，实际克隆了 CameraConfiguration 和 CharactersConfiguration，并展开
其通用字段/容器图。该轮目录仅 5.1 MB，只有日志、哈希、结果和画面；退出后没有
游戏 runtime、副本 binary 或 dylib 副本。

值布局原先将每个子 struct 限定在根 struct 的同一 image。新增 `layout_in` 由
调用方传入已持有且记录指纹的 bounded image family；根类型仍必须属于明确的
根 image，子值类型只能来自传入的 family。typed arrays、普通字段、box/unbox
和 managed scalar aggregate 使用同一契约。闭合类型、递归深度、字段声明者、
重叠、对齐、padding、Boolean、reference barrier 与 raw pointer 拒绝规则不变。
single-image `layout` 保持原限制，给只接受 CoreModule 的固定 ABI 使用。

这项修复随后由 probe-134 验证越过角色配置；尚无执行成功的
PlayerLoop/Awaitable 证据。

跨 image 值布局修复之后，完整 release feature 回归 **522 passed / 7 ignored**，
格式检查与 all-target Clippy 通过。probe-134 实际越过所有 Naninovel 配置，包括
UIConfiguration/UnlockablesConfiguration，进入 EngineInitializationUI 与 CanvasScaler
构造；22.04 秒后停在嵌套 `mono_object_get_virtual_method`。这是尚未接入重入路由
的通用虚方法解析，非缺少这些类的构造函数。

嵌套虚方法解析现在复用现有 checked resolver 与 pinned receiver request，分为
prepare/execute/finish。执行 native resolver 时不保留 Metadata/Memory 借用；仍
核对 rooted reference receiver、声明者 assignability 和非静态方法，结果只发布
metadata token，不发布 vtable/code 地址，也不在解析时调用方法。
worker domain/current 查询日志限制为前 16 次和计数的二次幂；attach/detach 仍记录，
累计统计保持完整，避免长期循环输出同一条诊断。

probe-135 实际执行上述嵌套虚方法解析，继续展开初始化 UI 的 UnityEvent、Slider、
PersistentCall 与 ArgumentCache。22.43 秒后在 Resources.Load 的旧独立 2,000,000
work 预算停止。probe-136 增加诊断后重现：target=0x1800b88f0，最后
RIP=0x18123c362，depth=1，38,201 次 Mono 重入 / 13 次 managed invoke。
最后位置在原 Unity 资源分配路径；对象图仍持续增加，未据此宣称加载成功或无限循环。

已验证签名及原 registration target 的 Resources load/builtin 与 Object clone
adapter 现在选择独立 32,000,000 work 上限。普通标量、GC/profiler callback 保持
2,000,000；没有运行时任意预算开关，也不改变深度、借用、root 或地址检查。
预算恰好执行完 RET 时允许正常返回；失败诊断包含 target/RIP/work/limit/depth 和
重入计数。真实 guest 指令回归覆盖准确边界、无限循环拒绝、失败后 TEB/depth 复原，
以及大型资源预算不会影响前后的普通 callback。下一轮 probe-137 验证资源能否返回。
