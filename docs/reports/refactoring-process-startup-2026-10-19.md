# 双架构进程启动信息契约（2026-10-19）

状态：结构、完整正确性和启动功能通过；随后在 [Unity 渲染结束后的累计复核](refactoring-post-unity-performance-2026-10-19.md)中通过性能验收，当前累计 706 项。直接运行时基线为资源迁移 `ab96bfc`，独立旧行为基线先提交于 `99d280d`。以下保留当时暂停长计时的过程。

五项 GetCommandLineA/W、GetStartupInfoA/W、IsProcessorFeaturePresent 由 StartupApi 唯一声明 provider、参数和执行策略，删除 PE32 重复 ABI、Win64 arity/DWORD 行和旧名称处理体。`win32::{x86,x64}::process_startup` 只借内存和既有命令行指针。PE32 kernel32 与 Win64 kernel32/kernelbase 的范围、命令存储、结构布局、feature 集合及返回帧差异保持，详见[调用边界](../architecture/process-startup.md)。

两项 PE32、两项 Win64 独立测试在迁移前后通过：命令指针不解码/分配，结构仅写 68/104 字节，范围不足不写输出，LastError/trace、DWORD 高位、错误 provider、fresh-provider GetProcAddress 以及输出覆盖返回槽时的不同返回行为。原 helper 与架构 facade 继续存在，没有修改 CPU/IR/cache、SIMD 或 native 代码。

完整验证 `r2-process-startup-validation-*`：release **640 passed / 7 ignored**、debug **486 passed**（Win32 235、AMD64 240、FPU 9、日志单元 2）、显式 native **7 passed**、工具 **28 passed**；fmt、strict all-target/all-feature Clippy、SIMD crate 静态检查及 release build 通过。

冻结 `r2-process-startup-wire/wire`：SHA256 `8a8c13bd0749fc71476e308fcd4490871a8ba6ea313df2333e8354ea4cbb8ae9`，**6,040,984 bytes**，较资源冻结增加 **19,024 bytes**，在 +24KiB 范围内。299 个 src Rust 文件、target/frozen executable 和 manifest 核对一致；仅两根分发器、三个模块声明和三个新启动契约/适配器文件改变，证明 `r2-process-startup-source-proof.json`。保留此二进制成本，未枚举构建配置追逐布局收益。

原创 `gateway-process-startup` 每轮消费两个命令指针、两个 STARTUPINFO 大小与 feature 6/10/12 的回复：n=max(1,work/31)，EAX=140n、retired=31n+11、API=7n。迁移前冻结 raw/IR n=100 校验通过，日志 `r2-process-startup-baseline-fixture*`。生成器当前合法 case 为 60 项，延续未知名称提前拒绝策略。

PE32 八种输入的 raw/IR 共 **16 条 12K 功能协议、96 次执行**通过，涵盖启动、资源、进程上下文、消息、GDI、整数、SSE、标量 FP；六对完整启动 API trace 保持调用内容与顺序，频率 footer 只排序比较。AMD64 四条整数/访存/SSE/线程 ID 协议 **24 次执行**也通过。证据 `r2-process-startup-functional-{pe32,amd64}`，这些小样本用于结果/调用回归。

一对 Alice 200M 功能前缀均到相同预算出口、hidden 314×211、Draw/Present 0/0，证据 `r2-process-startup-functional-alice`。复用现有游戏输入和 runtime，临时解释器软链接已删除。本轮不把单对启动或短协议的时间/RSS 差值作为性能准入；继续保留后台 Unity/WindowServer 干扰及同哈希漂移的既有调查结论。
