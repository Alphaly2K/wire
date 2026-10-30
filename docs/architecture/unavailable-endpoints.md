# 可选系统后端的不可用协议

两种 ABI 共用四项 MCI、三十九项 wave 输入/输出和六项 GDI 字体资源声明。wave 与 MCI 没有设备或宿主音频后端；DirectSound 使用独立服务。

## MCI 与 wave

MCI 四项返回 MCIERR_DEVICE_NOT_INSTALLED(274)，忽略命令、字符串及输出指针，保持 LastError。

waveInGetNumDevs/waveOutGetNumDevs 返回零。waveInOpen/waveOutOpen 返回 MMSYSERR_NODRIVER(6)，普通打开清零非 NULL 的完整句柄输出；WAVE_FORMAT_QUERY 不访问该输出。格式、callback 和实例输入不解引用，不发消息、不执行回调、不触发事件。

设备能力查询返回 MMSYSERR_BADDEVICEID(2)，输出保持；waveOutGetDevCapsA/W 的零字节请求按接口约定成功且不访问输出。没有可用的 wave 句柄，因此关闭、header 准备/撤销、提交、暂停/重启/重置、位置/ID/音量/音高/速率及消息操作均返回 MMSYSERR_INVALHANDLE(5)，不改写缓冲区和输出。

四项 waveIn/OutGetErrorTextA/W 提供英文 MM 错误 0..21 和 WAVE 错误 32..35 的描述，长度以字符为单位。容量不足时截断并保留 NUL；容量零时成功且不检查错误码或输出地址；未知码返回 MMSYSERR_BADERRNUM(9) 并保持输出。只预检和写入实际返回文本，不访问声明容量中多余的空间。没有语言资源或本地化。

全部入口保持 LastError，不采样时钟。服务先保存调用帧，句柄/文本输出与返回槽重叠时仍返回原位置；跨页输出失败保持全部原字节并允许修复后重试。wave 播放、录音、设备 mapper 和后端接线仍缺失。

公开契约依据：[waveOutOpen](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutopen)、[waveInOpen](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveinopen)、[waveOutGetDevCaps](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutgetdevcapsw) 和 [waveOutGetErrorText](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutgeterrortext)。

## 字体资源

安装/卸载返回零并设置 LastError 120，忽略数据、名称、flags 和输出指针，输出保持原值。逻辑字体创建属于 GDI 家族，实际字体资源后端仍未接入。

## 证书

两种 ABI 接入七项 crypt32 声明。没有证书 store/context owner、X.509 parser、链构建、根信任库或 policy 验证；这些入口提供运行时明确的不可用协议。

| 入口 | 行为 |
| --- | --- |
| `CertOpenStore` | NULL，LastError 50；不读取 provider 或证书输入。 |
| `CertAddEncodedCertificateToStore`、`CertVerifyCertificateChainPolicy` | FALSE/50，保持 context/status 输出，不报告可信链。 |
| `CertGetCertificateChain` | FALSE/50，预检完整输出及错误槽后写 NULL 链；NULL 输出返回 FALSE/87。 |
| `CertCloseStore` | NULL store 成功且保持 LastError，其他句柄返回 FALSE/6。 |
| `CertFreeCertificateContext`、`CertFreeCertificateChain` | NULL 分别为 TRUE/空操作，保持 LastError；非 NULL 不属于任何 certificate owner，显式停止且不访问或释放数据。 |

这些协议不实现证书验证。来源为公开 SDK 的声明，以及 [CertGetCertificateChain](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/nf-wincrypt-certgetcertificatechain)、[CertFreeCertificateContext](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/nf-wincrypt-certfreecertificatecontext) 和 [CertFreeCertificateChain](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/nf-wincrypt-certfreecertificatechain) 的返回及生命周期约定。空 store 关闭沿用 Wine 10.0 的公开行为。

## ICMP

两种 ABI 接入 `IcmpCreateFile`、`Icmp6CreateFile`、`IcmpCloseHandle`、`IcmpSendEcho`、`IcmpSendEcho2`、`IcmpSendEcho2Ex`、`Icmp6SendEcho2`、`IcmpParseReplies`、`Icmp6ParseReplies`。没有 ICMP transport、请求句柄或 reply parser。创建返回完整指针宽度的 INVALID_HANDLE_VALUE 和 LastError 50；关闭和发送返回零/6；解析返回零/50。缓冲区、事件、APC 和 I/O 完成状态保持，不等待或采样时钟。WORD 请求长度保留其 ABI carrier，由具体适配器消费。

声明来自公开 `icmpapi.h`，创建失败的返回约定见 [IcmpCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmpcreatefile)。错误 50 是本运行时的无传输协议，没有创建可通信的句柄。

## UI Automation

两种 ABI 共用七项 UIA 声明，没有 UIA client、provider、跨进程 token 或事件投递后端。`UiaClientsAreListening` 返回 FALSE；`UiaReturnRawElementProvider` 返回零，不发布连接 token，不执行接口回调。`UiaHostProviderFromHwnd` 先预检并清零完整输出；NULL 输出为 E_POINTER，现存逻辑 HWND 为 E_NOTIMPL，非法完整 HWND 为 E_HANDLE。

四项 raise 为 AutomationEvent、AutomationPropertyChangedEvent、NotificationEvent、StructureChangedEvent，NULL provider 返回 E_INVALIDARG，其他调用返回 E_NOTIMPL。字符串、数组、interface 和 VARIANT payload 均不访问；PropertyChanged 的两个按值结构仍由 [ABI 捕获层](abi.md)完整复制，PE32 清栈 40 字节，AMD64 读取两个间接 24 字节记录。HRESULT、零客户端和零 token 入口保持 LastError。

公开返回约定见 [UiaHostProviderFromHwnd](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationcoreapi/nf-uiautomationcoreapi-uiahostproviderfromhwnd) 和 [UiaReturnRawElementProvider](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationcoreapi/nf-uiautomationcoreapi-uiareturnrawelementprovider)。上述 HRESULT 明确报告本运行时缺少 provider/事件服务。

## Console 与 Shell 操作

没有 console 对象、进程附着或控制事件后端。AttachConsole/SetConsoleCtrlHandler 返回零/50；ReadConsoleA/W、Read/PeekConsoleInputA/W、GetNumberOfConsoleInputEvents 与 SetConsoleMode 返回零/6，保持 buffer、计数和 mode。FreeConsole 在未附着状态成功并保留 LastError。原 GetConsoleMode/WriteConsoleW 同样返回 6，ReadFile/WriteFile 的重定向字节端点独立。没有注册 handler 或变更标准句柄。

ShellExecuteA/W 没有 verb/association 服务，返回 SE_ERR_NOASSOC(31)及 LastError 1155，不启动宿主或客体进程。ShellExecuteExA/W 返回 FALSE/1155，保持 SHELLEXECUTEINFO 和进程输出。SHFileOperationA/W 返回 120，保持记录、文件和 LastError，没有交互式或批量文件操作后端；普通 VFS 文件操作使用独立的 kernel32 接口。

## 多媒体事件

timeSetEvent 因没有多媒体 callback/event 调度器返回零，不注册回调或创建 owner；timeKillEvent 返回 TIMERR_NOCANDO（97）。两项保留 LastError，不采样时钟；可等待和 USER timer 使用独立 owner。
