# Toolhelp 七项 API 统一契约

状态：正确性、性能及启动前缀验收完成，保留代码别名负载的局部成本。直接运行时基线为 `8aa9d05` 的 snapshot owner 冻结；双架构 provider/ABI 旧行为与 PE32 fixture 先提交于 `5db0070`。七项声明接入 catalog，累计 **713 exports**。

`SNAPSHOT_CONTRACTS` 统一 provider、参数 slot/宽度和 typed ID，删除 Win64 重复 arity、DWORD 表及字符串执行分支。Win64 静态导入与动态查询继续接受 kernel32/kernelbase，枚举 owner 使用绑定得到的 SnapshotApi。PE32 仅 kernel32 有已知 stdcall ABI，沿用 LegacyUnimplemented 的零返回、error 120、输出不变及正确清栈；kernelbase/user32 保持未知 ABI 的零清栈。不新增 Process32FirstA/NextA alias。Thread32First/Next 的句柄保持完整 pointer carrier。原 create、flags、close 与记录编码逐字保持，详见[资源边界](../architecture/snapshots.md)。

完整 `r2-snapshot-validation-*`：release **655 passed / 7 ignored**，debug **501**（Win32 238、AMD64 252、FPU 9、日志单元 2），显式 native GPU **7**、工具 **31**；fmt、strict all-target/all-feature Clippy、SIMD crate 检查及 release build 通过。冻结 `r2-snapshot-wire/wire` SHA256 `fa4a3e660459bd2c8fd63cfbe4a5edf99efb641e24505b08fe123b0ff96cd7d3`，**6,043,832 bytes**，较 owner **−9,968 bytes**，303 个 src Rust 文件与 target/frozen/manifest 一致；证明 `r2-snapshot-source-proof.json`。没有扩大缓存或资源限额。

三标签轮换并穿插同旧 binary control，stable launch/fixed executable slot；常规 AMD64/PE32 为 utility，V2 保持原默认 QoS。18 组计时批次、**17 个不同协议、162 次执行**全部校验；同协议的 raw 追加样本合并，不合并不同 work。汇总 `r2-snapshot-performance-summary.json`。Win64 snapshot 150M copyout CPU **−12.007%**；其它计算/网关最终中位数 −2.149% 至 +2.305%，代码别名负载单独列于下段。PE32 integer/raw 初筛 **+4.923%**，追加三对 900M **+0.684%**，同协议六对汇总 **−0.745%**，相对 control **+0.888%**。保留全部慢轮。fixture RSS 中位数最多增加 **114,688 bytes**。

code-alias 150M **+3.685%**，追加三对 900M **+4.286%**，相对 control **+4.404%**；三对各 **+4.786%/+3.137%/+4.110%**。这是持续的局部成本，本批接受并明确保留；未触发三对均超过 5% 的默认拒绝门槛，也不以快照网关的改善抵消它。串行 diagnostics 两版均 IR 12,000,008、fallback 0、translations 20、invalidations 0。macOS sample 两版热点均在 CLI 执行循环和 reuse_translation/cache save；采样不进入正式计时。额外抽取实际 CLI 单态化 segment caller、checked_access、reuse_translation、save 的完整反汇编与操作数；前两项主要缓存热区在地址归一化后相同，CLI 热区仍有常量地址差异，冷错误路径亦有重排。没有认定整个生成代码相同，具体性能机制尚未确证。证据 `r2-snapshot-code-diagnostics`、`r2-snapshot-code-profile`、`r2-snapshot-code-helper-comparison` 及 `r2-snapshot-core-codegen-investigation.json`；未进行 CGU/LTO/inline 扫参。

PE32 gateway-snapshot 测量 ABI-only CreateToolhelp32Snapshot 快捷路径，IR/raw CPU **−82.834%/−77.516%**。它不创建快照，不增加 PE32 API 兼容性；n=max(1,work/6)，EAX=0、retired=6n+7、API=n。十次调用的前后 raw/IR 四份完整日志逐字节相同，均包含 UNIMPLEMENTED 诊断；每份 1,032 bytes，SHA256 `20aefb2765040a0bcd0834d79f502743fc6c2846167c87d1ee2f792d142fb230`，见 `r2-snapshot-trace-proof.json`。没有通过省略日志取得该收益。

三对 Alice 200M 在可访问 Metal/AppKit 的宿主环境全部到同预算、hidden 314×211、Draw/Present 0/0。CPU/wall 中位数 **−1.008%/−0.404%**，RSS **−770,048 bytes**。复用原 runtime 与游戏输入 SHA，解释器临时软链接退出即删除。该结果为尚未进入绘制的启动前缀。证据 `r2-snapshot-performance-alice`；所有本批证据位于 `wire-out/refactoring-2026-10-14/`。
