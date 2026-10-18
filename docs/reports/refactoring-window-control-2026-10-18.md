# 窗口生命周期与过程调用基线（2026-10-18）

状态：十七项 typed 迁移已接入，独立基线与完整正确性通过，尚待性能验收。日志 `r2-window-control-baseline-tests.txt`；前批文本/输入所有权的初筛与游戏运行结束后才编译。该批冻结源码此前已核对，随后新增的三项测试不属于其 380 文件快照。

计划范围十七项：CreateWindowExA/W、DestroyWindow、DefWindowProcA/W、DefDlgProcA/W、CallWindowProcA/W、ShowWindow、UpdateWindow、EnableWindow、SetParent、MoveWindow、SetWindowPos、SendMessageA/W。PE32 保留 callback/block/nonlocal transfer，Win64 保留 callback resume 与四项 absent。创建结构、拒绝回调、嵌套默认位置消息、绘制及跨线程发送已有独立原创回归，本批在此基础上固定额外的验证顺序。

新增 PE32 两项检查 ShowWindow 非零值/跨线程与 DestroyWindow owner 检查的区别、SetParent 循环拒绝、ShowWindow 及 DestroyWindow callback 故障前状态提交。Win64 一项检查静态 provider/absent 范围、有效 API 返回槽但无效 callback frame 时，错误先于 HWND lookup 且 CPU/API/LastError 保持。

首次构建前固定 executable 增量预算为相对已验收 `r2-window-state-wire/wire` +24KiB。PE32 以具名控制结果保留 callback、阻塞和非局部转移；Win64 UpdateWindow 保持 scalar，其余受支持调用在执行器上校验 callback frame。DispatchMessage 仍执行原来的外层检查及转入 SendMessage 的第二次检查。旧手工 ABI、arity、DWORD 和普通名称执行分支由唯一契约替代。

三项迁移后回归通过，日志 `r2-window-control-candidate-tests.txt`。首次完整检查发现 native DXGI 测试调用了删除的私有 user_call 入口；改为 typed window_value_call 后重新执行，首轮编译失败日志保留为 `r2-window-control-validation-release-tests.txt`，后续日志使用 `r2-window-control-validation-corrected-*`。[架构边界](../architecture/window-control.md)记录控制与资源归属及架构差异。

原创可选 `gateway-window-control` 实际创建/销毁窗口，每轮 SendMessageW 进入应用 guest WNDPROC，返回 3 并消费累加值。两端结果均为 3n+1；Win64 retired=10n+51/work=11n+54，PE32 retired=10n+36，API 均 n+3。旧运行时双架构及 PE32 IR/raw 均独立通过。Win64 首两次 fixture 入口与固定导入区重叠，缩短零常量载入并加入边界断言后通过；失败日志与最终 `r2-window-control-fixture-baseline64-layout`、`r2-window-control-fixture-baseline32` 均保留。工具 23 项通过。该修正只涉及原创测试 PE，不涉及运行时。

完整 release 603 / 7 ignored、debug 449、显式 native 7，以及 fmt、strict all-target/all-feature Clippy、build 全部通过。384 个 Rust 文件（src/tests 376）live/frozen/manifest 与文件集合一致，target/frozen executable SHA 相同。冻结 `r2-window-control-wire/wire`：SHA256 `aab40883614c0b70188c75408527084098ff2cfe0d69b0da3bd34dcc7f0737ad`，5,964,712 bytes，相对前批减少 12,720 bytes，满足预设 +24KiB 预算。证据 `r2-window-control-validation-corrected-source-verification.json`。下一步用该冻结执行 57 条路径和 Alice 200M 前缀，编译与计时串行。
