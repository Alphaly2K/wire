# 图标与光标资源边界

新 Wasm 入口在 PE32 与 AMD64 通过 user32.dll 接入八项图标/光标接口，参数宽度和返回约定集中在 `catalog/images.rs`。LoadImage 的 type/尺寸/flags 按 DWORD 截断，instance/name/image handle 保留客体指针宽度。旧入口的 provider 和 PE32 arrow 子集仍由原适配器维护。

新旧入口共用 Images owner 和加载规则。shared 按 instance/key/kind 复用身份；destroy 验证完整 handle 与 kind，shared 不移除，owned 移除后不重复分配旧地址。销毁不重写 Cursor owner 中已选择的逻辑句柄，后续 SetCursor 查询同一 Images owner 的有效性。新入口每进程最多 4096 个活动记录、4096 个累计身份及 16 MiB payload/名称，超限返回错误 8；进程清理释放全部记录。

PE32 图像身份使用独立于 GDI stock/动态对象的范围，错误类型的销毁操作不能混淆两类对象。

资源 Key 与纯 icon group/DIB 算法归该领域，原 resource 模块保留 Key re-export。校验仍按旧次序处理 group header/count、接近 32 像素的项、payload length、DIB format/dimensions/planes 和 bitmap/mask 长度；unsupported format 与 malformed resource 保持不同结果。

资源树遍历共用有界读取代码，使用当前装载模块的资源目录、客体 VMM 与完整指针，支持 PE32/PE32+、数字和 A/W 名称。新入口将畸形目录/group/DIB 返回为错误 13，缺失资源为 1814，不支持的 DIB 格式为 50；实际访存故障保留原调用帧和资源身份。shared 资源保存实际 payload，重复加载不重读原数据。模块卸载完成时取消其 shared 缓存键；已加载句柄继续拥有原 payload，后续模块重用同一基址时重新读取资源。无效或已卸载模块返回错误 6。当前支持逻辑 stock 图标/光标及无压缩 32bpp DIB 图标资源；绘制、缩放、文件加载和资源光标仍待实现，相关请求明确失败。
