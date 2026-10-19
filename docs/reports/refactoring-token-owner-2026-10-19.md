# Win64 token 状态归属

状态：正确性、性能调查和启动前缀验收完成，保留 PE32 整数 IR 的局部成本。直接运行时基线为 `9281501` ETW owner；三个独立旧行为测试和原创 fixture 先提交于 `87ae227`。本批不新增 catalog export，累计仍为 **706**。

私有 `token::Tokens` 拥有 query access、handle flags、128 个存活 token 与 4096 个单调编号。OpenProcessToken/GetTokenInformation 只借 Memory、参数和当前线程 LastError 地址；通用句柄操作通过 flags/set_flags/close 委托。原 SID 和 information codec 除类型别名及可见性外逐字保持。OpenThreadToken 的线程身份判断、legacy provider/ABI、saved return frame、早退 API count/trace 提交继续由原适配边界负责。详见[资源边界](../architecture/tokens.md)。

测试先在旧实现验证 process/access/容量优先于 output、故障不消耗编号、DWORD access 与完整 64-bit 句柄、output alias、query 权限/内部指针溢出/required-size guard、保护关闭错误 6，以及累计编号耗尽。迁移后 release/debug 定向各 8 项通过。完整 `r3-token-validation-*`：release **650 passed / 7 ignored**，debug **496**（Win32 237、AMD64 248、FPU 9、日志单元 2），显式 native GPU **7**、工具 **30**；fmt、strict all-target/all-feature Clippy、SIMD crate 静态检查与 release build 通过。

原创 gateway-token-info 在循环前打开一个 QUERY token，每轮消费 BOOL、TokenElevation 和 required size；n=max(1,work/13)，RAX=5n+1、retired=12n+20、work=13n+21、API=n+1。小输入先在旧冻结实际校验，inventory 固定两个 advapi32 导入与独立 OFT/IAT。150M 查询循环 CPU 中位数 **−0.639%**；不据此宣称游戏或其它计算加速。

冻结 `r3-token-wire/wire`：SHA256 `58663fd48cd7a05cebe0a4ab62cc9dd291cbdb9a6800f984748a5291bc29a312`，**6,072,696 bytes**，较 ETW **+9,536 bytes**，在本批 +24KiB 预算内。302 个 src Rust 文件、target/frozen/manifest 核对一致，仅 runtime.rs/token.rs 改变，证明 `r3-token-source-proof.json`；没有扩大缓存或资源限额。

三标签轮换、同旧 binary control、stable launch/fixed executable slot，共 **17 条协议、162 次执行**全部校验；常规 AMD64/PE32 使用 utility，V2 保持独立默认 QoS。初筛 SSE +5.414%、thread-id +3.425% 及 memory 相对 control 的漂移，追加 900M 后分别为 **+0.566% / −0.670% / −2.708%**。其余初筛 AMD64/V2 与 PE32 raw/FP 为 −0.606% 至 +2.229%。fixture RSS 中位数最多增加 **114,688 bytes**。

PE32 integer/IR 初筛 **+3.438%**；两轮各三对 900M 分别 **+2.376% / +3.145%**，同协议六对汇总 **+3.320%**。保留全部轮次，包含 +10.360%、+6.654% 的单对成本和同旧 binary 漂移；第二轮相对 control +5.485%，不以六对汇总的 after/control −2.334% 抹去该差异。两端分别做两次独立采样：均为 900,000,003 条 IR、1 次 INT3 fallback、149,999,714 个 scalar run，热点仍是 steps_cached、PendingFlags::compute 与 cc。20 份 PE32 CPU/memory/IR/SIMD 源码未变；六个执行器节的助记符序列保持，原始 0F 节增加一条指令。静态比较未覆盖所有操作数，具体成本机制尚未确证。本批经过调查后接受这一已记录的局部成本，没有三对均超过 5% 的持续回退，也没有将采样计时纳入正式对照。证据 `r3-token-pe32-codegen-investigation.json`、`r3-token-profile-*` 和 `r3-token-performance-summary.json`。

三对 Alice 200M 在可访问 Metal/AppKit 的宿主环境全部到相同预算、hidden 314×211、Draw/Present 0/0 出口。CPU/wall 中位数 **−3.611% / −3.871%**，RSS 中位数 **+49,152 bytes**。该样本是未进入绘制的启动前缀；复用原素材，解释器临时软链接退出删除。证据 `r3-token-performance-alice`，未复制或硬链接游戏资源。
