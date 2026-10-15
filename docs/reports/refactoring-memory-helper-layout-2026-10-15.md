# 受检访存 helper 的编译布局

本批基线为 `cfe475d`，运行时与目录枚举契约 `240e4ac` 相同。路径/句柄 19 项迁移因访存等负载成本尚未通过验收，已完整保存并暂时撤回；网关函数提取与缓存命中内联的失败候选也已撤回。本批只验证访存 helper 的布局边界，不包含上述运行时代码。

独立 diagnostics 中，两版 memory 的 IR/cache 计数及 CheckedSegment 的 2156 条规范化指令一致。sample 将 checked_access、Pages::slot 与 read_int/write_int 定位为频繁调用的入口；冷 API 变更后，它们在普通 text 中的缓存行偏移发生变化。这个证据支持单独测试布局隔离，不能单凭地址变化断定成本来源。

macOS 将上述四个 helper 分别放入 4KiB 对齐的 Mach-O instruction section，并显式禁止内联以固定这个边界。函数名、参数、body、检查顺序、缓存算法、owner 和返回值保持；其它平台的属性与原来相同。build.rs 沿用现有 IR 执行器的 section alignment 机制。后续需核对独立 section 的生成代码、二进制体积、正确性、性能和启动前缀。

## 结果：拒绝并撤回

冻结候选为 `wire-out/refactoring-2026-10-14/memory-helper-layout-wire/wire`，SHA256 `37996868f4eccb248efa0379f3cd92eff3cf907f13f3ed5bce856f057e13f1e4`。四个 section 的 flags 均为 `0x80000400`，alignment 均为 4096；大小分别为 slot 272B、checked_access 376B、read_int 964B、write_int 1228B。它们确实形成了预期的生成代码边界。

完整 release 425 passed / 7 ignored、debug Win32/AMD64/FPU 273 passed、显式 native GPU/Metal 7 passed，fmt、strict all-target/all-feature Clippy、release build 与工具 12 项通过。结果位于 `memory-helper-layout-verify-*`。

随后在没有 Cargo、sample 或 runtime diagnostics 并发的条件下，以 700M work、三组 before/after/control 串行对照检查敏感路径。before 与 control 复用同一个目录枚举基线二进制；PE32 使用 utility QoS 与 null API log。

| 路径 | after CPU 中位数相对 before | control CPU 中位数相对 before |
| --- | ---: | ---: |
| AMD64 memory | +2.390% | −3.337% |
| AMD64 thread-id | +0.759% | +1.155% |
| PE32 SSE / IR | +10.908% | −1.245% |

全部 27 次运行的 work、retired、返回值和适用的 API count 校验通过。memory 存在明显宿主波动，全部轮次保留；SSE 三对候选均超过 5%，达到拒绝门槛。原始结果为 `memory-helper-layout-sensitive-{amd64,pe32}/compare.json`，汇总为 `memory-helper-layout-sensitive-metrics.json`。

四处 helper 属性与 build.rs 调整已全部撤回，未继续本候选的宽矩阵或 Alice smoke。现有执行器 section 和所有已接受的通用优化保持。路径/句柄迁移继续暂缓；下一批改为独立验证文件 section 契约，不依赖本候选。
