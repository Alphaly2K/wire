# 精确标量 SSE 边界

当前为待验收候选，结果与编译原因见[编译隔离报告](../reports/refactoring-scalar-codegen-2026-10-19.md)。对话框迁移与该候选共同验证，原绘制迁移保持撤回。

`crates/wire-simd` 保存既有软件标量 SSE 语义，依赖 iced 的 Mnemonic 和锁定版本的 rustc_apfloat。库只接收输入位模式、精度/操作以及 MXCSR/RFLAGS 两字段状态，不接收 CPU、guest memory、IR、Win32 或宿主对象。根 `simd.rs` 重导出这些类型并继续拥有原整数向量 helper；x87 仍在架构执行器的原实现中。

封闭的 `Scalar` / `ConvertTo` trait 只允许既有 Single/Double 格式，静态调用进入内部库的具体函数。入口禁止跨库内联，私有泛型算法内联到入口，库自身固定一个 release 编译单元；根 crate 与其他依赖的 profile 不变。库没有动态分派、生成 guest 代码、常量输入捷径或宿主浮点近似。

调用者先完成所有源访存，再建立运算事务。库执行既有舍入、DAZ/FTZ、NaN、sticky exception 与掩码规则，返回结果或原 #XM 错误；调用者保留原本的目标、FLAGS、PC 与退休提交边界。源访存失败不触发运算，未掩码 SIMD 异常仍只提交允许的 MXCSR 状态。packed/raw 的逐 lane 异常累积及 scalar IR 的目的寄存器宽度仍由调用者协调。

新增普通 Win32 契约不需要修改此库。改变这里的算法、精度或编译配置时，继续执行真实 FPU/Unicorn、双架构 scalar IR/checked 对照、fault/guard/异常提交测试及浮点性能样本；不能将隔离视为放宽 ISA 验证的理由。

本库未跟踪阶段，冻结工具须显式包含 `crates/wire-simd/Cargo.toml`、`crates/wire-simd/src/lib.rs` 和 `crates/wire-simd/src/state.rs`。提交后这些文件由 tracked-source 策略自动收集。格式与严格检查可以独立执行 `cargo fmt --manifest-path crates/wire-simd/Cargo.toml -- --check` 和 `cargo clippy -p wire-simd --all-targets --locked -- -D warnings`，实际语义回归仍从根 crate 的架构测试执行。
