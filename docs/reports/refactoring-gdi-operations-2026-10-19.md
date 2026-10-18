# GDI 对象、像素与字体契约候选

状态：31 项结构迁移已接入，完整验证进行中；尚未增加已验收计数。直接基线为 PE32 分发模式冻结 `929f495f386ef76931e75b5cc6dbfd7dc855083e7c47e8298fbc552c2a182558`。对话框、菜单和分发候选的性能成本继续独立保留。

迁移前提交三项 PE32 独立基线和一项 Win64 缺席基线，在旧运行时上与已有 GDI/DC 检查共 13 项通过。覆盖 BITMAP 输出覆盖返回地址、跨页失败的原字节与 frame、颜色/BOOL 错误和 error120 trace、错 provider 零清栈、DIB handle 错误优先于 flags/input 以及可读 BITMAPINFO 诊断。测试输入和期望值独立于新 catalog。旧 executable SHA 与冻结仍一致，日志 `r2-gdi-operation-independent-baseline.txt`。

新增唯一 catalog 和纯图形 GdiOperation 枚举；31 项均 PE32 typed、Win64 absent。删除 30 条旧手写 ABI，逐项清栈数相同；GetObjectType 原用 Wine 10.0 `gdi32.spec:415` 的 stdcall(long)，新签名继续一槽。GDI 适配层改为只借 Graphics 与错误槽，删除旧 DLL/名称检测列表；核心、DIB 和 font 三个模块也直接匹配操作。

位图、DIB、像素和字体处理顺序保持，font 优先于 bitmap，错误诊断与输出/返回槽提交位置保持。三份核心源文件核对为原文本加明确的名称→枚举、字符串条件→有限枚举条件替换和 rustfmt，计算、访存、资源和错误处理体没有其它改动。graphics 不导入 Win32 catalog；没有扩展 Win64 GDI、native graphics、Mono 或 IL2CPP 能力。

完整 release **623 passed / 7 ignored**、debug Win32/AMD64/FPU **469**、显式 native GPU **7**、fmt、strict all-target/all-feature Clippy 与 release build 全部通过。冻结 `r2-gdi-operation-wire/wire`，SHA256 `a95c0105e765e8ce0c644cfa717ff5737f10c3adcfa3fd016db536f066a8b90a`，6,018,680 bytes，较直接基线减少 6,240 bytes，满足本批 +24KiB 预算。source manifest 与 patch 同目录保存，测试与构建日志使用 `r2-gdi-operation-` 前缀。

正在串行执行 before=已验收消息、previous=直接分发、after=本候选的 GDI 150M raw/IR 三对，随后完成累计宽矩阵与游戏前缀。尚未把完整正确性视为性能验收。架构入口见 [GDI 对象边界](../architecture/gdi-operations.md)。
