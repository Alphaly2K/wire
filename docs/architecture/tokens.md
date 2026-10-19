# Win64 token 资源与查询边界

`x64::runtime::token::Tokens` 私有拥有 token 记录、query access、handle flags 与单调编号。Runtime64 的组合根通过 `open/query/flags/set_flags/close` 操作资源；open/query 只借 guest Memory、参数和当前线程 LastError 的 guest 地址，不再访问整个 Runtime64。SID 纯 codec 保持，OpenThreadToken 的线程身份判断仍由原调度边界负责；本批不迁移 legacy API 元数据或新增能力。

当前 token 表达原有受限 guest 用户的 query-only 身份。OpenProcessToken 只接受当前进程伪句柄和 0/QUERY access，最多 128 个同时存活的 token。句柄从 `0x7fff4000` 单调分配到 `0x7fff4fff`，CloseHandle 释放存活容量但不复用编号。错误优先级为 process handle 6、unsupported access 50、容量/编号耗尽 8，最后才检查 8-byte output。输出故障不消耗编号；输出覆盖返回地址时使用 gateway 预先保存的 frame。

GetTokenInformation 先验证完整句柄及 query access，再构造原有 class 1/8/18/20/25/29 的 guest 字节。内部指针仍指向 guest output，溢出错误先于指针检查；不读取宿主凭据。先检查 required-size 输出，短容量写 required size 再设置 error 122；足够容量时完整检查 data output，再写 data、最后写 required size。输出别名保持该顺序，guard/访存故障不提交 CPU 返回或 API count。成功保留 LastError。

GetHandleInformation 保留原资源查找顺序和 token flags。SetHandleInformation 只接受 mask 低两位，更新错误为 87；其原先提前提交返回、计数及 recent call 的路径保持。PROTECT_FROM_CLOSE 的 token 仍以 error 6 拒绝关闭，和其它资源可能不同；清除保护后才移除资源。关闭后查询立即拒绝，完整 64-bit token 身份不按低 DWORD 匹配。

三项独立测试位于 `tests/x64/token_owner.rs`，先在旧实现验证 process/access/容量/指针优先级、saved frame、flags、output alias、required-size guard、128 个存活及 4096 个累计编号的边界。原创 `gateway-token-info` 只在循环前打开一个 query token，每轮消费 BOOL、四字节 TokenElevation 与 required size。n=max(1,work/13)，RAX=5n+1、retired=12n+20、work=13n+21、API=n+1；两个小输入已在 ETW 冻结上实际校验，离线 inventory 固定两项 advapi32 导入及各自 IAT。

完整正确性、17 条性能协议及三对启动前缀已验收；保留 PE32 integer/IR 六对长样本 +3.320% 的局部成本，具体调查和全部证据见[验收记录](../reports/refactoring-token-owner-2026-10-19.md)。
