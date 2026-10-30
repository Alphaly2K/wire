# ETW provider

Wasm 入口在 PE32 和 AMD64 的 `advapi32.dll` 接通 EventRegister、EventUnregister、EventEnabled、EventProviderEnabled、EventWrite、EventWriteString、EventWriteTransfer 和 EventSetInformation。其它 DLL 的历史私有别名不在新入口导出范围内。静态导入、动态查询和显式绑定使用相同资格。

当前 profile 没有 tracing session，所有 provider 始终禁用。注册保存 GUID 和客体 callback/context 数值，不调用 callback、不提交事件、不读取事件 payload。每个进程最多同时注册 64 个 provider，句柄从 `0x7fff6000` 单调分配到 `0x7fff6fff`，注销不复用；累计耗尽返回状态 8。进程终止和启动拒绝时清理注册。

EventRegister 先检查 GUID 的 16 字节可读范围，再检查输出的 8 字节可写范围，随后判断容量，最后读取 GUID、写出句柄并登记。source guard 优先消耗，输出故障不消耗身份。输出可与输入或返回地址重叠，返回帧使用调用前保存的值。所有状态码直接返回，LastError、FLAGS 和浮点状态保持；访存故障不提交 CPU 返回或 work。

无效句柄的 EventEnabled/EventProviderEnabled 返回 0，其它操作返回 6，并跳过后续指针。有效 EventEnabled 检查 16 字节 descriptor；EventWrite/Transfer 先检查 descriptor，再拒绝超过 128 个条目，非零 count 只检查每项 16 字节的数组范围，不跟随 payload。Transfer 不访问 activity/related 指针，EventWriteString 不访问文本，EventSetInformation 返回 50。

REGHANDLE 和 keyword 使用完整 Uint64，在 PE32 占两个 DWORD 栈槽，在 AMD64 占一个参数位置。DWORD count/info/length 按 32 位截断。注册输出在两种 ABI 中均为 8 字节。这些调用不采样时钟或随机源。

`win32::tracing_state::Providers` 直接复用原有调用处理与句柄状态，新旧入口仅适配各自的内存检查、GUID 读取和句柄写入。共享实现不依赖 CPU 或执行器。常规检查入口为 `node crates/wire-wasm/tests/etw.mjs`，使用项目生成 PE；旧入口的累计身份耗尽与既有语义回归可通过 `cargo test --locked --offline --test x64 etw_ --target-dir target/resident` 定向运行。
