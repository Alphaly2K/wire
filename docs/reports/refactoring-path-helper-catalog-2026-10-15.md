# SHLWAPI/imagehlp 路径辅助契约

行为与性能基线为 `240e4ac`，独立行为回归和原创 gateway fixture 为 `3bb016f`。本批迁移路径辅助的身份、ABI 和绑定路由；不改变路径算法、filesystem/handle owner、ISA、FLAGS、IR v3、翻译缓存或 Mono。

## 范围

独立 PathApi 家族包含 13 个 export：PathFileExistsA/W、PathIsDirectoryA/W、PathRemoveFileSpecA/W、PathRenameExtensionA/W、PathAddExtensionA/W、PathUnquoteSpacesA/W、MakeSureDirectoryPathExists。PE32 全部 Typed，前十二项仅 shlwapi，最后一项仅 imagehlp；Win64 全部 Absent。文件家族仍单独统计为 45 个 export。

指针占一个 PE32 stdcall slot，扩展名函数两个 slot。SHLWAPI 签名与 pinned Wine 10.0 spec 核对；imagehlp 保留原来的单 slot 手写 ABI，本地没有该库的 Wine spec。Unquote 的返回元数据标记 Void，adapter 继续写入零 EAX。删除 PE32 重复手写 ABI、独立支持名称列表和 handler 内的字符串操作选择；Wine spec 继续为其它 legacy API 提供 ABI。

主 gateway 直接调用绑定 PathApi；公开 file_api facade 继续限制为原来的小写 provider，GetFileType 仍由主分发器处理。内部互斥 Core/Helper ID 取代 Option<FileApi> 与字符串的组合；A/W、IsDirectory 与 Add/Rename 由 ID 决定。原 handler 顺序、算法与 trace 保持，Hangover/Wine LGPL 来源、版权和 pinned commit 注释原样保留。

保留 NULL 短路、checked 读写、CP932 lead/trail byte、UTF-16 长度、root/UNC/extended prefix/volume 与 ANSI/Wide 差异。扩展名长度超限先返回 206，再考虑输出权限；输出 fault 返回零/LastError=87。Unquote 成功返回零，trace 记录原字符串；MakeSureDirectoryPathExists 按末尾 separator 决定创建目录本身还是文件 parent。现有成功路径、错误码和可能的输出提交行为未统一改写。

## 正确性与候选冻结

四项独立新增回归先在旧实现通过并提交，覆盖十三项清栈/NULL/LastError/TEB、错误 DLL、只读输出、CP932 与 UTF-16 长度、真实文件/目录查询和目录 suffix。Win64 对四个 provider 的十三项导入均拒绝，CPU/API 计数保持不变。迁移后所属路径回归 12 项通过；完整 release 421 passed / 7 ignored、debug Win32/AMD64/FPU/Unicorn 306、显式 native GPU/Metal 7、fmt、strict all-target/all-feature Clippy 与 release build 均通过；工具 12 项在 fixture 基线提交通过。

候选冻结为 `wire-out/refactoring-2026-10-14/r2-path-helper-bound-wire/wire`，SHA256 `bbe27f7f6ee53b9e0cc2317f4dc475f7adc58cc3934ab90fdb2d34e43609389b`，5,833,832 bytes。before 为 `r2-file-search-cold-route-wire/wire`，SHA256 `927662997fbc71b5102113ab1b5ca2f8f9f5c86d723010d177d7f56be659438f`。两版都通过新增 NULL PathRemoveFileSpecA 的 12K raw/IR smoke，独立验证零返回、retired 和 API count；fixture 没有 host 文件或时钟操作，默认 case 集合不变。

## 首版性能验收与拒绝

计时串行执行，与 Cargo、sample、runtime diagnostics 分开。PE32 使用 utility QoS clamp，API trace 保留格式化/计数并写 null device。初筛的 SSE IR 三对为 +15.64%/+16.23%/+17.85%，raw 中位数 −0.24%，因此停止首轮矩阵并保留 KeyboardInterrupt 与部分结果，先调查 SSE。700M 三对的 SSE IR 为 +31.99%/−4.61%/−1.43%，两版均校验，before CPU 本身从 5.17s 到 9.07s 波动；追加相同二进制 control 标签的长样本，完成前不判定通过。

静态反汇编显示 Descriptor::execute 的 SIMD 节由 1734 条变为 1666 条，packed integer 部分的寄存器分配/生成指令发生变化，调用列表一致。原始反汇编、逐块差异和摘要保存在 `r2-path-helper-{before,after}-simd32.asm`、`r2-path-helper-simd32-code-diff.txt` 与 `r2-path-helper-simd32-code-summary.json`；读取的是显式 `__wire_simd32` 节。没有仅凭测量噪声排除编译代码变化的影响。

完成补样后的 39 个 case/path 均校验 work/retired/返回值/API count。PE32 NULL 路径辅助 IR/raw 的 CPU 中位数为 −8.85%/−1.81%。SSE 的相同 work 两组共六对合并保存；追加 control 的一组三对，中位数候选 +0.96%、相同基线 control +0.22%。位操作 700M 候选 +2.04%，control +5.83%。所有初筛、补样、control 与中断轮次保留。

Win64 thread-id、metadata、search 初筛分别 +3.82%/+3.37%/+17.82%，因此继续 700M 三标签调查。thread-id 的三对候选均回退超过 5%（+6.94%/+6.12%/+6.19%，中位数 +6.94%）；control 中位数 +2.69%，单对也有波动。按持续回退门槛拒绝首版候选，停止剩余长样本，保留已完成数据与 KeyboardInterrupt。未将首版运行时代码提交 main；下一步独立验证热网关/通用网关编译边界，再在新基线上继续路径契约。
