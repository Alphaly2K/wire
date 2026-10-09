# 原生 Mono 有限启动阶段性能（2026-10-09）

实际修复在 `src/x64/process/mono_startup/callbacks.rs`。此前每次 owner scope 都创建/
撤销 1MiB guest callback stack，约 120 万个 metadata 查询为叶调用却仍付出此成本。
现在 owner 安装只登记状态，首次真正 callback 才分配完整栈；同 owner 的嵌套与 GC
回调复用。深度、TEB 边界和 guard 保持原契约，Drop 仅撤销成功分配的栈。

## 相同工作量的三对冻结 AB/BA

`tools/mono/perf_startup.py` 使用 frozen before/after wire，每轮 fresh APFS game clone；
只比较 wire subprocess 的 user+system CPU。输入 SHA 和所有关键初始化计数必须一致。
安装/复制成本不计 CPU；wall 是诊断运行与输出解析成本。运行时无 JIT/动态 trampoline。

| 次序 | 配置 | CPU 秒 | wall 秒 |
| --- | --- | ---: | ---: |
| 1 | before | 53.156647 | 54.634801 |
| 2 | after | 5.224913 | 6.874751 |
| 3 | after | 5.179268 | 6.228909 |
| 4 | before | 53.300444 | 54.331386 |
| 5 | before | 53.546327 | 54.806394 |
| 6 | after | 5.142675 | 6.370785 |

CPU 中位数 53.300444 → 5.179268 秒，约 **10.29×**；wall 中位数
54.634801 → 6.370785 秒。范围是有限原生 Mono 初始化，终点 `mono_object_new`
之前。完整游戏 backend/Admission 仍 false，不能推算 FPS 或整款游戏启动倍数。

每轮均 exit 0，原始 x64 Mono 未 attach，noexec guard denied=0；初始化相同：
8 actual top-level managed steps，130 images，1,204,030 metadata queries，25,096
handles，38 assembly-name parses/hits/frees，967 attributes / 2,821 field reads /
7 Attribute[]，7 checked statuses，592 external strong handles。根、数组元素身份、
字段快照和所有视图/handle 的 GC/cleanup 检查仍通过。

## Profile 与回归

5 秒 sample：before Process::run 3023 样本，Memory map/unmap 1586（约52.5%）；
after Process::run 2310，map/unmap 4（约0.17%）。Profile 本身耗时不用于上述对照。

新增回归覆盖叶 scope 不分配栈、预存映射保持、实际 callback 碰撞拒绝且不误撤销、
去掉冲突后真实 callback 成功及 owner 退出撤销。25 项定向测试、feature all-target
Clippy 通过；完整 release feature 回归 **425 passed / 7 ignored**。

## 冻结输入与原始证据

证据根为实施 worktree `/Users/alphaly/.codex/worktrees/native-mono-shared-memory/wire/wire-out/`：

- `native-mono-startup-lazy-stack-perf/results.json`：全部六轮与所有外部 SHA。
- `native-mono-startup-stack-profile-{before,after}/sample.txt`：原始 sample。
- before executable SHA256：`15a3d98752474f7a2d6ba94a7900867244fe31187b19396ebdf5727dbedf20e6`。
- after executable SHA256：`1bbfabed69affecb6919165ec9ee2b87e82aedb32587a9cf450b8cb18659c068`。
- native Mono SHA256：`94a8252dfb4bf7aa3584be4b1888afd128431cc7ea70b7bd6773fc299cab0a5e`。
- precompiled corelib support SHA256：`7cd421d0e370b09a865848a59e47aa2b1362fd9ff484804fe3b2c1b4a5bc8029`。

功能扫描的更早失败（string 容量/预算耗尽）不作为通过证据。此前 runtime 不修改。
