# 进程错误模式与启动属性

新 Wasm 入口在 PE32/AMD64 均接入 GetErrorMode、SetErrorMode、InitializeProcThreadAttributeList、UpdateProcThreadAttribute 和 DeleteProcThreadAttributeList，可通过 kernel32/kernelbase 静态绑定及动态查询。参数使用客体 DWORD、DWORD_PTR 和 SIZE_T 宽度，成功调用计一 work、不增加退休指令、不采样时钟。

进程默认错误模式由当前进程拥有，所有线程共享，初始为零。接受 mask 0x8007，SetErrorMode 返回此前模式，SEM_NOALIGNMENTFAULTEXCEPT 一旦置位便保留；其余未知位返回零并设置错误 87。线程覆盖值独立保存，初始为零，进程模式变更不改写线程值；SetThreadErrorMode 的合法 mask 为 0x8003，进程专用的对齐标志在该线程接口返回错误 87。线程旧值输出成功后才提交新值，见[线程属性](thread-properties.md)。系统错误对话框与 Windows Error Reporting 尚未接入。

属性列表由调用者分配并按不透明数据使用。先用 NULL 列表查询容量，函数写出所需 SIZE_T，返回 FALSE/错误 122；该查询不读取容量的初值。非 NULL 初始化读取容量，成功后仍写出实际所需大小。输出、容量槽及失败时的 LastError 统一预检，跨页或 guard 故障不留下部分列表。容量为 0–32 项，超过限制返回错误 50；保留 flags 必须为零。

UpdateProcThreadAttribute 支持以下元数据形状。lpValue 仅登记完整地址，由调用者保持有效直到列表销毁；注册不复制或解引用值，也不取得句柄引用。父进程权限、可继承句柄及策略应用需要在进程创建消费这些值时验证。

| 属性 | cbSize |
| --- | --- |
| PARENT_PROCESS | 一个客体 HANDLE |
| HANDLE_LIST、JOB_LIST | 客体 HANDLE 宽度的整数倍 |
| GROUP_AFFINITY | PE32 为 12 字节，AMD64 为 16 字节 |
| IDEAL_PROCESSOR | 4 字节 |
| MITIGATION_POLICY | 4、8 或 16 字节 |
| CHILD_PROCESS_POLICY | 4 或 8 字节 |
| MACHINE_TYPE | 2 字节 |

未知属性返回错误 50，已知属性长度不合要求返回 24；列表满返回 31，重复属性返回 698。dwFlags 必须为零，lpPreviousValue 与 lpReturnSize 必须为 NULL。更新预检完整列表写入范围后提交，故障恢复不会重复添加。DeleteProcThreadAttributeList 使非 NULL 列表失效，调用者随后归还自己的存储；NULL 删除不访问内存。删除成功不修改 LastError。

CreateProcessW 与子进程执行尚未接入，属性列表的父进程选择、句柄继承和缓解策略尚无进程创建消费者。当前支持列表的准备、更新和销毁，不接受子进程启动请求。进程句柄与查询见[进程控制](process-control.md)。
