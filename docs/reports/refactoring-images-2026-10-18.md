# R2/R3 图标与光标资源（2026-10-18）

状态：五项独立旧行为基线通过，尚未迁移运行时，不计入已验收 447 exports。

8 项候选：LoadIconA/W、LoadCursorA/W、LoadImageA/W、DestroyIcon、DestroyCursor。PE32 仅 stock arrow 加载成功，其余六项错误 120；DestroyIcon 的既有 `USER32 error 120` 和 UNIMPLEMENTED trace 单独保持。Win64 八项保留 typed 子集，静态 kernel32/kernelbase/user32、动态仅 user32。

独立测试在未迁移的 `24d918d` 运行时通过，固定 PE32 返回/错误/清栈/trace、Win64 provider、DWORD 高位截断与完整句柄、shared/owned 生命周期、销毁不重写当前选择、invalid module/name/尺寸检查顺序、guest fault 不提交 CPU/API。原创最小 PE 图标资源测试损坏 group header/count 和 DIB dimensions 后仍保留首个 handle；既有真实原创 PE resource 回归继续保留。证据 `r2-images-baseline{32,64}-tests.txt`。

原创可选 `gateway-images` 重复 LoadCursorA(NULL, IDC_ARROW)，只复用一个共享资源，消费完整返回句柄并按 carrier 宽度累加 checksum。旧 cursor 冻结双架构 12K 实跑通过，Win64 退休 6n+10/work 7n+10，PE32 退休 6n+7，API 均为 n；PE32 raw/IR 均通过，工具 20 项通过，默认 case 集合保持。证据 `r2-images-fixture-baseline{32,64}`。

before 为 `r2-cursor-wire/wire`，SHA256 `bffc882421f25ea6be0ee82b23c4ebe2383bda4c8736fb10e6312bb89f4bd59c`，5,945,400 bytes。完整文件体积预算 +24KiB。证据根 `wire-out/refactoring-2026-10-14/`。
