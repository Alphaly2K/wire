# 当前交互阶段性能采样

时间：2026-10-21 04:20（Asia/Singapore）。用户已确认按钮有响应，随后将“卡住”修正为“可以推进，就是有点卡”。本次保留 PID 35751 和当前进度，未重启或重置 Users；只读取执行状态、函数栈和计数，不捕获画面。

对 `interactive-click-timing-cfrjj8m4` 连续执行 20 秒 `/usr/bin/sample`，请求采样间隔 1ms，取得主线程 **16,795 个样本**。采样前后分别只读附加 LLDB，随即 detach；主采样期间没有调试暂停。冻结运行二进制、源码与命令见该实例 `manifest.json`，分析脚本为证据根下 `analyze-current-profile.py`。

下表按调用路径将主线程样本互斥归类；包括等待时间，不是 CPU/GPU 利用率。

| 路径 | 样本 | 主线程占比 |
| --- | ---: | ---: |
| guest 解释执行、调度与内存访问 | 12,762 | 76.0% |
| 诊断启动器与状态观察 | 2,445 | 14.6% |
| 原生图形及相关调用、等待 | 1,346 | 8.0% |
| 宿主窗口事件轮询 | 231 | 1.4% |
| 其它已识别 API 分派 | 11 | 0.1% |

细分热点有重叠，不能再相加到上述表格：

- 标量 SSE `execute_fp`：**15.9%**；APFloat/软件浮点调用链：**13.9%**。APFloat 还可经其它向量/转换路径进入，两者不是严格的包含关系。
- 状态观察 `Watch::read`：**11.2%**，包含字节格式化；实际 `Memory::observe` 仅约 **0.3%**。当前每 1000 work 读取五条 watch，即使字节未变化也先分配并格式化字符串，之后才比较。这是诊断开销，不能拿该启动器直接当正式性能基线。
- `build_pipeline`：**2.5%**；`vkWaitForFences`：**4.8%**，均已包含在相关上层路径。此前两秒短样本也观察到管线创建、SPIR-V→MSL 转换和 fence 等待，但不同交互片段的短样本不能直接作为前后性能对比。

两个计数快照跨度 **41.369 秒**，覆盖 20 秒采样及前后间隔：完成 **1586 Draw / 84 Present**，约 **2.03 次 Present/秒、18.9 Draw/Present**。这是当前诊断版的实际呈现计数速率，不是无探针基准或帧时间分布；不能将该较宽窗口误写成 20 秒内的计数。

优化顺序建议：

1. **先取得无高频探针的基线。** 输入验收已通过，下一次正式运行可关闭五条 watch 和输入 trace，使用正常执行入口；当前窗口保持运行以保留进度。单独移除诊断开销不会消除执行器的主要成本。
2. **优先优化解释器派发和浮点密集路径。** `Process64::run`、单步缓存检查、checked/register segment、标量 SSE 与 APFloat 是主要 CPU 路径。进一步拆分派发成本和浮点成本，优先检查连续内存/浮点指令能否在保持预算、gateway、异常与逐条提交边界的前提下减少重复派发。浮点优化必须保留 MXCSR、舍入、denormal、NaN 和异常语义；当前采样不足以支持直接替换成普通宿主浮点。
3. **缓存图形管线。** [native_draw.h](../../src/platform/native_draw.h) 每次 draw 构造 `PipelineScope`、创建 render pass 并调用 `build_pipeline`；[native_graphics.cpp](../../src/platform/native_graphics.cpp) 用 NULL pipeline cache 创建 Vulkan pipeline，作用域退出就销毁。相同 shader/布局/渲染状态的重复创建应优先复用，并为缓存键、资源生命周期和容量建立明确边界。这也是交互中遇到新状态时的卡顿候选。
4. **再减少逐 draw 的同步提交。** [native_resources.h](../../src/platform/native_resources.h) 的 `Commands::submit` 每次创建 fence、提交并同步等待，draw 也走这条路径。可评估帧内批量提交和资源延迟回收，保留 CPU 读回、Map、销毁等同步点；这项改动涉及资源生命周期，验证成本高于管线缓存。

当前已经具备菜单响应和继续推进的人工反馈；输入优化与执行器/图形优化需要分别衡量。此次没有修改执行器或原生图形代码，没有宣称性能提升。

证据目录：`wire-out/alice-resume-2026-10-20/interactive-click-timing-cfrjj8m4/`。

- `host-profile-20s.txt`、`host-profile-20s-command.json`：主采样与准确命令/时间。
- `profile-summary.json`：互斥分类、包含关系、计数窗口、二进制及证据 SHA-256。
- `runtime-hotspot-snapshot-{1,2,3}.json`：仅代码地址、寄存器和计数；第 2/3 份为计数窗口两端。
- `host-sample.txt`：较早的两秒探索样本，未与主采样合并。
- `user-feedback.json`：按钮和推进反馈及后续修正。
- `runtime-profile/{summary.json,runtime-folded.txt,flame.svg}`：仓库既有 `tools/cpu_profile.py summarize` 对同一主采样生成的反混淆汇总、折叠栈和火焰图。该工具只保留能识别到运行循环边界的 **14,130 个样本**，剔除诊断启动器及缺少该边界的内联栈，因此火焰图百分比的分母与本文全主线程表不同。
