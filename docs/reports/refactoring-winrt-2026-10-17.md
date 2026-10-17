# R2/R3 WinRT 字符串与激活（2026-10-17）

## 独立旧行为基线

起点 `4707a91`，before 为 `r2-com-core-final-wire/wire`，SHA256 `d627b51f5ff407ccc1a9b963036a04ecd6844512bad0417b8cf267fb530dc1c3`。范围十一项 Win64 HSTRING/activation export，PE32 保留 absent 和未知 ABI。

基线提交 `049ec55`，五项新增独立回归全部通过：静态 provider 与动态 API set remapping、完整指针/DWORD、reference header 重读、owned metadata、HRESULT/LastError/输出优先级、返回槽别名、激活输出/trace/访存和 PE32 unknown 清栈。初次基线纠正了对 ole32 未实现 HRESULT 的预期，并去掉测试 helper 对只读返回帧的额外写入，未修改运行时行为；日志 `r2-winrt-baseline-tests*` 保留。

新增可选 `gateway-winrt` 原创 fixture，每轮在调用者栈创建含 embedded NUL/孤立 surrogate 的 reference，再读取长度并消费结果，没有 owned 分配增长。checksum、退休数、work 与 API count 在旧 executable 上 13K 预检通过，证据 `r2-winrt-fixture-baseline-apiset`。初版直接导入 combase 被旧 loader 拒绝，改用已准入的 string API set；模块装载范围保持。

## 实现

十一项 export 接入 WinrtApi 唯一声明，删除 arity/DWORD/provider 重复表、legacy bit 路由与调用时名称比较。静态 provider 分组与 GetProcAddress 的既有 API set remapping 各自保留。Strings owner 移入独立子模块，字段对 adapter 私有，负责 reference header 重读、owned 成员/长度/计数、release 与空 buffer 身份；adapter 继续原 backing allocator 协调。五项独立回归及现有 HSTRING 测试通过，边界见 [WinRT 契约](../architecture/winrt.md)。

## 完整正确性与冻结

完整 release **514 passed / 7 ignored**、debug Win32/AMD64/FPU **361 passed**、显式 native **7 passed**、工具 **20 passed**，fmt、strict all-target/all-feature Clippy 和 release build 全部通过。冻结 `r2-winrt-wire/wire`，SHA256 `6eb15b034181ae17bb97fb26d5f4e389d89f9d62767920640ce9ded4baa35e55`，**5,997,512 bytes**，增加 **480 bytes**。

冻结 manifest 的 **293 个 Rust 文件**（src/tests **285**）、合计 **391 个源码文件**逐项核对一致，target 与冻结 executable 哈希一致，证据 `r2-winrt-source-verification.json`。后续 BSTR 独立基线草稿尚未注册到 target，不属于本批冻结与验收。

## 性能与接受决定

稳定启动协议下 **50 条初筛路径**各三对全部结果校验，最大 CPU 回退 **+1.860%**，RSS 最多增加 **64KiB**。新增 reference/length 网关 CPU **−2.674%**，三对为 −2.674%/−2.839%/−1.227%；该负载未测量 owning copy 分配或 factory/backend。

PE32 integer/IR 初筛 −10.559%，三对为 −0.130%/−10.559%/+6.218%；系统信息/IR 初筛 +1.321%，有一对 +5.052%。两项追加三倍 work、直接引用 before 原路径的同哈希 control，按三种顺序各一对。完整 `r2-winrt-stable-summary.json` 保留 **52 个 workload protocol**，不同 work 不合并：

| 路径 | work | after/before CPU | control/before CPU | after/control CPU |
| --- | ---: | ---: | ---: | ---: |
| PE32 integer/IR | 900M | +0.082% | −0.084% | +0.166% |
| PE32 系统信息/IR | 150M | −0.210% | −0.568% | +0.360% |

Alice 200M 三对全部保持预算出口、隐藏 **314×211** 窗口及 Draw/Present **0**，CPU **−0.840%**、wall **−0.895%**。原 runtime 直接复用；编译、正式计时和 native 游戏串行，证据 `r2-winrt-stable-alice`。该前缀尚未实际绘制。

接受十一项 export，契约累计 **340 项**。未出现三对均回退超过 5% 的路径，全部离散轮次保留；integer/IR 的初筛负值不作为可靠收益。COM 基础及线程控制、线程属性、日历等批次的独立基线和局部成本继续保留于原报告，Mono 继续排除。
