# PE32 GDI+ 状态与调用边界

`gdiplus::GdiPlus` 私有拥有 matrix、allocation、startup token 集合及 token 游标。公开 `gdiplus::dispatch` 保留兼容签名，委托 `gdiplus/calls.rs`；普通处理体只借 GdiPlus、GuestMem、GuestHeap 和 process heap ID，不再借 ApiState。

Wine 的矩阵乘法、逆矩阵阈值、浮点运算顺序、整数舍入及状态映射保持原算法。创建矩阵仍先预检输出，再分配、写矩阵、写指针，写入失败释放分配，最后登记对象；startup 仍先登记 token 再写输出。对象释放及无效指针策略保持。

本批收窄资源依赖，ABI 仍由原 Wine gdiplus.spec 解析，未知签名和未移植函数范围保持。没有新增 GDI+ 功能或 catalog export。

基线 `2760af1`；原处理体、数学/codec、分配 helper 和 ABI parser 归一核对一致。release GDI+ 4、完整 Win32 255、fmt/release lib/bin strict Clippy/build 通过。普通 owner 提取不跑性能或游戏矩阵。证据：`wire-out/refactoring-2026-10-14/r3-gdiplus32-{source-checks,verification}.json`。
