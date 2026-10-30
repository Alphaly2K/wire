# Local/Global 内存调用边界

新 Wasm 入口在两种 ABI 支持 Alloc、ReAlloc、Free、Size、Lock、Unlock、Flags 的全部十四个固定内存 export。PE32 provider 为 kernel32，Win64 为 kernel32/kernelbase，签名由共享 catalog 声明。两种名称使用同一进程堆与分配身份，固定块可由 HeapSize/HeapFree 等对应进程堆操作处理；不为每个小块建立独立页面。

Alloc 接受固定标志 0 和 ZEROINIT（0x40），返回地址即固定句柄；可移动句柄及其余标志当前返回零和错误 50。Size 返回逻辑请求尺寸，Lock 验证块成员后返回原地址，Flags 对固定块返回零；无效句柄返回错误 6，Flags 的失败值为 0x8000。

ReAlloc 接受 0、2、0x40、0x42。固定块原址增长失败且未设置位 2 时返回零，保留原块和 LastError；位 2 允许移动固定块，成功后返回新地址。ZEROINIT 清零增长部分，复制保留原有内容。尺寸、地址与失败 Free 的返回值使用完整指针宽度；标志按 DWORD 截断。Free(NULL) 成功返回零，失败 Free 返回输入句柄并写错误 6。

固定块的两个 Unlock 入口有不同的公开约定：[GlobalUnlock](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-globalunlock) 返回 TRUE 并保留 LastError；[LocalUnlock](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-localunlock) 返回 FALSE 并设置 ERROR_NOT_LOCKED（158）。固定块不增加锁计数。新入口按实际 export 身份选择该行为。

分配额度、对齐、空闲区复用、页寿命与[堆契约](heaps.md)一致。所有线程共享分配，线程退出不释放块；静态启动拒绝回收整组堆。调用参数和返回帧在操作前保存，清零/复制受检后才发布分配记录，访存故障可重试；成功完成计一 work、零客体退休，不采样时钟。

旧 PE32 适配器与旧 Win64 的独立 local-block/page owner 仍作为迁移来源保留。可移动句柄、锁计数、discard/modify 标志及尚未迁移的分配调用者仍待接入。
