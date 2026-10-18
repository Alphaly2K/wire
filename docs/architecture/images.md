# 图标与光标资源边界

状态：8 项契约和资源 owner 已接入候选，验证见[本批报告](../reports/refactoring-images-2026-10-18.md)。

`catalog/images.rs` 集中参数宽度、返回约定和 provider。PE32 两项 LoadCursor 保留 stock arrow 子集；DestroyIcon 用明确的 typed 拒绝适配保留错误 120、UNIMPLEMENTED 和 `USER32 error 120` trace；另五项 ABI-only。Win64 八项 typed 保留静态 kernel32/kernelbase/user32 与动态仅 user32 的差异。LoadImage 的 type/尺寸/flags 在 Win64 按 DWORD 截断，instance/name/image handle 保留完整指针宽度。

`user/images.rs` 的 Images 私有拥有 map、单调句柄游标、边界和实际 payload。shared 按 instance/key/kind 复用身份；destroy 验证完整 handle 与 kind，shared 不移除，owned 移除后不重复分配旧地址。销毁不重写 Cursor owner 中已选择的逻辑句柄，后续 SetCursor 继续查询同一 Images owner 的有效性。容量、地址范围和错误码保持。

资源 Key 与纯 icon group/DIB 算法归该领域，原 resource 模块保留 Key re-export。校验仍按旧次序处理 group header/count、接近 32 像素的项、payload length、DIB format/dimensions/planes 和 bitmap/mask 长度；unsupported format 与 malformed resource 保持不同结果。

Win64 facade 按 typed ID 协调资源读取、模块身份、owner 和 LastError，原 PE 资源树遍历保留。PE32 适配只接 Args/LastError；两端旧名称分发及重复 ABI/arity/DWORD 删除。未增加绘制、缩放、文件加载、32 位图标或资源光标能力。
