# 真实 ColorCurves 字段反序列化（2026-10-11）

已执行原始 UnityPlayer 的 ColorCurves 反序列化，而非只执行构造函数。
完整游戏准入仍为 false；当前有限 `fields` checkpoint 停在随后 FilmGrain::.ctor
（URP token `0x0600054c`）之前。既有 `curves` checkpoint 仍停在构造完成处。

## 实现

新增 `mono_startup/fields.rs`：只有已初始化图中显式登记的字段可见。
逐字段核对 instance/static/literal、declaring class、实际类型、byref、offset、
instance size、alignment 与重叠，建立精确宽度 owned cells，不映射对象 header、
vtable 或 padding。

ColorCurves active 的实际 native offset 为 48；八个参数 reference offsets 为
56..112。每个 TextureCurveParameter 只暴露 m_OverrideState 与 m_Value；
每个 TextureCurve 暴露 m_Curve、length、loop、zeroValue、range。
AnimationCurve.m_Ptr 只暴露受检且与已创建 guest resource 一致的只读 8B cell。
共有 73 cells：65 个标量/引用和 8 个只读 engine pointers。

子对象从仍强 rooted 的已验证父图读取，先创建 pinned v2 strong root，再发布 owned token；
24 个 borrowed roots 与两次 mono_object_new 的 allocated objects 分开计数。
reference cell 不含宿主 MonoObject 指针，只含兼容类的 live owned token 或 NULL。

整个待写 batch 先完成 root/type/Boolean 校验，再更新 native heap。
mono_field_set_value 的 reference 参数是对象本身，scalar 参数是值地址；已核对冻结
Mono object.c 的 mono_copy_value，reference 更新使用 Mono 的实际 write barrier。
原始 Unity 以 obj=NULL 调用 barrier 时，用登记的精确 field destination 找回 owner；
不能靠未知 guest destination 访问宿主堆。即使 token 不变也执行实际 barrier。

真实 GC 前同步 guest cells，GC 后逐项核对 native scalar/reference 与 owned snapshots。
构造初态仍严格验证；进入序列化后允许已受检 scalar 合法改变，同时保留 resource
identity 与图 reference 核对。owner cleanup 执行真实 managed finalizers 和 guest destroys，
随后释放所有强根、unmap/revoke views，关闭后禁止 guest resume。

## 实测与回归

输出目录（实施 worktree）：`wire-out/native-mono-unity-fields-probe-03`。
冻结 wire、命令、runtime/game hashes、完整日志与 results.json 都在此目录。

- exit 0，停止前已越过旧 `0x1807b07ae` active 写入故障和 `0x1807b8c7e` m_Ptr 读取故障；
- 73 cells，16 次 native field writes，其中 8 次真实 reference barriers；
- 73 次 field GC verification，24 borrowed root GC verifications，全部 views retired；
- ColorCurves graph 验证 3 次；8 次真实 managed finalizers 与 guest destroys 完成；
- 130 images，1,227,525 metadata queries，25,154 metadata handles；
- guard 232 checks / 0 denied，original Mono 未 attach；
- CPU user 5.105876s，system 0.193603s，wall 9.042585s；这是有限诊断，不是端到端性能对照。

Rust startup targeted 34 passed；完整 release feature 434 passed / 7 ignored；
feature all-target Clippy `-D warnings` clean。完整回归日志
`wire-out/native-mono-unity-fields-tests.log`。
新增回归覆盖批量失败不误写较早字段、非法 Boolean、伪造/interior/incompatible token、
引用 C ABI 参数、NULL/替换、强根去重与失败回收、精确宽度与 release-before-unmap。

## 下一步

真实下一请求是 FilmGrain 构造函数。继续接 URP 常见后处理组件的实际 ctor 与
VolumeParameter scalar/enum/inline value 字段，保持冻结 image 身份、字段类型与
callback owner 检查。生产 finalizer owner queue、完整 objects/arrays、全 Process
safepoints 与游戏 native backend 仍未完成。
