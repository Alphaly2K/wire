# 核心文件 API 契约迁移记录

本批以 `89a8f9a` 的冻结模块 catalog 二进制为性能基线（SHA256 `03e5b2a3a2c26a3e3ae9f55737a1a244ba07e7b5059da1b9aae1943818daa8b3`），在本地 main 开发，不 push、不复制游戏，不恢复 native Mono。当前候选尚未完成性能验收，不能据此将 R2 file 或 R3 PE32 状态所有权标为完成。

## 范围与行为

11 个核心文件 export 的 DLL、ABI、类型和执行计划集中到 FileApi。PE32 全部 typed，Win64 9 个 typed、SetFilePointer/SetEndOfFile 两个 absent；没有扩展能力。SetFilePointerEx 的 Int64 距离在 PE32 保持两个栈 slot，在 Win64 保持一个 64 位 carrier；DWORD 高位在 gateway 截断。路径/目录、metadata、section、标准句柄、跨资源 CloseHandle 留待后续。

PE32 的旧 file_api::dispatch 保留原函数/DLL/大小写范围，主 gateway 直接消费 bound ID。首版 Win64 FileCallResult 区分 Value 与提前 Returned；后续候选保留原 gateway 分支和调用边界，撤出这项抽象。各版都保持已保存 frame、CPU、计数、trace 和 endpoint 特有的 unblock 次序，以及输出 alias、PE32 先 seek 后输出、Win64 seek preflight、GetFileType 的 LastError 差异。

新增两项独立行为基线在迁移前、迁移后均通过：PE32 split Int64/清栈与定位后的输出失败；Win64 kernelbase 导入、DWORD 高位、负距离、输出故障 preflight、count alias return slot 与 absent 能力。首次测试误用了 logger 字段，编译失败日志和修正重跑均保留。

首版候选 release 408 passed / 7 ignored、debug Win32/AMD64/FPU/Unicorn 293、fmt、strict all-targets/all-features Clippy、工具 12 通过；native GPU/Metal 7 显式通过。冻结为 `r2-file-core-wire`，SHA256 `42f9f0d8ed954ffb690e826d7c5c3d80cf66f353f637acac71aa51cfa5eca1ff`，体积 5,820,152 字节。

## 失败与调查

首版 PE32 16 个 raw/IR case/path 的 96 轮都校验通过，但 SSE/IR 三对 +16.25%/+16.27%/+16.82%，CPU 中位数 +16.37%，不满足结构重构门槛。integer/IR −25.52%、file-type IR/raw −2.58%/−1.84% 不用于抵消该失败。尚未进行 Win64/V2 完整矩阵或游戏 smoke，没有将首版计为验收通过。

独立 700M SSE 采样中，guest self time 约 60–63% 落在 Descriptor::execute_inner；steps_cached、execute_mixed_run、execute_inner 前后同为 2208/503/1659 条，opcode 序列 hash 相同，地址及常量位置变化。没有改动 ISA、FLAGS、IR/cache 和预算，采样结果未确定回退的唯一成因。

第二个候选将 execute_mixed_run 和 Descriptor::execute_inner 放入同一个 4KiB 对齐 Mach-O 代码节，保留 body，避免一次迁移全部 PE32 executor。组合冻结为 `r2-file-core-sse-layout-wire`，SHA256 `f8575b18f0e6accc0a01426f61a679b17dbcbb470ba9e02cb5115e065a608cb8`，体积仍为 5,820,152 字节；release 408/7 ignored 通过。100M 四类 raw/IR 48 轮校验通过，但 FP/IR +7.46%、SSE/IR 一轮离散 +24.34%。700M 的 integer/FP/IR 三对扩样为 +0.09%/+0.25%；SSE/IR 成对 +2.83%/+1.29%/+15.52%，CPU 中位数比 +15.10%，两版都出现慢轮次，继续做同二进制噪声对照和更多成对样本，尚未验收。

原始证据均在 `wire-out/refactoring-2026-10-14/`，前缀 `r2-file-core-*`；冻结保留源码、diff、toolchain 和 hash。测试、计时、sample 分开，不删除失败或离散轮次，不变更验收阈值。

后续同二进制噪声对照的 CPU 中位数为 −0.18%，单轮范围 4.817–5.583s；第二候选 SSE/IR 700M 的九对为 +0.11%，成对范围 −0.38% 至 +0.53%。然而撤出 file catalog 单独验证这项布局后，integer/IR 三对 +40.88%/+41.29%/+40.76%，bit-test/IR 三对 +16.66%/+17.46%/+16.05%，因此独立布局被拒绝。停止进程组 25396，未完成的 AMD64/V2 和剩余 PE32 项不计为通过。独立布局正确性 release 408/debug 293、fmt、strict Clippy、native7 通过；Alice 200M 在原始 guest Mono 路径按预算停止（retired=199588615、API=390505、hidden 314×211、draw/present=0）。这些正确性结果未抵消性能失败。

两个 SSE 函数的代码节属性和 linker 参数已撤回，CPU 源码恢复原样。继续只调整 file adapter 的编译内联边界，并把新增 FileApi/Int64 声明追加到既有 variant/契约之后；不通过 padding 或反复指定地址寻找测量结果。新候选 `r2-file-core-inline-*` 仍在验证，两项独立行为回归已先提交为 `5cca1b9`。

内联候选的完整 release 408/debug 293、fmt、strict Clippy 通过，但 PE32 integer/IR 100M 三对 +13.84%/+5.45%/+7.34%，中位数 +7.22%，按门槛拒绝；bit-test/IR +4.85%，其余 raw/IR 中位数回退不超过 +0.4%。没有继续 native/Win64/V2/游戏验收，也没有保留内联标记。

下一版保留原 handler 所在函数与调用边界，只合并冷契约、删除 ABI/参数表重复项并用 bound enum 选择原分支。PE32 typed ID 直接进入 file_api 执行器，路径/目录 legacy facade 保留；Win64 原 gateway 提前完成/通用完成顺序保持，没有引入新的 FileCallResult。文件状态不再搬动，CPU/build.rs 无修改。首次机械替换误触 MoveFile 的宽字符选择被现有文件回归发现，精准修正后重跑全部检查；日志前缀 `r2-file-core-retained-fixed-*`，尚未做性能验收。

该保留边界版最终也未通过性能门槛：integer/IR 三对 +214.88%/+206.45%/+206.01%，CPU 中位数 +206.49%；FP/IR +8.37%、bit/IR +11.44%，raw 各项约 1% 以内。三个 IR 函数仍有 2208/503/1659 条指令，opcode 序列 hash 相同；这些证据没有确定唯一成因。日志 `r2-file-core-retained-preflight` 和机器码分析保留，文件 catalog 再次撤出。两项已提交的行为基线保留。

后续单独验证 PE32 执行器布局，文件 catalog 未接入。首个五声明候选实际只生成四个独立节：SIMD execute_inner 被内联进 execute，声明的 SIMD 节不存在。该版的 28 个 case/path 与 integer 700M 扩样已完成，最大最终 CPU 中位数回退 +1.58%，但不据此声称五函数全部隔离。删除无效属性又改变了编译器的内联选择，重建二进制未保持一致；现将隔离点放在实际 SIMD execute/回滚边界，并重新执行独立验收。旧版及删除属性的重建证据继续保留在 `pe32-executor-layout-*`；新候选使用 `pe32-executor-boundary-*`。
