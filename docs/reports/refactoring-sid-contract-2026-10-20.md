# SID 九项契约与受检内存适配器

状态：接入完成，累计 **725 exports**。运行时基线为 `149af12` token 契约；独立旧行为先提交于 `1df4813`。九项 SID 声明集中 provider/arity/参数宽度，删除 root 中重复支持、动态排除、arity、DWORD 和字符串 handler，约百行调用逻辑移入仅借显式资源的 `win32/x64/sid.rs`。原 SID codec 除函数名/可见性和函数间空行外保持，token owner 删除该无状态 helper 后逐字一致。

保留 PE32 advapi32 ABI-only 和未知 provider 零清栈；Win64 三个静态 provider 与已注册 gateway 动态 fallback 保持。独立旧行为涵盖高位指针、BYTE 原始 trace、DWORD index、初始化不清零 subauthority，以及 InitializeSid/CopySid 各自的 guard 消费、访存故障和错误提交顺序。详见[SID 边界](../architecture/sids.md)。

风险评估：普通契约和无状态适配器抽取，不涉及执行器、memory/cache、资源布局或实际图形/启动逻辑，因此**不执行性能、采样、游戏或 native 图形复测**。release 定向 **5 passed**（Win32 1、AMD64 4），debug AMD64 定向 **4 passed**；catalog 唯一性/架构计划测试 **1 passed**，fmt、strict all-target/all-feature Clippy 与 release build 通过。上一批 release 全量 657 / 7 ignored 保留其原上下文，本批没有冒称重新全量验证。

验证文件 `r2-sid-validation-*` 与 `r2-sid-verification.json` 位于 `wire-out/refactoring-2026-10-14/`。记录全部 306 个 src Rust 文件 SHA、源码比较和构建 executable SHA256 `397fb490bbe3f969a30d03981887bf164f09d9231750d7991632d1b54f9fd85f`，体积 6,057,208 bytes。代码已在 Git 按批保存，本批不额外复制整套源码/游戏素材。
