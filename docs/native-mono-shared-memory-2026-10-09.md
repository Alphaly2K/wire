# Mono 受控共享 backing（2026-10-09）

本阶段在 `codex/native-mono-shared-memory` 独立 worktree 实施，基于 `59ca3d7`。
未修改 CPU、SSE、IR 或主 checkout。共享访存仅在 `native-mono-memory` 编译
feature 下存在；默认构建没有共享 registry、额外字段或读写分支。
该 feature 不启用原生游戏后端。

## 数据与执行边界

`src/x64/memory/shared.rs` 新增 `HostRegion`：稳定的字节区间、保留的 owner、
跨别名撤销状态和可重入执行 lease。owned 存储位于 `UnsafeCell<Box<[u8]>>`；
foreign 构造是 unsafe，调用者必须证明地址范围、初始化、可写存储、owner 的
存活与释放线程，以及包括 native/GC writer 在内的所有别名访问同步。
`with_native` 只交出 raw data pointer，不交出可跨重入存活的 Rust slice。
同一宿主线程可嵌套重入，其他线程等待 lease；panic 退出会释放 lease。

Memory 保留原有 owned 页，另建明确的 byte-interval registry。支持非页对齐的
数组元素区间，但不将周围宿主页或相邻分配一起暴露。区域只能授予 READ/WRITE，
不能授予 EXECUTE；权限上限由 owner 登记。拒绝越界、非 canonical 地址、重叠、
跨登记边界和撤销后的访问。检查完整跨度后才写入，故障不会部分改写数据。

每个登记区间对其覆盖的每个 4 KiB 页维护 one-shot guard。故障只消耗首先触及
的 guard；一次跨两页访问可能依次触发两次。权限/guard 属于 mapping view，
不同地址的 alias 可以拥有不同保护；revoke 属于共享 region，会同时失效所有 view。

共享区间参与 mapping/merge/VirtualAlloc 冲突检查、容量与 memory-status 记账。
容量按每个登记 view 的字节数保守计入，alias 也计数。普通 VirtualProtect/Free
不能接管 foreign allocation。精确对象区间无法真实表示为 Win32 页分配，
VirtualQuery 对触及该宿主页的查询返回失败，free-region 查询也不会跨过它。
`protect_shared_data` 是精确区间原型 API，不宣称已完成托管堆的 Win32 VM 语义。

## 验证

`cargo test --release --features native-mono-memory --test mono_shared`：8 项通过：

- native/guest 同一 backing、非对齐/跨页 scalar 访问、同线程 callback 重入；
- Cpu64 真实指令读写，以及禁止从数据区执行；
- readonly、NOACCESS、精确区间、overflow、越界与故障输出不变；
- 分页 guard、跨页写入原子故障；
- 两个 Memory alias 在两线程执行 2,000 次受 lease 保护的 read-modify-write；
- competing lease 阻塞与 panic 释放、所有 alias 撤销；
- foreign owner 保留到最后一个 mapping/unmap handle 释放；
- VirtualAlloc、merge、查询和容量记账不覆盖 foreign 区间。

完整 release 回归：默认构建 **336 passed / 7 ignored**；feature 构建
**344 passed / 7 ignored**。两种构建的 all-targets Clippy `-D warnings` 均通过。
7 个 ignored 是原有依赖真实 GPU/外部图形库的测试，本次未更改图形路径。

实际 Mono pinned 数组、GC、线程、unsafe IL 和用户样本的证据，见
[native-mono-m2-shared-2026-10-09.md](native-mono-m2-shared-2026-10-09.md)。
不能由 Memory 的受检访问推定 native IL 或 native helper 自动遵守同样的保护。

## 普通解释路径的测量

冻结 `59ca3d7` 构建作为 before，当前默认构建作为 after；原创 PE 的 checksum
和精确退休数全部一致。构建结束后，对每类 300M guest work 做三对 AB/BA，
关闭统计和 profiler。此前与 Mono 编译同时执行的两批计时不用于验收。

| 原创负载 | before MIPS 中位数 | after MIPS 中位数 |
| --- | ---: | ---: |
| integer | 154.43 | 153.63 |
| memory | 107.74 | 107.18 |
| simd-mixed | 36.99 | 37.43 |
| large-code | 44.14 | 44.15 |
| indirect-call | 147.04 | 140.22 |

这是默认路径的影响检查，不是原生 Mono 或游戏加速结果。间接调用项出现回退信号，预热后的七对500M复测为135.33→130.44 MIPS。
随后将默认VM容量表达式和空闲区遍历保持原始实现；再次七对500M复测为
93.84→94.98 MIPS，未重现持续回退。后一次整体吞吐和pair间波动明显变化，
因此不把这次差值解释为性能收益，也不跨批比较绝对MIPS。
不以关闭 feature 或编译掉分支单独宣称机器码布局/整机计时绝无变化。

可复核日志位于独立 checkout 的 `wire-out/native-mono-default-tests.log`、
`native-mono-shared-tests.log`、`native-mono-memory-default-quiet-perf/compare.json`
、`native-mono-default-indirect-repeat/compare.json` 与
`native-mono-default-indirect-preserved/compare.json`。二进制与日志不提交仓库。
