# 精确标量 SSE 边界

`crates/wire-simd` 保存精确标量 SSE 语义，只接收输入位模式、具体操作和 MXCSR/RFLAGS 状态。它不读取客体代码、操作数描述或 CPU 执行循环，可编译为核心 Wasm 的专用数值函数。

`Scalar` 与 `ConvertTo` 只允许 Single/Double 格式。实现处理舍入、DAZ/FTZ、NaN 选择、sticky exception 和异常掩码；特殊值和不满足快速路径条件的输入使用 APFloat。数值快速路径必须保持相同位模式与异常结果，不修改宿主浮点控制。

调用者先完成所有源访存，再调用数值操作。源访存失败不触发运算；未掩码 SIMD 异常只提交允许的 MXCSR 状态，保留目的寄存器、FLAGS、PC 与退役边界。packed 运算由调用者维护逐 lane 异常累积，并在所有 lane 成功后提交目的寄存器。

源文件位于 `crates/wire-simd/src/`，已有确定性数值对照与 FP 控制保持测试继续作为该组件的验证资产。新运行时的操作数与提交适配不能改变这些契约。
