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

序列化入口另外按 `UnityEngine.ISerializationCallbackReceiver` 的真实 interface contract
准入 OnBeforeSerialize/OnAfterDeserialize；核对 rooted receiver 的 virtual resolution
必须等于请求的 implementation。同名而未实现接口的方法拒绝，显式接口实现同样按契约核对。
零参数接口回调可携带 guest 的非 NULL 空参数向量；不解引用，也不传给 native invoke。

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
reference，以及实际 class 为 reference type 的 GENERICINST / Object 引用。泛型值类型、
byref、raw pointer 和含引用的 inline struct 拒绝整份描述，不复制 host bytes。
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
- boxed-zero allocation 改用 metadata-derived value layout，不再使用 InputBinding 类名/88B
  分支。含引用值数组和 owned unbox payload 的细节见
  `native-mono-value-payloads-2026-10-11.md`。
- frozen corlib 的 resolved generic reference backing fields 由 metadata 描述；包含实际
  private/NonSerialized storage，不改变 Unity serialized field 筛选。已有 native SZ array
  立即 pin，复用 canonical instance identity，再逐 slot 转换；不暴露 native headers。

## 实际证据

最新输出 `wire-out/native-mono-unity-assets-probe-17`（实施 worktree）：

- exit0；41 次后续 metadata-qualified 构造（历史 component19/asset1/map2/action19）及2次实际序列化回调；
- 最初 ColorCurves 构造、强制 GC 图验证及8次真实 engine curve cleanup 通过；
- 990 field cells /230 native writes /138 reference barriers /175 refreshes，990 GC checks；
- 34 virtual resolutions；5 reference-element arrays /8 value-element arrays，全部通过 GC 核对；
- 572 array reference barriers /32 scalar writes；1个借用的已有 backing array；
- 91 actual allocations（含3 boxed values）、588 borrowed roots、430 canonical strings；
- 所有相关 roots/field/array/string views 通过 GC 检查并释放/撤销；
- guard233 checks /0 denied，original Mono 未 attach，`full_game_backend=false`。

此前 `probe-11` 是删除逐类 ctor 名单后的原始检查点（18次构造/602 cells）。
本轮继续填充 InputBinding[35]、更多 InputSystem value arrays、两次 Map callback、
VolumeProfile/List backing storage 和后续 URP components。未知 managed entry 在调用前
形成可检查的有限诊断边界，未把该方法返回为成功。

下一个真实请求是 Renderer2DData::.ctor（0x06000210），位于另一个 managed image，
尚未准入/执行。实际 IL 依赖 LayerMask/Vector3、Dictionary<uint,Material> 和
ScriptableRendererData base ctor，见 probe-17/startup.log 的 pending listing。
继续完善 image family 与 typed field shape，不恢复逐类 token 名单。

native-value-unbox-calls=0：游戏尚未实际请求该 wrapper；新 owned payload/coalescing
有单元验证，不据此声称真实游戏 unbox ABI 已验收。此次 timing 同时存在 release tests，
只用于诊断记录，不能作性能对照。

完整 `cargo test --release --features native-mono-backend`：454 passed /7 ignored；
all-target feature Clippy `-D warnings` clean。日志为
`wire-out/native-mono-value-layout-release-tests.log` 和
`wire-out/native-mono-value-layout-clippy.log`。
