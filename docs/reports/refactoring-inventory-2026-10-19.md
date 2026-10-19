# R7 离线 API inventory 职责整理

状态：已完成。解释器保持已验收 `b433c63` 的相同源码与 executable，本批只调整离线工具。

将 PE32/PE32+、延迟导入、导出与 CLR ImplMap 的受检解析提取到 `tools/inventory/pe.py`，将调用/查询/失败与 TSV 转义解析提取到 `traces.py`；`api.py` 保留目录扫描、字符串候选、ABI helper 和 JSON/CSV/Markdown 编排。原 `tools/api_inventory.py` 兼容命令入口和 fixture 的导入方式。按新文件位置修正 src 根路径定位，保持从任意工作目录扫描同一项目源码。

迁移前工具 **23 项**通过，并使用原创 PE32、PE32+、改名插件和 TSV 轨迹保存旧命令报告。迁移后八个函数/类与六个正则常量的 AST 一致，唯一允许的变化为 main 中源码根路径表达式；独立机器数据对照中，JSON 除 elapsed_seconds 外一致，CSV 逐字节一致，Markdown 除扫描耗时外一致。原输入与 ABI helper hash 保存在 `r7-inventory-inputs`，对照和 AST 证明为 `r7-inventory-proof.json`。

新增一项实际 CLI 回归：从项目外的临时工作目录扫描小型 PE，确认已知源码符号 malloc 进入字符串候选，未知符号不被升级；覆盖本次位置调整最易破坏的路径。最终工具 **24 项**通过，既有 PE/CLR 边界、provider、轨迹计数及性能 fixture 使用的旧 PE 导入均通过。日志 `r7-inventory-baseline-tests.txt`、`r7-inventory-tests.txt`、`r7-inventory-final-tests.txt`。

没有修改 Rust、build.rs、native 或游戏输入，完整运行时字节恢复证明为 `r7-cli-rollback-verification.json`，因此不重跑运行时测试和计时。本批不新增 export，不改变 ABI helper 的覆盖含义，也不执行游戏。
