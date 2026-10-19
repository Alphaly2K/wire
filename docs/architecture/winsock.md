# Winsock 启动、地址值与事件

20 项 `WINSOCK_CONTRACTS` 统一 ABI、参数宽度、provider 与 WinsockApi。Win64 静态及动态仅接受 ws2_32.dll；原九项序数别名在冷绑定/lookup 阶段规范化，诊断使用原有规范名称。PE32 仅保留 WSAStartup、WSACleanup、socket 的 ABI-only 策略，closesocket 保留原 legacy ABI；其余命名与序数仍为未知 ABI，不扩大实现范围。

私有 `x64::runtime::sockets::Winsock` 独立拥有进程启动引用计数。标量调用只借 Memory、ID、参数、当前线程 LastError 地址与共享 Events owner。WSAStartup 保持 WORD 请求的原 Win64 trace carrier、408-byte WSADATA 预检、版本协商、未写 vendor/padding 及输出后增加引用的顺序。Cleanup 保留零引用错误，LastError 仍使用当前线程 TEB。

地址转换继续使用 Windows AF_INET=2、AF_INET6=23 和网络字节序；源数据读取、完整输出和错误提交顺序保持。ANSI/Wide 语法错误与输出不足的错误差异保持。当前没有 transport provider，socket/WSASocket 返回 pointer-width INVALID_SOCKET 和原错误，不生成网络连接。

Winsock 事件与 kernel 事件继续使用同一个 Events owner，保留手工重置、完整句柄与访问检查。WSAWaitForMultipleEvents 绑定专门 control ID，由原进程等待/APC/调度路径执行；等待主体仅将启动状态查询委托给 Winsock owner，没有迁移 CPU、阻塞或返回帧提交逻辑。

基线 `4b5e327` 的 20 项 arity/DWORD、九项序数和标量 handler 经字段、ID、窄借用与格式归一核对一致。相关地址转换、事件、startup refs、ordinal lookup 和故障输出继续使用既有 network 测试；新增 PE32 回归固定 ABI-only 名称、未知序数和 provider-specific 清栈。该批不改执行器或内存/cache 热路径，不做性能计时。证据位于 `wire-out/refactoring-2026-10-14/r2-winsock-*`。

相关 release network 7 项、PE32 ABI 回归 1 项、catalog 一致性 1 项、fmt、release lib/bin strict Clippy 和 release build 通过，累计 **836 exports**。源码及 executable SHA 记录在验证 JSON，不复制构建产物或源码树。
