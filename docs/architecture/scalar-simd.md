# 精确标量 SSE 边界

编译隔离背景见[编译隔离报告](../reports/refactoring-scalar-codegen-2026-10-19.md)。2026-10-21 的受检正常值加速与验证见[执行器优化](../reports/executor-optimization-2026-10-21.md)。

`crates/wire-simd` 保存精确标量 SSE 语义，依赖 iced 的 Mnemonic 和锁定版本的 rustc_apfloat。库只接收输入位模式、精度/操作以及 MXCSR/RFLAGS 两字段状态，不接收 CPU、guest memory、IR、Win32 或宿主对象。根 `simd.rs` 重导出这些类型并继续拥有原整数向量 helper；x87 仍在架构执行器的原实现中。

封闭的 `Scalar` / `ConvertTo` trait 只允许既有 Single/Double 格式，静态调用进入内部库的具体函数。入口禁止跨库内联，私有泛型算法内联到入口，库自身固定一个 release 编译单元；根 crate 与其他依赖的 profile 不变。库没有动态分派、生成 guest 代码、常量输入捷径或宿主浮点近似。

ARM64 上，正常有限单精度数的加、减、乘可使用宿主 IEEE 算术。入口逐次确认 guest round-to-nearest，以及 host FPCR 的舍入、FZ 和异常控制；读写 guest MXCSR，仅读取 host 控制。输入或结果为零、非正常数、NaN、无穷，或控制不适用时回到 APFloat。两个 binary32 有效数相乘的精确值能装入 binary64；加减指数差不超过 24 时也能精确装入，超过时正常 binary32 结果必定不精确，据此保留 precision sticky 与未掩码 #XM。precision 已 sticky 且 masked 时可省去再次判断精确性。其它架构、Double、除法和特殊值继续使用原精确实现。

调用者先完成所有源访存，再建立运算事务。库执行既有舍入、DAZ/FTZ、NaN、sticky exception 与掩码规则，返回结果或原 #XM 错误；调用者保留原本的目标、FLAGS、PC 与退休提交边界。源访存失败不触发运算，未掩码 SIMD 异常仍只提交允许的 MXCSR 状态。packed/raw 的逐 lane 异常累积及 scalar IR 的目的寄存器宽度仍由调用者协调。

新增普通 Win32 契约不需要修改此库。改变这里的算法、精度或编译配置时，继续执行真实 FPU/Unicorn、双架构 scalar IR/checked 对照、fault/guard/异常提交测试及浮点性能样本；不能将隔离视为放宽 ISA 验证的理由。

冻结工具自动收集 tracked 文件；新增的 `crates/wire-simd/src/fast.rs` 在提交前须显式 `--include`。格式与严格检查可以独立执行 `cargo fmt --manifest-path crates/wire-simd/Cargo.toml -- --check` 和 `cargo clippy -p wire-simd --all-targets --locked -- -D warnings`。根 crate 执行双架构语义测试；`cargo test --release --locked -p wire-simd` 另以 10 万组输入、三种运算和五种 MXCSR 配置对照原 APFloat 算法，并检查 host FPCR 保持。
