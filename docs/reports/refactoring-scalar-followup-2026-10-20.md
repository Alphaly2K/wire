# 剩余标量入口补齐

基线 `47447ed`。五项既有入口汇入所属契约：EncodePointer/DecodePointer、SetThreadStackGuarantee、AreFileApisANSI、GetLogicalProcessorInformationEx。删除重复 ABI/arity/DWORD 和名称分发，累计 **856 exports**，不增加 Windows 能力范围。

encoding 保留架构全宽 identity 与原 PE32 note；stack guarantee 在 handle 解析前处理指针，保留零查询/非零 error 50；file ANSI profile 保持 Win64 返回 1、PE32 ABI-only。processor topology 的 record codec 归一后与原函数一致，relation DWORD、容量/输出/LastError 顺序保持。公开 system/file facade 只接受各自既有 PE32 typed 项。

release 双架构整合回归 Win64 265、PE32 244，catalog 一致性 1、fmt、release lib/bin strict Clippy 与 build 通过。沙箱内 Win64 测试出现 SIGILL 后，保留原日志并在沙箱外重跑相同正确性检查。本批没有修改执行器、IR 或缓存，不跑性能和游戏/native 矩阵。

完整日志、源码与 executable SHA 记录在 `wire-out/refactoring-2026-10-14/r2-scalar-followup-verification.json`，不复制二进制或游戏目录。
