# VARIANT 与属性值的资源边界

Wasm 入口在 PE32/AMD64 的 oleaut32 提供 VariantInit、VariantClear、VariantCopy、VariantChangeType，以及对应的 ordinal 8、9、10、12。PE32 记录为 16 字节，AMD64 为 24 字节，payload 指针从偏移 8 开始并保留完整架构宽度。参数和返回遵循各自 ABI，VariantChangeType 忽略 flags，目标类型取低 WORD。

VariantInit 清零完整记录。支持的 scalar tag 为 0..7、10、11、14、16..23，复制保持 reserved 和 payload 原字节。VariantClear 只清 tag，BYREF clear 保留所指资源。VT_VARIANT、VT_UNKNOWN、VT_DISPATCH 等未支持的 managed 类型在清理时返回 DISP_E_BADVARTYPE，在复制或转换时返回 DISP_E_TYPEMISMATCH；没有新增数字或字符串类型转换。旧入口把 tag 12/13 按原字节复制/清理的行为不用于新入口，因为它缺少相应的 COM 引用管理。

BSTR 复制使用与同 ABI 的 SysAllocString 系列相同的分配器和成员 owner，执行深拷贝；自复制也创建新 BSTR。PE32 使用共享进程堆，AMD64 使用独立运行时数据页，两种 ABI 的复制都保留奇数字节 BSTR 的完整字节长度。VT_EMPTY 到 VT_BSTR 创建 owned 空串。先取得 replacement，再清理旧目的资源；目的 tag 读取、类型检查或清理失败时回收 speculative BSTR。完整输出在修改前预检，目的记录不得与即将释放的 BSTR allocation 重叠。

VariantCopy 和 VariantClear 另支持 owned SAFEARRAY，深拷贝或释放完整数组及其 scalar/BSTR/VARIANT 子元素；BYREF 数组保留借用关系。清理前检查整张所有权图和锁，失败复制回收已经创建的对象；目的记录不得位于即将释放的数组存储中。VariantChangeType 保留原转换子集，数组创建和各元素契约见 [SAFEARRAY](safe-arrays.md)。

成功保持 LastError。语义错误同时返回原 HRESULT/错误码并写当前线程 LastError；非法访存保留 CPU、已有资源和输出，修复权限后可重试。返回帧先保存，结果可以覆盖原返回槽；成功计一 work、零客体退休。

共享 VARIANT 处理体位于 `src/win32/com/variant.rs`，两种 ABI 的字符串 owner 分别位于 `automation32.rs` 和 `automation64.rs`。PE32 WMI 的调用者继续使用同一 owner；WMI 对象和 COM 方法的接入状态由接口台账单独记录。

## PROPVARIANT

ole32/combase 的 PropVariantCopy 和 PropVariantClear 在两种 ABI 使用独立的属性值规则；记录宽度仍为 16/24 字节，指针保持完整位宽。CoInitialize 不构成这两个内存操作的前置条件，HRESULT 成功及失败均保持 LastError。

Copy 先保存源记录并独立复制其拥有的成员，再一次发布输出；目标可以是未初始化存储，既有目标资源须由调用者事先清理，函数不会替调用者释放它们。同地址复制为空操作。Clear(NULL) 成功；成功清理会将整条记录清零为 VT_EMPTY。BYREF 不是受支持的 PROPVARIANT 类型：顶层非法类型在 clear 时清零并返回 STG_E_INVALIDPARAMETER，copy 返回 DISP_E_BADVARTYPE 且保持目标。

支持数值、BOOL、DECIMAL、FILETIME、BSTR、LPSTR/LPWSTR、CLSID、BLOB/BLOB_OBJECT、CLIPDATA，以及对应的 primitive/BSTR/字符串/CLIPDATA/嵌套属性 vector。字符串和二进制数据保持原字节及终止符；CLIPDATA 的 cbSize 包含四字节格式字段。普通字符串、GUID、二进制和 vector 存储由同一 COM task owner 分配和释放；BSTR 与 owned SAFEARRAY 复用已有成员和深拷贝规则。接口、stream/storage（含 VERSIONED_STREAM）的 NULL 值可以复制/清理；非 NULL 值需要尚未实现的 AddRef/Release，明确返回 E_NOTIMPL 并保持记录。

释放前检查整张属性图的 task/BSTR/数组身份、重复拥有、环、锁、存储跨度和输出重叠；失败保留原记录及全部成员。每个 task slab 在检查阶段即验证最终解除映射的资格，避免多个成员依次释放时最后一项才失败。复制失败回收已经创建的临时成员，发布前也检查新存储与输出的重叠。输出、源或返回帧故障保留原 CPU/work，并允许修复后重试。

属性 vector 的嵌套深度上限为 32，最多 4096 个属性节点/临时分配根，单次 task payload 复制合计不超过 16 MiB；BSTR/SAFEARRAY 的内部对象与数组深度沿用各自额度。外来分配器和非 NULL COM/stream 接口的引用回调尚未接通。

类型和所有权规则参考固定 Wine 10.0 的 combase 属性值实现，使用 LGPL 2.1 并保留原署名；当前存储检查替代原生指针。公开契约：[PropVariantCopy](https://learn.microsoft.com/en-us/windows/win32/api/propidl/nf-propidl-propvariantcopy)、[PropVariantClear](https://learn.microsoft.com/en-us/windows/win32/api/propidl/nf-propidl-propvariantclear)。
