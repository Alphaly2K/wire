# 文件版本资源边界

六项 GetFileVersionInfoA/W、GetFileVersionInfoSizeA/W、VerQueryValueA/W 的声明位于 `win32/catalog/version_resources.rs`，仅在 PE32 version.dll 使用 typed 绑定，Win64 保持 absent。四参数入口保留 16 字节清栈，size 查询保留 8 字节；原六条 ABI override 随契约接入删除，ordinal 来源不变。

`win32/kernel/version_resources.rs` 只解析原始 PE32 文件字节和 VS_VERSION_INFO 节点，不访问 guest 内存或 ApiState。原 resource 目录层级、边界检查、首个 name/language 选择、大小限制、UTF-16 key 和子节点遍历保持。

`win32/x86/version_resources.rs` 只借 GuestMem、受限 GuestFs 和 LastError。文件查询仍先读取/解析资源，再写 handle 输出；VerQueryValue 先预检 pointer/length 输出，再清零 length，然后读取 blob 和路径。错误保留已清零的长度，指针与长度按原顺序提交，输出覆盖返回槽时仍读取被覆盖的返回值。ANSI 文本转换继续写入调用方扩展 buffer，不单独分配资源。

独立基线 `32fa8e3` 覆盖缺失文件、输出预检、部分失败、LastError、provider 和返回槽别名；既有原创 PE fixture 覆盖固定版本、translation 和嵌套 A/W 文本。普通 parser/adapter 与冷元数据迁移不重复性能或游戏矩阵。

原 parser 与输出处理体归一核对保持。release 完整 Win32 253、Win64 modules 13、catalog 一致性及 fmt/release lib/bin strict Clippy/build 通过。证据为 `wire-out/refactoring-2026-10-14/r2-version-resources32-{source-checks,verification}.json` 及 baseline/validation 日志。
