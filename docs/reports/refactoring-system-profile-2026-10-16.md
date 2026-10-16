# 系统信息与内存状态契约迁移

## 独立旧行为基线

范围为 GetSystemInfo、GetNativeSystemInfo、GlobalMemoryStatus、GlobalMemoryStatusEx 四项 export。运行时基线 `10fbe95`，before 为 `r2-local-memory-wire/wire`，SHA256 `4554773e51c5f828d21c637b2286b81c047bbb218e5272612cb90f85767ecae8`。PE32 四项 typed、kernel32 provider；Win64 前两项和 Ex typed、kernel32/kernelbase，GlobalMemoryStatus 继续 absent。

两项新增旧行为基线通过，固定 PE32 36-byte 与 Win64 48-byte SYSTEM_INFO、各架构虚拟硬件值、32/64-byte 内存状态、Ex length 校验、输出哨兵及 LastError。PE32 公共 `wire::system::dispatch` 精确接受 `kernel32.dll`，拒绝其它大小写和 kernelbase，继续保留；输出故障转 error 87，输出覆盖返回槽时 EIP 读取被覆盖内容。Win64 则先保存返回帧，受检输出故障不提交 CPU/API count/LastError。既有布局、内存记账与时钟相关回归一并通过，共五项，日志 `r2-system-profile-baseline-tests.txt` 和 `r2-system-profile-baseline-related-tests.txt`。

原创可选 `gateway-system-profile` 每轮 GetSystemInfo 后累加 architecture 和 page size，输出位于本地栈。n=max(1, work//9)，Win64 返回 4105n、retired=8n+10、work=9n+10；PE32 返回 4096n、retired=9n+9；API calls 均为 n。独立 PE inventory 确认单一 GetSystemInfo 导入及 IAT 0x1190；before 的双架构 12K smoke（PE32 raw/IR）及 20 项工具检查通过，默认 case 集合保持。日志前缀 `r2-system-profile-baseline-`。
