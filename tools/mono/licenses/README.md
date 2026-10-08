# 候选运行时许可材料

固定来源见上级 `source-lock.json`，此处保存上游文本，不改写许可条款。

- `Mono-LICENSE.txt`：Unity Mono 仓库的总体范围、MIT 及所列第三方许可。
- `Mono-Microsoft-Patent-Promise.txt`：上游承诺的原始文本，适用范围按文本本身判断。
- `BDWGC-notices.txt`：锁定 Unity BDWGC README 的 Copyright & Warranty 部分。
- `libatomic_ops-LICENSING.txt`：区分 permissive 核心与 GPL 的高级 stack/malloc 设施。

当前实际对象/源码/头文件由 `audit_build.py` 记录；它验证未选择 atomic GPL
设施，不会替代逐文件版权、其他 BSD notices、SDK corelib 分发权和 LGPL 履约。
完整的固定归档和补丁必须与最终打包物的构建清单一起保留。
本原型不分发 Mono 二进制、SDK corelib、UnityPlayer 或游戏资产。
