# 线程创建、退出、上下文与 APC

六项 export 的 provider、参数 carrier、返回约定和 typed ID 集中在 `win32/catalog/thread_control.rs`。Win64 接受 kernel32/kernelbase；PE32 仅 kernel32，其中 CreateThread/ExitThread 使用 typed route，其余四项保留 ABI-only/error 120。内部 `wire!ExitThread` 继续单独处理 worker 返回，不作为 Windows provider。Win64 的创建 flags、退出 code 截断 DWORD，stack SIZE_T、handle、callback、APC data 和指针保持完整宽度。

Win64 冷绑定 `ControlImport::Thread`，调用时不比较 API 名称。领域结果明确区分普通 Value、Exit 和 Context。根入口先预检返回帧；普通返回保持 CPU、unblock、计数和 trace 的公共提交。Exit 只记计数和 trace，不提交 returned CPU、不清阻塞；Context 成功提交 returned CPU/BOOL 和计数，由 Process64 继续读取已保存的 guest 寄存器，保留原路径的 trace/阻塞规则。该控制结果不运行 guest CPU。

Threads 拥有私有 description 和 APC queue。设置 description 先完整读取 UTF-16，成功后替换；读取先验证 thread identity，再预检输出、编码文本，由 Runtime64 依次分配原 heap、写文本、写 pointer，最后登记 LocalFree 归属。HRESULT 错误不改 LastError。分配仍由已有 heap owner 负责，输出覆盖返回槽使用已经保存的返回帧。

APC 入队先解析完整 handle、检查线程 exit 和 256 项队列上限，随后校验 callback。registered gateway 保留准入，其余地址检查 READ|EXECUTE。错误检查先后、LastError 和完整 64 位 data 保持。alertable delivery 由 Threads 先检查整个队列的 callback，再一次 drain 成 owned callback 请求；Process64 负责执行。目标保护失效时保留整个队列，既有等待/唤醒机制继续在 owner 内查看队列。

CONTEXT 先解析 handle，要求非当前、已挂起且未退出，再检查 16 字节对齐、整个输出可写，最后读取 flags。读取成功后先记 flags trace，再校验 `0x100000` 架构位和 `0x10001f` mask；错误码和故障不提交 CPU 的规则保持。实际寄存器 copy-out 仍由 Process64 完成。

创建线程仍由 Runtime64 协调 stack、TEB、static TLS 和 Threads 的提交：先 scheduler admission、flags/security、ID 输出预检和完整 SIZE_T 上限，映射失败只回滚本次资源，成功写 ID 后才登记新线程、handle 与 pending spawn。error mode 继续继承进程值。PE32 保留另一套历史次序：忽略 security/stack，flags 只消费掩码 0x4，先分配线程/handle/TLS，最后写 ID；ID fault 已有的创建效果保留。两架构没有套用同一个创建事务。

PE32 退出继续先通知 DLL，再释放模块 TLS、清理 USER 资源，最后由调度器退出并切换 CPU；通知 callback 未正常返回时原样提前离开。普通 ExitThread 读参数，内部 worker stub 读 EAX。完整资源创建/退出编排仍借助旧 facade，后续 owner 收敛应继续按真实资源边界推进。

独立回归覆盖旧行为、故障/输出别名、队列原子取出、描述 LocalFree 归属及控制结果；既有 Process64 小/大 slice 的 CONTEXT/APC、DLL 通知和 TLS 回归继续验收。见[本批记录](../reports/refactoring-thread-control-2026-10-17.md)。
