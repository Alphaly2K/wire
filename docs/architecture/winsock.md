# Winsock 启动、地址值与事件

新 Wasm 入口在 PE32 与 AMD64 共用 78 项 `WINSOCK_CONTRACTS`，provider 仅为 ws2_32.dll。经典命名与序数导入、动态查询、转发和延迟绑定共用同一服务入口；序数/名称事实来自固定 Wine 10.0 的 `third_party/wine-10.0/abi/ws2_32.spec`，公开参数宽度保留完整 SOCKET/指针和 DWORD。该声明表没有引入 Wine transport 或 namespace provider。

每个进程拥有独立的启动引用计数。WSAStartup 先预检完整 WSADATA，再写协商版本、描述及状态，最后增加引用；vendor 指针与 padding 保持。PE32 结构为 400 字节，description/status 位于 4/261，maxSockets/maxUDP/vendor 位于 390/392/396；AMD64 为 408 字节，对应偏移为 16/273、4/6/8。Cleanup 在零引用时报错；LastError 使用当前线程 TEB。线程与 DLL 共用进程状态，其他进程和 VM 独立。

当前没有 transport provider。socket/WSASocket 仍返回 pointer-width INVALID_SOCKET 和原错误；accept、connect、收发、socket 控制及事件关联在初始化前返回 10093，初始化后返回 10038，不生成网络连接、不消费或清空用户缓冲、不投递 callback/message/completion。accept 使用完整 INVALID_SOCKET，其余 SOCKET_ERROR 是 32 位整数。select 检查三个集合的计数，初始化后的空集合返回 10022，非空集合返回 10038，不等待或修改集合。WSAIsBlocking 为 FALSE；NULL hook 移除和已初始化的 Unhook 成功，非空 hook/异步名称请求返回 10045，取消不存在的异步/阻塞调用返回 10022。这些接口没有阻塞回调或异步消息后端。

地址值继续使用 AF_INET=2、AF_INET6=23 和网络字节序。inet_pton/InetPtonW、inet_ntop/InetNtopW 保留原语法、短容量和错误差异。inet_addr 另支持 BSD 一至四段地址与十进制、八进制、十六进制，NULL、空串及非法地址返回 INADDR_NONE；纯空白返回 INADDR_ANY，保留 LastError。inet_ntoa 接收按值 DWORD IN_ADDR，返回点分 ASCII；不要求启动或可用 transport。__WSAFDIsSet 按两种 Windows fd_set 布局比较完整 SOCKET，最多 64 项；空/NULL 集合返回零，超额返回零和 10022，不要求启动。其输入读取和输出提交保留精确访存故障。

gethostname/GetHostNameW 在初始化后返回虚拟 profile 的 DNS hostname，字节/UTF-16 容量含 NUL，NULL、负容量及容量不足返回 SOCKET_ERROR/10014 并保持缓冲。gethostbyname 解析此名称、localhost 和 BSD 数值 IPv4；NULL/空名选择当前虚拟主机。gethostbyaddr 只为 127.0.0.1 提供 localhost 反查；其余外部名称或地址返回 NULL/11001。hostent 使用 16/32 字节头、真实内部指针、空 aliases、单个四字节地址及结尾空指针。

传统文本和 hostent 在每线程一个 runtime data block 中使用独立位置；同一线程后续同类查询覆盖其原结果，其他线程相互隔离。工作线程退出在统一回收预检后释放该块，启动拒绝与 VM 关闭沿现有 runtime data owner 回收。成功保留 LastError，写入故障保留原内容与调用状态。getproto/getserv 系列没有数据库，初始化后返回 NULL/11004。

getaddrinfo/GetAddrInfoW/GetAddrInfoExW 支持同步数值 IPv4/IPv6、当前虚拟主机、localhost 和数字端口 0..65535。未指定族的本地查询只返回一个 IPv4 结果，显式 IPv6 地址或 AF_INET6 返回 IPv6；NULL node 按 AI_PASSIVE 选择未指定地址或 loopback。支持 hints 的 PASSIVE、CANONNAME、NUMERICHOST、NUMERICSERV，socktype 0/1/2 及 protocol 0/6/17，检查 TCP/UDP 一致性；其他 flags、非空 hints 尾部字段、族或类型分别返回明确错误。没有外部 DNS、hosts/service/protocol 数据库、IDN、地址族自动扩展、接口探测或带 zone 的输入地址解析。

普通 addrinfo 头为 32/48 字节，Ex 为 44/72 字节；每次结果独立拥有一个 runtime data block，包含单个头、16/28 字节 sockaddr 及按接口编码的可选 canonical name。最大并存 1024 项，耗尽返回 8。Ex 只支持 NS_ALL/NS_DNS，provider GUID、timeout、OVERLAPPED 或 completion callback 请求返回 10045，不创建异步句柄；可选 cancel 输出清零，与结果输出重叠返回 10022。失败先预检结果/cancel 输出与错误槽，再一并清零并设置错误；成功输出故障不分配结果。

freeaddrinfo/FreeAddrInfoW 释放普通结果，FreeAddrInfoExW 释放 Ex 结果，NULL 为无操作。释放按 owner 中的精确根地址进行，不遍历客体可修改的链接；错误类型、外来地址或重复释放明确停止并保持存储。公共 HeapFree、VirtualFree 不能释放这些结果。GetAddrInfoExCancel/GetAddrInfoExOverlappedResult 因不存在异步请求而返回 6，不访问请求结构、触发事件或调用回调。

getnameinfo/GetNameInfoW 支持 16/28 字节 sockaddr、数值地址/端口、loopback 的 localhost 反查、数值 IPv6 scope 和 flags 0..31；没有其他反向 DNS 或服务名数据库，NAMEREQD 缺少本地记录时返回 11001。输出容量含 NUL，先捕获完整输入并预检两份实际输出，再按 host、service 顺序写入；源、输出、返回帧或错误槽故障不会留下部分结果。普通返回槽可与结果输出别名，完成使用预先保存的目标。

Winsock 与 kernel 事件使用同一内核对象 owner，保留手工重置、完整句柄与访问检查，Cleanup 不关闭事件。WSAWaitForMultipleEvents 共用[对象等待](waits.md)的保存调用帧、目标引用、首次 deadline 和调度协议；启动状态及参数错误先于时钟采样。alertable 等待通过同一 JIT 投递[用户 APC](thread-control.md)，完成返回 WSA_WAIT_IO_COMPLETION。

稳定验证入口是 `node crates/wire-wasm/tests/winsock.mjs` 和 `node crates/wire-wasm/tests/winsock-names.mjs`，输入全部由项目生成，不使用网络或外部程序。源码分别为 `src/win32/winsock.rs`、`crates/wire-wasm/src/win32/winsock.rs` 和 `winsock_names.rs`。
