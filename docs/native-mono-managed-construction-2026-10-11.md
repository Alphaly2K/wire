# 元数据驱动的 Mono 构造桥接（2026-10-11）

后续构造入口不再按类名或 method token 建名单。托管构造函数和静态初始化仍由
冻结的 ARM64 Mono interpreter 实际执行；Rust 负责受检调用、对象身份和跨架构数据。
本阶段仍是有限启动诊断，`full_game_backend=false`，不是完整游戏后端准入。

## 构造入口

`src/x64/process/mono_startup/constructors.rs` 依据真实 metadata 核对：

- typed Method identity，method 名 `.ctor`；
- declaring class 属于已按 SHA256 锁定的 CoreModule、render-core、URP 或 InputSystem；
- 非 static / abstract / PInvoke / InternalCall / native / runtime / synchronized；
- nongeneric、非 inflated、reference receiver、instance `void()`；
- bounded 非空 IL body；rooted native receiver 的实际 class 与 declaring class 相同。

没有将一般 `mono_runtime_invoke` 放开：有参数构造、value-type receiver、任意方法及
其他程序集仍未准入。既有 owner-thread、strong roots、guest 参数检查、固定 callback
和异常拒绝继续生效。P/Invoke resolver 对未登记方法拒绝解析；Unity InternalCall
只安装经过签名与原始 guest target 核对的预编译 adapter，不向 Mono 注册未知地址。
动态 code mappings 仍由冻结 runtime 的 no-exec guard 拒绝。

`animation.rs::execute_managed` 提供统一的活跃 receiver scope，执行真实 IL，允许
普通 CLR 类没有 ScriptableObject callback，也允许该 rooted receiver 实际绑定一次。
Profiler marker 使用同一个受检 UTF16 span/scalar adapter，不按托管类或 marker 名扩展名单。
原始 marker flags=0 时真实返回 NULL，这一正常结果原样保留。

ColorCurves 的最初有限资源验证器仍保留：它核对八条真实曲线的创建、托管图和实际
清理，是引擎资源/生命周期的专门验证，不是后续构造函数的分派名单。VolumeParameter
对象图及 Vector/Color 的既有精确值布局也保留，尚未扩展为任意堆图。

## 字段布局

`field_layouts.rs` 为 InputSystem 分配的对象自动生成 serialization specs：枚举声明和
同 image 的 parent chain；依据 public 或 `[SerializeField]` 选择字段；排除 static、
literal、readonly 和 NonSerialized。属性只查询 metadata，不运行属性构造函数。
不再硬编码 InputActionAsset、InputActionMap、InputAction、InputBinding 的字段名。

当前形状包括 exact-width primitives、四字节 enum、String、class reference、SZARRAY
reference。byref、raw pointer 和含引用的 inline struct 拒绝整份描述，不复制 host bytes。
`fields.rs` 再核对 declaration、type、offset、alignment、instance size 和 cell overlap，
仅发布字段宽度的 owned guest views，不映射 Mono object header。

构造前同步所有已发布字段与引用数组。构造后的字符串采用独立 canonical string root，
然后以 native fields 更新 guest snapshots；未提交的 guest 写入不能被 refresh 吞掉。
Reference 写入调用真实 Mono setter/barrier，对象 token 不替代 native pointer。

新增托管类如果落在上述 image、signature、field-shape 范围内，入口和字段规则无需新增
Rust 类名/token 分支。新的 native ABI 或新的数据形状仍需补类型实现和验证。

## 本轮一起完成的数据边界

- 受检 virtual-method resolution：rooted reference receiver、declaring assignability、
  非 static/value receiver；只返回 typed metadata token，不执行方法或发布 vtable。
- 非空 InputActionMap / InputAction 引用数组：实际 Mono SZ allocation、立即 pin、
  guest bounds/length readonly、payload 仅含 guest tokens。全 batch 验证后逐 slot
  运行真实 write barrier；GC 前同步、GC 后核对、释放 roots 后撤销 views。
- canonical string / empty-array alias 保持唯一身份，拒绝错位、替换、退休和容量越界。
- `mono_string_new_wrapper`、`mono_unity_string_empty_wrapper` 实际 native 实现，校验
  owned domain，复制 bounded 输入并 root/publish；empty singleton token 不变。
- InputBinding boxed-zero allocation 仍使用冻结 88B/align8 的精确值布局，以实际
  `mono_value_box` 分配并立即 pin。没有发布 unboxed host address。

## 实际证据

实施 worktree 输出 `wire-out/native-mono-unity-assets-probe-11`：

- exit0；18 次后续 metadata-qualified 构造：7 个渲染 component、asset1、map1、action9；
- 最初 ColorCurves 构造及8次真实 engine curve cleanup 仍通过；
- 602 field cells /142 native writes /68 reference barriers /80 native refreshes，602 GC checks；
- 34 virtual resolutions；2 reference arrays /10 slot writes /2 GC checks；
- 56 actual allocations（含1 boxed value）、132 borrowed roots、52 canonical strings；
- 所有相关 roots/field/array/string views 通过 GC 检查并释放/撤销；
- guard232 checks /0 denied，original Mono 未 attach，`full_game_backend=false`。

此前 `probe-10` 已以旧逐类构造路径通过同一终点。新路径没有改变该终点或字段数，
构造后的 refresh 包含 asset 的3个字段。CPU user5.759918s /wall10.026626s 只是本次诊断
运行记录，不是性能对照或端到端加速结论。

下一个真实请求是 `mono_array_new`：InputBinding[35]。它是含字符串引用的结构体数组，
不能当作 reference array，也不能 memcpy native payload。下一步应基于 metadata 生成
结构体的 scalar/reference slot 布局，再实现 token 转换、真实 write barrier 与 GC核对。
后续 `mono_object_unbox` 同样应返回 owned typed guest payload，而不是 native pointer。

回归还覆盖未知类/字段名称无需名单、非法入口在 body/call 前拒绝、属性资源释放、
late unsupported field shape 整批拒绝、late forged array token 不发生 earlier native store、
header readonly、slot size/index/alignment、pending guest writes、canonical 身份及退休。

完整 `cargo test --release --features native-mono-backend`：447 passed /7 ignored；
all-target feature Clippy `-D warnings` clean。日志分别为
`wire-out/native-mono-generic-constructors-release-tests.log` 和
`wire-out/native-mono-generic-constructors-clippy.log`。
