# 环境变量契约与 Win64 状态归属（2026-10-19）

状态：结构、完整正确性与启动功能通过；随后在 [Unity 渲染结束后的累计复核](refactoring-post-unity-performance-2026-10-19.md)中通过性能验收，当前累计 706 项。直接基线为启动信息 `a9b9d36`，独立旧行为检查先提交于 `2ee3a02`。九项环境 export 汇入 EnvironmentApi；以下保留当时暂停长计时的过程。

删除 PE32 八项重复 ABI、Win64 arity/DWORD 行及两端名称处理体。PE32 六项 typed、三项 ABI-only；Win64 六项 typed、三项 absent。PE32 gateway 继续调用原 scratch allocator，领域 adapter 只接收内存和分配地址。Win64 变量与环境块集合归入私有 Environment owner，原 page allocator 和 mapping 服务保持，见[环境边界](../architecture/environment.md)。

两端各两项新增测试，加上已有 Win64 大小写/长度/guest 隔离检查，在迁移前后通过。覆盖 PE32 忽略无效名称/输出指针、LastError 保留、16 字节 scratch 递增、任意指针 free、三项已知 ABI 的 error 120，以及 scratch 耗尽后内部错误先于 TEB/计数提交；Win64 覆盖无效名称、缺失变量、DWORD 容量、失败 set 保留原值、输出故障与 CPU/API count、重复释放和普通 HeapFree 拒绝环境块。早期测试把环境块误当普通 heap allocation、以及 helper 变量作用域错误的日志保留于 `r2-environment-baseline*`；修正测试后以 `*-v4.txt` 固定旧行为，没有调整运行时迎合预期。

完整检查 `r2-environment-validation-*`：release **644 passed / 7 ignored**、debug **490 passed**（Win32 237、AMD64 242、FPU 9、日志单元 2）、显式 native **7 passed**、工具 **28 passed**；fmt、strict all-target/all-feature Clippy、SIMD crate 静态检查及 release build 通过。

冻结 `r2-environment-wire/wire`：SHA256 `eccc6142a980c984d96be14f9b1fe983762527249447ae05a5b6e8e9d83d5e44`，**6,062,984 bytes**，较启动信息冻结增加 **22,000 bytes**，在 +24KiB 范围内。302 个 src Rust 文件、target/frozen executable 与 manifest 核对一致。源码只改变两根分发器、catalog/x86 模块声明及三个新环境文件；CPU/IR/cache、SIMD、host/native 未改。证明 `r2-environment-source-proof.json`。

原创 `gateway-environment` 只在循环前分配两个 block，消费其零内容；循环覆盖其余七项查询/free/ABI-only export。n=max(1,work/28)，EAX=2n、retired=28n+13、API=7n+2。旧启动信息冻结 n=100 的 raw/IR 均通过：EAX 200、retired 2813、API 702，证据 `r2-environment-baseline-fixture*`。当前 PE32 fixture 合法 case 为 61 项，未知名称继续提前拒绝。

PE32 八种输入、16 条 raw/IR 12K 协议 **96 次执行**，AMD64 整数/访存/SSE/线程 ID 四条协议 **24 次执行**全部校验。六对完整环境 API trace 的调用内容/顺序保持，频率 footer 只排序比较，原始 SHA 保留；证据 `r2-environment-functional-{pe32,amd64}`。小样本用于执行、绑定和调用回归，未作吞吐量准入。

一对 Alice 200M 功能前缀均到相同预算出口，hidden 314×211、Draw/Present 0/0，证据 `r2-environment-functional-alice`。复用既有 runtime，解释器软链接退出即删除，没有复制/硬链接游戏素材。不根据单对启动的 CPU/wall/RSS 差值作性能判断，后台 Unity/WindowServer 干扰及同哈希漂移仍单列保留。
