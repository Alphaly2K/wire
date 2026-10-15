# 页槽缓存命中路径内联

状态：候选拒绝，Pages 运行时代码已全部撤回。不得将本报告中的候选验证计数或 memory 局部改善视为 main 的交付结果。

基线为 `cfe475d`，运行时与目录枚举契约 `240e4ac` 相同；before 复用 `r2-file-search-cold-route-wire/wire`，SHA256 `927662997fbc71b5102113ab1b5ca2f8f9f5c86d723010d177d7f56be659438f`。网关函数拆分的三个失败候选已经撤回，调查见 [网关拒绝记录](refactoring-gateway-boundary-2026-10-15.md)。本批单独处理 profile 中的页槽查询调用成本。

## 改动与边界

Pages::slot 的小型缓存命中路径显式内联，HashMap 查询和缓存填充移入不内联的 slot_miss。缓存的大小、hash、完整地址比较、命中/未命中结果、失效顺序与 owner 均保持。缓存保存索引，不保存裸指针或权限证明；get/get_mut 继续从实时 slots 取得 Page，现有权限、guard、generation、shared backing 与写入检查保持。没有修改 IR 格式、guest 指令预算、FLAGS、API 语义或 Mono。

冻结候选为 `wire-out/refactoring-2026-10-14/page-slot-inline-wire/wire`，SHA256 `ee464e120ef82668fb68255a3a79ef378ff28ced2fe9481dea9894255e14cf6e`，5,923,416 bytes。nm 显示独立 Pages::slot 已消失，slot_miss 仍为独立符号；对应最终生成代码与性能检查单独记录。

## 验证与拒绝

700M memory 的 before/after/control 三标签对照全部校验，CPU 中位数候选 −3.77%，同二进制 control 仍保留用于观察宿主波动。完整 release 为 425 passed / 7 ignored，debug Win32/AMD64/FPU/Unicorn 为 310 passed，显式 native GPU/Metal 为 7 passed；fmt、strict all-target/all-feature Clippy、release build 均通过。四项句柄基线和工具 12 项已先独立通过。

后续性能矩阵计划串行执行，与 Cargo、采样和 runtime diagnostics 分开：AMD64 integer/SSE/thread-id 700M；PE32 integer/SSE/bit IR 700M、四类 raw 与 FP/IR 100M；AMD64 FP/bit/code-alias/data-alias 300M；六项 AMD64 gateway 100M。连同 memory 原计划 22 个 case/path。PE32 保持 utility QoS clamp 和 null API log，默认 AMD64 调度保持原样。

该矩阵完成 19 个 case/path，全部 work/retired/返回值校验，但没有通过性能验收：PE32 SSE/IR 的 700M 三对持续回退 +15.55%/+8.18%/+16.74%，CPU 中位数 +15.55%。AMD64 thread-id 中位数 +5.70%，data-alias +3.73%，file-type +7.64%，也保留为未解决成本。此前 memory 的 −3.77% 没有抵消这些回退；按持续回退门槛拒绝并撤回全部八行运行时代码，停止剩余 gateway 轮次，保留 KeyboardInterrupt 和已完成的原始结果。未运行后续 thread-id control、额外 diagnostics 或 Alice 启动；源代码和冻结候选用于复现调查。

PE32 SSE 的测试 PE/IR 输入哈希分别为 `98b115dfaf4f5b77034ab183a628803cfbfc00fc8025a49bd49af3dca4259862` / `63046e97c639efa683647aabeb4bf44867c95ccc0cc5839891d369e8f2d14ba8`，预期 retired 为 700000002。正式 fixture、输出、参数和全部轮次仍位于 `page-slot-inline-*`；独立测试计数属于候选正确性结果，不是性能通过。
