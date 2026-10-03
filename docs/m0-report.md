# M0 里程碑报告：PE 装载器 + API 调用日志

日期：2026-10-04。宿主机：macOS 26.5.1 / Apple Silicon (arm64)。Rust 1.98.1。

## 验收对照

| 验收项 | 结果 |
| --- | --- |
| 装载目标 EXE（解析 PE、映射节、重定位、解析 IAT、未实现导出挂统一 stub） | 通过（目标程序映射于首选基址 0x400000，无需重定位；重定位路径已留位） |
| 每个 API 调用记日志（名称、参数摘要、返回值） | 通过（全量 trace + 频率表，wire-out/api.log） |
| 执行流到达入口点 | 通过（对照程序与目标 EXE 均到达） |
| 对照程序能完整跑完（哪怕 stub 全返回假值） | 通过（641 条指令、43 次 API 调用、干净退出码 0） |
| 产出目标 EXE 的完整 import 清单 | 通过（docs/m0-imports.txt：18 个 DLL、346 个函数） |

## 目标程序分析（Kirikiri2 系体验版，2016，MSVC 构建）

- PE32 GUI，.text/.rdata/.data/.rsrc/.reloc 标准布局，入口在 .text（非可写），346 个导入 —— 无加壳迹象。
- 图形相关导入只有 GDI32（23 个：CreateDIBSection/CreateCompatibleDC/GetGlyphOutlineW 等），
  **无 DDRAW/D3D/DSound** —— M3 可走纯 GDI blit → 宿主表面的路径。
- QUARTZ.dll（DirectShow）与 MF/MFPlat（Media Foundation）各少量导入 —— 视频链路，
  按计划本期跳过（黑屏/记录）。
- 目录含 plugin/（krkr 插件机制）—— guest 侧 DLL 装载是 M2+ 工作项，已在计划登记。

## 目标 EXE 执行轨迹（本轮最深处）

MSVC CRT 启动完成（GetProcAddress 动态解析 29 个现代 API 全部经动态 stub 注册成功）
→ 进入用户代码（WinMain → krkr 引擎初始化：引擎级 256KB 堆分配、CP932 区域设置、
命令行/环境处理）→ 首个环境相关初始化失败 → C++ `throw`（RaiseException 0xE06D7363）
→ EH 展开未实现，运行终止。16985 条指令、224 次 API 调用、103 种不同 opcode，
未触发"未支持指令"错误。

## 本里程碑排掉的坑（全部记录在案）

1. **Apple Silicon 16 KiB 宿主页**：按 4 KiB 调 mprotect 返回 EINVAL；提交粒度必须对齐
   宿主页（运行时 sysconf 探测）。iOS 同为 16 KiB 页，此坑早点名有价值。
2. `fetch_sized` 对 8 位立即数错取 4 字节 → 指令流漂移（靠指令级 trace 环定位）。
3. **stdcall arity 表是硬需求**：未知函数按 cdecl 处理会系统性破坏栈平衡；
   当前内置约 400 个常见 API 的 arity，未知者打警告日志。
4. CRT 对假返回值敏感的两个实例：`InitializeCriticalSectionAndSpinCount` 返回 0
   表示失败导致启动中止；`GetEnvironmentStringsW` 返回 NULL 触发 R6009。

## 已登记的简化假设（M2 必须逐条复核）

- 临界区全部 no-op（单线程假设；krkr 为轻线程，但 CreateThread 已在 import 清单中）
- bump heap：无 free/realloc 语义（M2 换真实堆）
- EncodePointer/DecodePointer 恒等编码
- CP932 假区域（GetACP=932、IsDBCSLeadByte 按 932 范围、GetConsoleCP 等）
- 假模块句柄；LoadLibrary 不真正装载 guest DLL
- GetProcAddress 动态注册新 stub（已实现且关键——CRT 与引擎都重度依赖）

## M1 输入

- 解释器 opcode 统计机制（op_stats）已内建，逐次补齐的迭代模式已验证
- 宿主→guest 回调（call_guest）经 CRT `_initterm` 验证可用
- 下一步：Unicorn 差分测试覆盖对照程序 + 目标程序已见的全部 opcode；
  x87 算术与 SSE 子集按统计驱动排期

## M2 输入（按目标日志优先级）

1. RaiseException / SEH 展开（当前首个硬阻断）
2. 真实堆（HeapAlloc 已 15 次，含 256KB 引擎堆）
3. 文件 I/O（游戏将读 data.xp3；CreateFileW/ReadFile/SetFilePointerEx 已在清单）
4. TEB/PEB 补全、线程 API（CreateThread 已在清单）
5. user32 窗口与消息循环、GDI blit 落地

## 复现

```sh
wire run "path/to/王様恋愛【体験版】.exe" --max-instr 50000000
# 全量 trace 与频率表：wire-out/api.log
```
