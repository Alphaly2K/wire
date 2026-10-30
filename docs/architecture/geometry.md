# 矩形与窗口边框

新 Wasm 入口在 PE32/AMD64 的 user32 接通 SetRect、SetRectEmpty、CopyRect、EqualRect、InflateRect、OffsetRect、PtInRect、IsRectEmpty、IntersectRect、UnionRect、AdjustWindowRect、AdjustWindowRectEx 和 AdjustWindowRectExForDpi。新入口不接受旧适配器中的 kernel32/kernelbase 别名。

RECT 的四个字段均为 signed LONG，偏移与膨胀按 32 位回绕。空矩形包含零面积和反向边界；无交集和两个空矩形的并集输出全零，并返回 FALSE。所有源先完整复制，再写目的，因此允许目的与源完全或部分重叠。NULL 矩形指针通常返回 FALSE；IsRectEmpty(NULL) 返回 TRUE，均保持 LastError。

PtInRect 的 POINT 是按值八字节结构：PE32 为两个连续栈 word，AMD64 为一个 packed 64-bit carrier；边界含左上、不含右下。共享契约的 Uint64 参数与清栈字节数由两种 ABI 的通用调用帧处理。

窗口边框复用同一 96-DPI 非客户区数值核心，与默认 NCCALCSIZE 的 border/caption/menu/edge 尺寸一致。AdjustWindowRectExForDpi 当前仅支持 96 DPI，其余值返回 FALSE 和错误 50，保持原 RECT。一般矩形计算与成功边框调整保持 LastError，不采样时钟。

输入、输出和原返回帧的访存故障保留 CPU、work、退休数及完整输出，修复后可重试。成功前保存参数和返回帧；输出覆盖原返回槽仍按保存位置返回。每次完成计一 work、零客体退休，FLAGS、FP 与 SIMD 保持。

数值核心位于 `src/win32/user/geometry.rs`，原 Wine/Hangover 派生算法及版权继续保留。旧 PE32/Win64 内存适配器作为迁移参考；新入口只复用纯数值函数。
