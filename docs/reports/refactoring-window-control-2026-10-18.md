# 窗口生命周期与过程调用基线（2026-10-18）

状态：三项独立旧行为基线全部通过，十七项 typed 迁移草稿已接入，尚未编译验收。日志 `r2-window-control-baseline-tests.txt`；前批文本/输入所有权的初筛与游戏运行结束后才编译。该批冻结源码此前已核对，随后新增的三项测试不属于其 380 文件快照。

计划范围十七项：CreateWindowExA/W、DestroyWindow、DefWindowProcA/W、DefDlgProcA/W、CallWindowProcA/W、ShowWindow、UpdateWindow、EnableWindow、SetParent、MoveWindow、SetWindowPos、SendMessageA/W。PE32 保留 callback/block/nonlocal transfer，Win64 保留 callback resume 与四项 absent。创建结构、拒绝回调、嵌套默认位置消息、绘制及跨线程发送已有独立原创回归，本批在此基础上固定额外的验证顺序。

新增 PE32 两项检查 ShowWindow 非零值/跨线程与 DestroyWindow owner 检查的区别、SetParent 循环拒绝、ShowWindow 及 DestroyWindow callback 故障前状态提交。Win64 一项检查静态 provider/absent 范围、有效 API 返回槽但无效 callback frame 时，错误先于 HWND lookup 且 CPU/API/LastError 保持。

首次构建前固定 executable 增量预算为相对已验收 `r2-window-state-wire/wire` +24KiB。PE32 以具名控制结果保留 callback、阻塞和非局部转移；Win64 UpdateWindow 保持 scalar，其余受支持调用在执行器上校验 callback frame。DispatchMessage 仍执行原来的外层检查及转入 SendMessage 的第二次检查。旧手工 ABI、arity、DWORD 和普通名称执行分支由唯一契约替代。
