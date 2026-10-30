# 系统信息与内存状态

新 Wasm 入口支持 GetSystemInfo、GetNativeSystemInfo、GlobalMemoryStatusEx 和 GetLogicalProcessorInformationEx，两种 ABI 均可调用；GlobalMemoryStatus 当前仅支持 PE32。PE32 provider 为 kernel32，Win64 为 kernel32/kernelbase。另支持 GetDriveTypeA/W 和 GetSystemPowerStatus，两种 ABI 均接受 kernel32/kernelbase。签名与返回约定由共享 catalog 声明。

SYSTEM_INFO 严格写出 36/48 字节，报告一个虚拟处理器、4096 字节页和 65536 字节分配粒度。PE32 地址范围为 `0x10000..0x7ffeffff`，AMD64 为 `0x10000..0x7ffffffeffff`，均含上界。处理器家族和版本与虚拟 CPUID 一致，GetNativeSystemInfo 在 PE32 VM 中同样报告 x86；查询不暴露宿主架构。

GlobalMemoryStatusEx 要求调用者先将 dwLength 设为 64，否则返回 FALSE、写 LastError 87 并保留结构。成功输出 64 字节；GlobalMemoryStatus 输出 32 字节，各 SIZE_T 使用 PE32 宽度。统计来自唯一 VMM：物理/提交额度均为 2 GiB，已提交页减少可用物理/提交额度，纯保留只减少可用虚拟空间。解除提交和释放沿同一 owner 恢复统计，保留范围可大于 4 GiB。扩展可用内存为零。

GetDriveTypeA/W 查询虚拟 C 卷：NULL 使用当前目录所在卷，返回 DRIVE_FIXED（3）；路径要求结尾斜线并指向 VFS 中现存目录。不存在的卷、UNC、无效路径、文件和不含结尾斜线的输入返回 DRIVE_NO_ROOT_DIR（1）。A/W 输入沿公共受检字符串读取，保留 LastError。运行时没有其他卷或挂载点；磁盘容量查询尚未实现。

GetSystemPowerStatus 写完整 12 字节虚拟桌面状态：AC 在线、无电池、未知剩余百分比（255）、节电关闭，剩余和满电寿命均为 DWORD_MAX。该状态属于虚拟设备 profile，与宿主电池无关；成功保留 LastError，跨页输出失败不发布部分结构。字段约定依据 [SYSTEM_POWER_STATUS](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-system_power_status)。

GetLogicalProcessorInformationEx 支持 core、NUMA node、cache、package、group、NUMA node Ex 和 All。单 CPU 的 affinity mask 为 1，group 和 node 为零；不发布 cache。All 依次返回 core、原 NUMA、package 和 group，合计 208/224 字节。普通记录为 44/48 字节，group 记录为 76/80 字节。单独查询 cache 成功并将长度置零；不支持的关系返回 FALSE 和错误 87。关系参数按 DWORD 截断。

长度指针是受检的 DWORD 输入/输出。容量不足时只发布所需长度和错误 122，不访问结果缓冲区；发布前预检长度和错误槽。容量充足时先验证整个输出，再写记录和实际长度，允许结果与长度重叠。调用者应先以零容量查询所需大小，再提供缓冲区。

两种 ABI 在输出前保存参数和返回帧，输出覆盖调用帧不会改变返回目标；故障保留 CPU、work 和退休数，修复后可重试。成功保留 LastError，每次完成计一 work、零客体退休，且不采样时钟。命令行与能力查询见[进程启动信息](process-startup.md)。

旧入口仍保留独立的 PE32/Win64 适配器作为迁移来源。旧 PE32 系统信息使用原有内存额度、输出错误 87 和未保存返回槽规则；旧 Win64 复用同一便携拓扑记录编码。新入口不调用旧执行器或旧进程状态。

GetDiskFreeSpaceExA/W 使用 VFS 校验目录，NULL 使用当前目录；目录存在时因缺少磁盘容量/quota 模型返回零/50，普通文件为 267，缺少目录为 3。三个可选 64 位输出均保持，不把文件缓存/私有写入额度作为磁盘容量。

GetCurrentProcessorNumberEx 写四字节 group 0/number 0/reserved 0。FlushProcessWriteBuffers 在当前单 agent、普通 Wasm memory 的执行模型下为空操作，没有跨 agent 共享 CPU 内存。

GetLogicalDriveStringsA/W 只发布 C 根的双 NUL 列表：容量不足五单元时返回五且不访问输出，成功写五单元并返回四。GetVolumeInformationA/W 仅接受 VFS C 根，NULL 使用该根；label 为 Wire，文件系统为 WireFS，serial 与文件身份一致为 1，component 上限 32765，flags 为 CASE_PRESERVED_NAMES|UNICODE_ON_DISK。可选输出统一预检；短文本容量返回 234，缺尾斜线 123，非根目录 144，其他驱动器/路径保留 VFS 错误。容量/quota 模型仍缺失。

VerLanguageNameA/W 接受 kernel32/kernelbase 与 version 的公开导出，提供 neutral、English、Japanese、en-US、ja-JP 和 invariant 的英文显示名；未知 LANGID 或零容量返回零，其他输出截断并终止，返回复制单元数。没有通用本地化名称数据库。新增查询的双 ABI、容量、失败和正式导入验证使用 `node crates/wire-wasm/tests/managed-runtime.mjs`。
