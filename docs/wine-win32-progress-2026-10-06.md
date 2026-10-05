# Wine Win32 逻辑移植与游戏插件推进（2026-10-06）

后续 VMM 已解除下文记录的 dirlist 内存阻塞，dirlist/extNagano 完成初始化；
随后菜单/基础GDI/常用USER32功能组已解除menu/win32dialog等插件边界，并创建真实游戏主窗口；
当前推进与新阻塞见 [功能组记录](common-win32-progress-2026-10-06.md)。
本页的102项是此前VMM检查点验证，后续Win32改为参考用户指定的Hangover。
最新结果见 [VMM推进记录](vmm-progress-2026-10-06.md)，下一阶段见 [图形渲染设计](graphics-rendering-design.md)。
以下保留 Wine 移植阶段的来源、实现与95项验证记录。

按用户“Win32 API 部分逻辑可以照搬 Wine”的授权，逐文件核查许可后直接移植已需要的逻辑。
基线为 `53c6190` 的真实 DLL/静态 TLS 装载器。本轮越过 layerExDraw 的 GDI+ 依赖缺失，
继续装载多个真实插件，该阶段阻塞为 dirlist.dll 的 `VirtualAlloc` 内存保留路径。
仍无可见游戏画面或持续主循环，**M2 未验收**。解释器为主路径，本轮没有性能优化结论。

## 许可与来源

