# R2 矩形与窗口边框（2026-10-18）

状态：2026-10-18 验收通过。实现提交 `fd3da58`，13 项契约、纯核和适配器已接入。

## 独立基线

在 `f6f6fbc` 的旧 geometry 运行时上，六项独立测试通过。PE32 固定十项 RECT 的 signed/wrapping/empty/交并集规则、重叠 copy、PtInRect 三个 stack words、handler 后读取返回槽、跨页输出 fault 前缀不变；三项边框调整保留 96-DPI profile 与 unsupported-DPI 的 LastError 120。Win64 固定 packed 64-bit POINT、DWORD 高位截断、静态 kernel32/kernelbase/user32 与动态 user32 范围、七项 rectangle helper absent、unsupported-DPI 的 LastError 50、saved return frame 和跨页输出 fault 不提交 CPU/API。

首轮测试 module 缺少显式 path 属性，编译失败；修正注册路径后六项在旧运行时上通过，运行时未修改。证据 `r2-geometry-baseline-tests.txt`、`r2-geometry-baseline-tests-corrected.txt`。

原创可选 `gateway-rectangle` 循环 OffsetRect(+1, −1)，stack 输出，消费 BOOL 与累计 x 坐标，checksum 2n。旧冻结 `r2-input-profile-wire/wire` 上双架构 12K 验证通过，PE32 raw/static IR 均通过。首次 PE32 预期把终止 int3 误计为退休指令，结果相差一条；修正为 8n+15 后通过，未改变 fixture 机器码或运行时。原失败与修正证据 `r2-geometry-fixture-baseline32{,-corrected}`，Win64 为 `r2-geometry-fixture-baseline64`。工具 20 项通过，默认 case 集合保持。

证据根为 `wire-out/refactoring-2026-10-14/`。本批验收后累计 **437 exports**。

## 实现候选

GeometryApi 追加到 catalog，十项 RECT 与三项 frame 调整统一声明；POINT 的 Uint64 carrier 同时保留 PE32 12-byte 清栈与 Win64 2-slot arity。PE32 全部 typed，Win64 保持原六项 typed、七项 absent，以及静态/动态 provider 差异。

RECT/frame 纯核独立于 guest 内存与总状态，两端分别保留 codec、错误码、output alias、fault 和返回帧顺序。原 frame 逐段迁移并保留版权，旧窗口调用者通过兼容 re-export 继续复用；Win64 旧 USER 查询复用集中 RECT codec。旧重复 ABI/arity/DWORD、名称分发与 helper 删除。六项独立基线在候选上再次通过，证据 `r2-geometry-candidate-tests.txt`。

before 为已接受 `r2-input-profile-wire/wire`，SHA256 `8d5b89786c354e42a4587445078d57b0d5eb8d25463c32a78237dd3b8ed684c2`，5,928,456 bytes。本批代码体积增量预算为 +16KiB。完整正确性、基础双架构矩阵、新矩形网关及 Alice 200M 前缀按串行协议验收。

首次完整检查在 strict Clippy 阶段发现抽取后遗留的 GuestMem unused import；移除后完整重跑。修正轮 release **548 passed / 7 ignored**、debug Win32/AMD64/FPU **395 passed**、显式 native **7 passed**、工具 **20 passed**，fmt、strict all-target/all-feature Clippy 与 release build 通过；领域 PE32 **181**、Win64 **205** 项通过。初轮日志保留，修正轮使用 `r2-geometry-*-corrected.txt`。

代码体积初检超过原 +16KiB 文件预算：冻结 **5,949,080 bytes**，较 before **+20,624 bytes**。独立 Mach-O 调查发现 `__TEXT/__text` **−3,440 bytes**，所有 section 使用量合计 **+42 bytes**，其中 `__DATA_CONST/__const` **+2,848 bytes**。16KiB 对齐使 DATA_CONST file segment 增加整页 **16,384 bytes**，LINKEDIT 文件内容另增 **4,240 bytes**；TEXT/DATA file segment 大小保持。基于这一页对齐和链接元数据的实际成本，将本批完整文件预算明确修订为 **+24KiB**，不声称通过原 +16KiB 门槛。未为体积调整运行时代码或 release profile；证据 `r2-geometry-initial-size.json` 与 `r2-geometry-size-investigation.json`。性能与 RSS 继续独立验收。

冻结 `r2-geometry-wire/wire`，SHA256 `37574f910b7f5096d0799950e457a1743f047dd096e32c0a11bf295940ea769a`。**330 个 Rust 文件**（src/tests **322**）与活动源码/快照逐项一致，target/frozen SHA 一致，证据 `r2-geometry-source-verification.json`。共享 frame 源函数逐字相同，见 `r2-geometry-frame-verification.json`。

## 性能与启动验收

稳定启动、至少三对轮换，编译、计时与原生窗口运行串行。28 条初筛全部通过结果、退休数与 API 次数校验，RSS 中位数增量最多 **64KiB**。AMD64 integer/memory/SSE 为 +0.550%/+0.692%/+0.506%，FP +1.700%、code-alias +1.343%；PE32 CPU 的 IR/raw 八条范围为 −1.811%～+1.492%。初筛及扩样的原始轮次和 manifest 均保留。

初筛 AMD64 bit-test **+3.196%**、data-alias **+3.422%** 触发调查，扩至 900M，加入 before 同哈希 control，轮换三种顺序。bit-test 跨轮中位数 **+1.039%**，三对分别 +4.527%/+0.774%/+0.520%；control/before +0.268%，after/control +0.769%。data-alias 中位数 **−1.246%**，三对 −1.246%/+2.130%/−2.305%；control/before −0.079%，after/control −1.168%。扩样没有持续超过门槛的回退；离散轮次不删除，未据此定位具体微架构原因。

新矩形网关 CPU 中位数 Win64 **−2.439%**，PE32 IR/raw **−19.121%/−17.978%**。其余初筛网关最大回退 thread-id **+2.085%**；这些数字仅描述各自循环。最终 **30 个 workload protocol**（28 初筛、2 长样本）全数校验，见 `r2-geometry-stable-final-summary.json`；扩样目录 `r2-geometry-stable-expanded-v2-{0,1,2}`，初筛为 `r2-geometry-stable-{amd64-cpu,v2,amd64-gateways,pe32-cpu,pe32-gateways}`。

Alice 复用现有 runtime，200M 三对均到预算出口，隐藏 client 314×211，Draw/Present 0。CPU/wall 中位数 **−5.910%/−8.017%**，CPU before/after 2.758533s/2.595497s；未从这一前缀推断渲染帧率。证据 `r2-geometry-stable-alice/compare.json` 与 manifest。
