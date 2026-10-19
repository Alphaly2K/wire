# PE32 资源契约迁移（2026-10-19）

状态：结构、完整正确性与启动功能通过；随后在 [Unity 渲染结束后的累计复核](refactoring-post-unity-performance-2026-10-19.md)中通过性能验收，当前累计 706 项。直接运行时基线为 CLI `c145084`，先于迁移提交的独立行为检查为 `6d42ed2`，fixture 栈输出修正为 `770274d`。以下保留当时暂停长计时的过程。

十项 export 由 `win32::catalog::resources` 声明 provider、参数和执行策略：user32 的 LoadStringA/W，kernel32 的 FindResourceA/W、FindResourceExA/W、LoadResource、SizeofResource、LockResource、FreeResource。删除总分发器的十项重复 ABI 和名称处理体，改用绑定的 ResourceApi ID。`win32::x86::resources` 只借 GuestMem、EXE 基址和错误槽；公开 `resource` 读取 helper 及其调用者保持。Win64 保持 absent，详见[资源边界](../architecture/resources.md)。

四项独立 PE32 测试固定数字/名称键、CP932/UTF-16、默认 image、语言回退、空槽/缺失块、ID 截断、容量与 DBCS 字节截断、部分输出故障、LastError、输出覆盖返回槽、错误 provider/零清栈；一项 Win64 测试固定十项导入及 fresh-provider 动态 lookup 缺口。均在迁移前后通过。首版测试 helper 缺少 mutable borrow 的编译错误保留在 `r2-resources-baseline-win32.txt`，修正后日志为 `*-win32-v2.txt` 与 `*-x64.txt`。

原创 `gateway-resources` 每轮消费全部十项 API 及 W/A 输出。最终 n=max(1,work/57)，retired=57n+11，API=10n，EAX 由独立指针/长度/字符加总计算。旧冻结 n=100 的 raw/IR 均通过：retired 5711、API 1000，IR 计数 5711、CALL_NATIVE 1000，仅终点 INT3 fallback。早期 image 输出设计先触发只读写故障，改成 writable image 后使已安装 image IR 失效；输出最终放在 guest 栈上。早期输出及错误 retired 预期均保留于 `r2-resources-baseline-fixture*`，不计作新运行时回归。

完整验证 `r2-resources-validation-*`：release **636 passed / 7 ignored**、debug **482 passed**（Win32 233、AMD64 238、FPU 9、日志单元 2）、显式 native **7 passed**、工具 **26 passed**；fmt、strict all-target/all-feature Clippy、SIMD crate 静态检查与 release build 通过。原生测试使用既有 graphics/MoltenVK/DXBC/GS 输入，不新增 shader 编译路径。

冻结 `r2-resources-wire/wire`：SHA256 `05c26a5682bb76587817e0ab03e1f8f7369df677f284be719e58a2a8e40ea6b2`，**6,021,960 bytes**，较 CLI 冻结减少 **3,616 bytes**。296 个 src Rust 文件与 manifest/target/frozen executable 核对一致；运行时只改变 api、两个模块声明、两个新资源文件和 Win64 exhaustive absent 分支，证明 `r2-resources-source-proof.json`。CPU/IR/cache、SIMD、host 和 native 源码保持。

八种实际输入的 raw/IR 共 **16 条 12K 功能协议、96 次执行**全部验证 EAX、retired 和适用 API count：资源、控件初始化、paint、消息、GDI、整数、SSE、标量 FP。资源六对完整 API 调用 trace 逐字节/顺序相同，频率 footer 只规范化行顺序；原始 SHA 保留。证据 `r2-resources-functional-fixtures` 与 `r2-resources-functional-gdi`。初轮误写 `gdi-gateway`，生成器旧默认分支实际生成整数；按相同输入 SHA 保留额外 12 次整数执行，未计为 GDI 覆盖，正确 `gateway-gdi` 已独立补跑。

一对 Alice 200M 功能前缀均到同一预算出口，hidden 314×211、Draw/Present 0/0；复用既有 runtime 和输入，解释器软链接退出删除。证据 `r2-resources-functional-alice`。本轮不启动长计时，不根据这些小样本或一次启动差值作性能/RSS 准入，后台负载调查见[paint 记录](refactoring-painting-followup-2026-10-19.md)。
