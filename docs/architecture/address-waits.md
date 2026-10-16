# 地址等待与线程状态边界

三项 WaitOnAddress/WakeByAddressSingle/WakeByAddressAll 的 provider、参数和控制 ID 集中到 `win32/catalog/address_waits.rs`。Win64 接受 kernel32、kernelbase 和 api-ms-win-core-synch-l1-2-0；SIZE_T 保持完整 64 位，只有 timeout 截断为 DWORD。PE32 标记 absent，继续走原有未知 ABI、零参数清栈与 error 120，不将名字已收录推成 ABI 已知。

冷绑定得到 `ControlImport::AddressWait`。Runtime64 负责保存返回帧、按 ID 调用线程 owner、保留 Blocked 或公共返回路径的 CPU/unblock/trace/API count 提交。原 address helper 从 `impl Runtime64` 移入 `threads/address.rs` 的 `impl Threads`，只借线程状态、受检 Memory 和当前 LastError 槽；不再借总运行时。序号保持私有，阻塞注册也由线程 owner 执行，旧 Runtime64 block facade 供其它尚在迁移的调用继续使用。

行为顺序保持：首先验证 SIZE_T 为 1/2/4/8；若已有地址等待，先校验地址 identity，再看 notified 和 deadline，重试不读取比较内存。首次等待依次读取当前值和比较值，值不同立即成功；值相同且 timeout 为零返回 1460。序号 checked_add 先于 scheduler 注册，注册成功后才推进序号，重试保持原 deadline。

唤醒按未通知、未退出、未过期且地址相同的最早 ticket 选择单个等待者；WakeAll 标记所有有效匹配者。没有等待者时不保存 wake credit，Wake 不读取 guest 地址。所有内存故障、LastError、到达顺序及真实线程行为维持独立旧行为基线，见[验收记录](../reports/refactoring-address-waits-2026-10-17.md)。
