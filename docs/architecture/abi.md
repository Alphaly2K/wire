# PE32 ABI 来源与绑定

新 Wasm 入口按 `win32/catalog` 的架构、provider、参数与返回约定绑定系统导出。Contract 默认 stdcall，cdecl 显式声明；stdcall_bytes 返回实际 callee 清栈字节数，参数捕获与清栈共用同一契约。DLL 名称按装载规则归一，导出名称区分大小写。

WMI、DirectSound 的 PE32 内部 COM 方法和 AMD64 DXGI/D3D11 方法由各自的 vtable 创建时注册，使用共享方法声明确定参数宽度及清栈；内部名称不会自动成为公开 DLL 导出。返回位置与参数在写入输出前保存，输出覆盖原栈槽时仍从已保存的帧完成调用。

按值 VARIANT 在 PE32 占 16 字节内联栈空间，在 AMD64 由对应参数位置的寄存器或栈槽传入 24 字节结构指针。捕获层把整个结构复制到固定参数快照的三个 u64 槽，PE32 的第三槽为零；后续参数仍使用原逻辑参数位置决定 AMD64 carrier。整份快照不分配堆内存，不解引用 VARIANT payload；源或返回槽故障不发布调用。`UiaRaiseAutomationPropertyChangedEvent` 的四个逻辑参数对应 PE32 的 40 字节 stdcall 清栈。

旧 `win32::abi::pe32` 保留 Wine/Hangover spec、typed i386 参数解析、ordinal 名称解析及 legacy 例外，供迁移核对。ABI 声明、旧路由和新实现资格分别列于 [接口台账](../api-migration.json)。GDI+ 在 PE32 复用完整 stdcall 声明，已支持的矩阵/启动/分配子集与其余状态 6 的失败协议见 [GDI+ 契约](gdiplus.md)。DirectSound 在 PE32 复用全部 14 项声明和 ordinal，内部方法仅由 vtable 注册，具体能力见 [音频契约](directsound.md)。CRT 的旧函数均使用已接入的 cdecl 契约，非返回退出不读取返回槽；原始数据导出使用可写存储，具体能力、未实现协议及返回约定见 [CRT 契约](crt32.md)。旧 D3D9 的签名已知不代表新入口可调用；未知名称不会采用旧 CRT 零清栈诊断策略伪造实现。

序数只按对应 provider 的已接入声明解析，不因名称路由或 legacy 例外扩大可见性。[文件版本资源](version-resources.md)与 [Windows printf](windows-printf.md)等接口的额外约束由各自领域契约说明。
