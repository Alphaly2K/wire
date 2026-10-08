# 原生 Unity Mono：独立运行时实测（2026-10-09）

## 当前出口

候选 Unity Mono `75711b4363fa58f05f953b3f95400d4d61c0ca35`、Unity BDWGC
`1113fefc4ba0e895767ac780d84e9e456fd624db` 和 libatomic_ops
`7e44a3b8dcf457207d6394e4e52e327189155e4f` 已在 Apple M4/macOS ARM64
独立构建。原创 IL 在关闭 JIT、拒绝运行时可执行映射的运行时上通过五组测试，
包括正常 VM 清理退出。尚未接入 wire 的游戏后端；不代表游戏性能已提高。

M0 的候选来源、归档哈希和实际编入清单已形成；精确游戏二进制匹配、完整
Unity 扩展 ABI 和最终分发清单仍待验收。M1 的基础执行/GC/类型化宿主回调
原型已通过。M2 的真实 guest 内存、权限、roots、调度和跨架构 callback 仍未完成。

## 验证结果

`python3 tools/mono/probe.py all` 通过：

| 测试 | 实际覆盖 |
| --- | --- |
| core | 算术/数组 checksum、引用与值类型泛型、反射 Invoke、闭包委托、异常捕获、分配与强制 GC、顶层异常回传 |
| interop | double/int InternalCall、结构体 ref/out、宿主重入 IL、异常以返回状态传递、P/Invoke 按值结构体返回、double/int 反向委托 |
| threads | 两条 pthread 的独立 MonoThread/TLS、每线程 100 次嵌套回调、强制 GC、Sleep 和正常 detach/join |
| gc | Unity BDWGC 增量开关、pinned strong/weak handle 存活、释放 strong 后回收、弱引用清除、finalizer 恰好一次 |
| layout | object header 16、array length offset 24/data offset 32、unbox offset 16、string length offset 16/UTF16 offset 20 |

测试显式使用 Unity 的 pointer-sized `MonoGCHandle` 和 `_v2` API。
旧 32-bit handle API 会截断这个 BDWGC 后端的地址，不能作为 bridge 的句柄契约。
弱引用测试在单独的已退出线程中创建对象，避免保守 GC 将遗留栈字误认作永久 root。
布局只证明候选宿主与样本已知基础偏移相符，未证明 vtable/class/metadata 全量兼容。

五个保护层自测分别触发 RX mmap、MAP_JIT、RX mprotect、vm_protect、
mach_vm_protect，均被拒绝并返回 86。上述正确性测试在同一保护层下正常退出，
无可执行申请；另有 Mono 内部的编译期 mapping 拒绝保护。
这覆盖本轮使用到的无动态 ARM64 代码路径，不宣称全 API/全部签名或 iOS 已通过。

输出位于 `wire-out/native-mono-m1/`：
`probe/results.json`、逐测试日志、`probe/build-manifest.json`、
`runtime-configuration.json`、`build-audit.json` 和 `native-exports.txt`。
本轮清单包含 304 个实际链接对象、668 个源码/头文件编译依赖，0 个未匹配对象；
未发现 GPL atomic stack/malloc 设施进入这些依赖。
Mono core/eglib、Unity BDWGC 和所需 atomic 核心头文件为当前运行时选入范围；
Mono 的额外 native class-library helper、BTLS、SGen 和游戏资源未被打包。

另在 `wire-out/native-mono-m1-repro/` 从锁定归档重新解包、应用补丁、
重建本地 GNU 工具、两套 Mono 和所有 fixture，五组运行及清单检查全部通过。
该复建曾发现遗漏的 mono/culture 链接依赖，已补入脚本；不依赖首个目录中
此前 whole-tree make 留下的缓存对象。可重复构建不等于逐字节产物一致，
AOT ID、绝对调试路径和本机 SDK 仍需要额外规范化。

## 无 JIT 方式

开发工具和执行运行时各自构建。开发工具使用同一候选源码，在构建阶段执行
`--aot=interp` 生成 mscorlib 的 200 个支持 wrappers 及有限静态 trampoline 池。
原创 fixture 的编译数为 `0/0`，不编译其普通托管方法体。
运行时使用 `--enable-minimal=jit`，初始化选择 `MONO_AOT_MODE_INTERP`，
从预先构建的库取得跳板；运行时申请 executable/JIT flags 立即中止。

`MONO_AOT_MODE_INTERP_ONLY` 本身不能满足无 JIT，因为它不设置 aot-only 与
静态跳板选择。保护补丁还去掉了上游为普通 RW 分配执行的 RWX/MAP_JIT 探测。
静态支持文件通过 dyld 装载；未来发布需作为项目构建产物签名，不能在设备上
临时生成后把它当作普通数据。测试使用本地 Unity 2022 SDK 的编译器和 corelib
作为只读 bootstrap，实际执行运行时是此次构建物；未替换成 SDK runtime。

## 已修复的构建与运行问题

补丁均保存在 `tools/mono/patches/`，完整归档哈希固定，可重复应用：

1. ARM64 C 编译使用现代 SDK 的 objc_super.super_class 字段。
2. 局部处理旧 Mach-O section API 的 SDK deprecation，保持原地址行为。
3. OOP 调试读取接口增加真实 unsigned callback adapter。
4. Mono mapping 路径拒绝运行时 executable/JIT flags；禁用 RWX hardening 探测。
5. C99 bool 在 signature token paste 中展开为 _Bool 时保留原类型语义。
6. BDWGC alloc-lock 回调使用真实 void* 参数与显式 MonoDomain 转换。
7. Burst 调试接口使用符合其 opaque buffer 签名的 adapter。
8. interp-only 支持文件也生成预编译 specific/rgctx/IMT 跳板池，解决初始化池容量为零。
9. Darwin 上禁止 pthread GC redirection，由 Mono 显式管理 GC thread attach/detach，
   修复正常 VM 清理时重复 unregister 导致的 native crash。

上游全仓 make 还会编译强依赖 SGen 的 unit-test library，当前脚本仅顺序构建
实际 embedding runtime 组件，而不把未构建的 SGen 当作依赖补入。
GNU autoconf/automake/libtool 安装在本地输出目录，不改系统包或 Unity SDK。

## 后续准入

后端继续保持禁用。下一步验证 guest 寄存器/栈中的引用 roots、跨架构 typed
gateway、两线程 owner 调度，以及 unsafe IL 的受检访存；尤其是 ldind/stind、
cpblk/initblk、readonly/NOACCESS/guard、跨页和释放后访问。
现有 Mono interpreter 的这些指令会直接解引用指针，只是 NULL 检查不能满足
wire 的 checked memory。未完成这部分前，不把 guest 整数直接作为宿主指针。

工作仍限定在 Mono 专用工具、测试、补丁和文档；未修改 SSE/CPU/Memory 主文件。
所有宿主 API 功能测试都属于 M1；不能将它们写成真实 guest↔Unity 闭环已完成。
