# Win64 热网关编译边界候选与拒绝

状态：三个候选均未接受，网关和 build.rs 运行时代码已全部撤回。独立句柄基线已提交为 `cfe475d`；路径契约等待在已通过的运行时基线上继续。

基线为 `3bb016f`（运行时与 `240e4ac` 相同），before 使用已冻结的 `r2-file-search-cold-route-wire/wire`，SHA256 `927662997fbc71b5102113ab1b5ca2f8f9f5c86d723010d177d7f56be659438f`。路径辅助首版候选因 thread-id 的 700M 三对持续回退超过 5% 已撤出；本批单独解决网关的编译边界，不包含路径契约运行时代码。

## 首版改动

dispatch_bound 保留原 import 支持、arity 与 delay hook 选择。通用参数读取、完整 CPU 快照和原 handler 分发移入不内联的 dispatch_general；原 dispatch_hot 算法和返回/故障/LastError/trace/unblock 顺序保持，固定为不内联的独立执行器。macOS 将后者放入 4KiB 对齐的 `__TEXT,__wire_gw64,regular,pure_instructions` 节，其它平台保留普通节归属。不改变 HotImport 集合、绑定字段、guest ABI 或 ISA。

生成代码中，原 dispatch_bound（包含内联 hot 与 general）有 18083 条指令，入口固定分配 0xac0=2752 bytes 的自有 frame。新入口 94 条、128 bytes；独立 hot 436 条、176 bytes，入口与 hot 自有 frame 合计 304 bytes。这里统计函数自身的 frame，不包含其调用的其它函数。原/新入口和 hot 反汇编及摘要为 `gateway-boundary-{before,after}-entry.asm`、`gateway-boundary-after-hot.asm` 与 `gateway-boundary-machine-code-summary.json`。

## 验证与冻结

完整 release 421 passed / 7 ignored、debug Win32/AMD64/FPU/Unicorn 306、显式 native GPU/Metal 7、fmt、strict all-target/all-feature Clippy、release build 均通过。只加 hot 非内联属性的中间构建日志也保留；正式候选同时包含 general 边界与 macOS 对齐，所有检查在最终源码上重新执行。

候选为 `wire-out/refactoring-2026-10-14/gateway-boundary-aligned-wire/wire`，SHA256 `f537f20de19de99ede24a315a8c3881f0e425a5b50cf34408f436826f923bac6`，5,927,432 bytes。待迁移的句柄测试/fixture 草稿尚未接入 test target 或工具入口，未用于本批运行结果。

## 首版性能验收与拒绝

计时使用冻结二进制，串行 AB/BA，与 Cargo、sample、runtime diagnostics 分开。PE32 保持 utility QoS clamp，API trace 格式化/计数写 null device；AMD64/V2 保持默认调度。thread-id 的 700M 三对 CPU 中位数 −7.09%。搜索 700M 首轮中位数 +7.16%，三对分别约 +2.35%/−2.76%/+17.79%，因此追加相同 work 的 before/after/control 三标签对照，并按全部六对合并，不丢弃初筛。control 与 before 引用相同 binary/hash。

首版的 39 个 case/path 全部校验。搜索六对合并为 +0.51%。PE32 bit-test/IR、AMD64 memory、data-alias 的短样本中位数分别为 +17.19%/+7.07%/+4.45%，因此扩样。bit-test 的 700M 三标签对照仍有明显宿主波动：同一基线首对 before/control 为 3.93/5.67 CPU 秒；候选中位数 +5.09%。memory 的 700M 三对候选持续回退 +12.41%/+9.76%/+13.08%，中位数 +11.43%，control 中位数约持平。按门槛拒绝此版，未执行其余无必要的 data-alias 补样，全部初筛及补样保留。

独立 memory diagnostics 的 IR=12008、fallback=0、translations=16、invalidations=0 及全部 translation causes 一致。两版 CheckedSegment 均为 2156 条指令，opcode 序列一致；除地址/符号注释外，规范化操作数也一致。原始机器码与检查在 `gateway-boundary-{before,after}-checked.asm`、`gateway-boundary-checked-code-summary.json` 和 `gateway-boundary-memory-diagnostics/`。这些检查没有消除实测成本，不能用来将首版判为通过。

## 入口边界修正

小型 dispatch_bound 也显式标记禁止内联，防止提取后的入口重新进入调用方的内联决策；general/hot 边界和算法保持。四项新增句柄基线在仍未迁移的实现上通过，覆盖 PE32 清栈/ABI-only/故障提交和 Win64 参数宽度/输出别名/返回帧预检。修正候选冻结在 `gateway-boundary-entry-wire/`，先对 memory/thread-id 做 700M 三标签初筛，之后再决定扩大验证。

入口修正版的 memory 中位数仍为 +8.25%，control 为 −3.97%；thread-id 为 −7.27%，control 为 +5.11%。没有接受带有 memory 成本的版本。下一版撤出新增 Mach-O 节与对应 linker alignment，仅保留三个函数的禁止内联边界；新的源代码与冻结结果单独保存。

仅函数边界版冻结于 `gateway-boundary-functions-wire/`，memory 700M 三对候选为 +31.24%/+9.40%/+4.70%，CPU 中位数 +8.47%。全部 work/checksum 校验，但该版也未接受，停止扩大矩阵。三个候选及中断/未执行的项目按实际范围记录，不将首版的 39 条路径外推到后两版。

正式计时结束后分别抓取 3 秒 sample，文件为 `gateway-boundary-profile-{before,after}.txt`，对应运行结果也单独保留。热点都是 CheckedSegment、checked_access、Pages::slot 与标量读写，profile 轮次保持相同 700000008 retired/work 和校验值。两版 checked_access 的 inclusive 样本占比约 20%；页槽缓存命中仍需经过独立 slot 函数。接下来单独验证内联命中路径、隔离 hash miss 的小型优化；网关边界方案暂缓，未用其它优化抵消其回退后混入同一提交。