固定来源为 [Wine 10.0](https://github.com/wine-mirror/wine/tree/b073859675060c9211fcbccfd90e4e87520dc2c2)，
commit `b073859675060c9211fcbccfd90e4e87520dc2c2`，annotated tag object
`2e9cb99369e5ead9d7fe9836dcd43c05e42890bc`。

本轮使用的 `dlls/gdiplus/matrix.c`、`gdiplus.c`、`gdiplus_private.h` 和 `dlls/user32/win.c`
文件头均为 LGPL-2.1-or-later，允许选择 LGPL 2.1，与 wire 的 LGPL-2.1-only 合并兼容。
GDI+ 文件版权为 Copyright (C) 2007 Google (Evan Stade)，user32 为
Copyright 1993, 1994 Alexandre Julliard。`gdiplus.spec` 没有单文件版权头，保留 Wine 的项目许可。
此结论针对实际使用文件；后续新文件仍逐文件核查。

原始 C 文件、带原文件头的窗口枚举/数值 helper 摘录、未修改 spec、COPYING.LIB 与 SHA-256
保存在 [来源目录](../third_party/wine-10.0/README.md)，Rust 移植文件保留原版权和许可头。
[THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES) 已登记来源和改写范围。
不构建这些 C 快照；spec 作为 ABI 数据嵌入。未引入整个 Wine、Wine server 或其平台驱动。

## 已移植的语义与 guest 适配

`src/gdiplus.rs` 从上游 spec 建立 named stdcall 导出 ABI，包括 float 的 DWORD 参数、
按值 GUID 的 16B 参数和实际栈清理长度。没有已知 ABI 的上游 stub 不公开为可调用导出。
layerExDraw 的 140 个静态 GDI+ 导入可以正确绑定；绑定 ABI 不表示该函数功能已实现。

- GdiplusStartup 支持版本 1/2，并分配真实唯一 token；其他版本返回 UnsupportedGdiplusVersion。
  未移植的 notification hooks 显式返回 NotImplemented，缺少要求的 output 返回 InvalidParameter。
  GdiplusShutdown 清理 token；GdipAlloc 零填充，GdipFree 释放实际 guest heap 块。
- 完整矩阵创建/clone/delete/get/set、prepend/append multiply、translate/scale/shear/rotate、
  inverse/invertibility、identity/equality、矩形到平行四边形映射、点/向量 F32/I32 变换。
  保留 Wine 运算顺序、对角逆矩阵特殊分支与一般行列式的双精度 `fabs(det) >= 1e-5` 比较。
  equality 按位比较，保留 -0/NaN payload 差别。整数点使用 Wine `floorf(x + 0.5)` 的舍入流程，
  超范围/NaN 转换明确得到 i32::MIN，避免依赖 C 未定义行为。
- 矩阵是 guest heap 内真实 24B F32 数据，并记录对象类型/生命周期；没有把 host 指针返回 guest。
  guest 输入/整段输出检查、count/长度溢出检查继续使用 GuestMem，跨未提交页失败不部分写数组。
- 未移植的绘图、brush/path/image/font/codec 等 GDI+ 导出返回 **GpStatus::NotImplemented=6**，
  标记 `[UNIMPLEMENTED]`，不修改输出。禁止用默认零值伪造 GpStatus::Ok。
- `EnumWindows` / `EnumThreadWindows` 移植 Wine 的窗口列表快照、回调前跳过已销毁窗口、
  callback FALSE 立即停止流程。排除 child/message-only 窗口，按实际 tid 过滤；空列表返回 TRUE。
  复用 wire guest callback 边界，保留 callback LastError 以及退出/非局部转移的新 CPU 现场。
  `GetWindowThreadProcessId` 返回当前逻辑窗口的真实 tid 和模拟进程 PID4242。

窗口枚举只遍历 wire 当前进程的逻辑窗口，顺序为创建顺序反序，未实现完整桌面/z-order。
实际绘图、字体注册/渲染、图像编解码与窗口呈现仍未实现。
新增字体资源 API 目前只补 ABI，明确失败并记录 LastError=120/`[UNIMPLEMENTED]`；
未照搬 Wine 字体删除函数中的假成功 stub，也未制造字体句柄。

## 真实游戏推进与当前阻塞

游戏使用原始完整目录：
`/Users/alphaly/Downloads/王様恋愛【体験版】/王様恋愛【体験版】.exe`。
游戏资产及生成 IR 不加入仓库。

1. layerExDraw.dll 映射到 `0x50050000`，guest DllMain `0x500764aa` 实际执行。
   `GdiplusStartup(token=0x50097928, input=0x500928f8, output=0)` 返回 Ok。
2. varfile.dll（`0x500b0000`）、KaichoTrans.dll（`0x500d0000`）、addFont.dll（`0x500f0000`）、
   scriptsEx.dll（`0x50110000`）均继续真实装载并执行初始化。
   此结果只表示到达这些插件的初始化路径，不表示其绘图/字体功能已运行成功。
3. 下一个插件 dirlist.dll 映射到 `0x50130000`，guest DllMain `0x50131000` 已进入。
   CRT 自定义分配器调用 `VirtualAlloc(NULL, 0x400000, MEM_RESERVE=0x2000, PAGE_NOACCESS=1)`。
   当前 API 未实现，返回 NULL；随后在 `EIP=0x5013346d` 读取 `0x000003f8` 发生 guest fault。
   更早的 VirtualQuery 同样未实现；不是 IR 解码故障。

最终 IR/raw 两条路径分别退休 **288,808,361 / 288,811,080** 条指令，
API 调用 **832,423 / 832,463** 次。均读取 **37 段、合计 1,130,713B**。
全部 GPR、EIP、EFLAGS 终态相同，停在同一 VirtualAlloc 阻塞。
计时轮询/调度次数仍有差异，本次单对初始化重放用于功能对照，不能当持续负载性能测量。

日志位于忽略的 `wire-out/game-gdiplus/`：`first`、`font-abi`、`font-ex-abi`、`window-enum`、
`final-ir` / `final-raw`。最终目录内保存 `api.log` 和 `report.txt`。

```sh
cargo build --release
target/release/wire run '/Users/alphaly/Downloads/王様恋愛【体験版】/王様恋愛【体験版】.exe' \
  --ir wire-out/game-v3.wir --max-instr 1000000000 --log-dir wire-out/game-gdiplus/final-ir
```

raw 重放省略 `--ir` 并换日志目录。游戏 DLL 继续用 raw fallback；没有运行时机器码/JIT，
没有修改 FLAGS 删除证明、checked memory、代码页失效、线程/回调边界或 x87 精度规则。

## 验证与下一步

`cargo test --release` **95 项通过**：3 unit、34 CPU differential、5 FPU、11 IR、1 Wasm、41 Win32。
本轮新增 5 项 Win32 回归，覆盖导出/ABI/启动/分配/未实现状态，矩阵顺序/几何/逆变换/生命周期，
奇异矩阵/行列式双精度阈值/-0/检查访问，矩形映射/整数舍入，窗口过滤/销毁跳过/FALSE/LastError/退出控制权。
Clippy release all-targets `-D warnings`、format、diff 检查通过，源码快照 checksum 与来源表匹配。

下一阶段先补通用 guest VMM 的 reserve/commit/free/protection 和 VirtualQuery。
Windows 4KiB 逻辑页应独立于 macOS 16KiB host page；MEM_RESERVE 只占地址空间，
PAGE_NOACCESS 不可变成可读写页。保留空间必须避开 image、heap、DLL、stack、thread/scratch 等区域，
统一 CPU/API fault、权限与代码页失效边界，不能用 reserve 等于 commit 的短路绕过当前阻塞。
然后继续真实插件初始化、GDI/GDI+ 绘图和窗口呈现。进入持续主循环后再扩大样本、重抓 profile。
