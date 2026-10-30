# 窗口类注册与元数据

Wasm 入口在 PE32 通过 user32 接入十七项类注册、注销、信息、字段和名称操作，通过 comctl32 接入 InitCommonControls/Ex。AMD64 继续接入六项注册/注销；其余类接口保留旧 absent 范围。provider、参数与返回声明来自 `win32/catalog/classes.rs`。

类身份共用 `win32/user/classes64.rs`。PE32 元数据、字段规则、基本类名称和文本复制算法从旧入口提取到 `user/class_metadata.rs`，旧适配保留原输出和返回帧次序，新适配使用当前 VMM、调用帧、进程 USER 和运行时数据页。PE32 注册使用 40/48 字节记录，扩展记录检查 cbSize；过程非零，class/window extra 声明上限均为 4096。AMD64 保留 72/80 字节布局及 class extra/菜单非零时错误 50 的原子集。

PE32 记录全部十一项字段，接受数字或 A/W 菜单名；GetClassInfo/Ex 输出所选编码的持久名称/菜单指针，Ex 写出实际 cbSize。名称复制与读取调用者存储分离，普通注销后 backing 继续保持进程寿命；静态启动拒绝回收 backing。类信息输出前保存原返回帧并检查完整输出，别名或故障不会留下前缀。缺失类为错误 1411，活动窗口引用使注销返回 1412。显式注册的菜单/源/返回帧故障不消耗 atom，错误槽故障可修复重试。

Get/SetClassLong 与 GetClassWord 保留旧负索引字段和完整 DWORD carrier；只读 atom 索引返回类身份。修改实例、样式、过程、window extra 和背景 brush 会更新同一类状态，后续窗口使用新的过程/extra 大小，已有窗口保留已捕获过程和存储。class extra 仅保存声明，正索引字段访问仍返回 1413；超过 extra 上限返回 87，菜单名修改仍返回 120。GetClassNameA/W 保留 signed capacity、NUL 和 CP932 字节截断；无效 HWND 先于输出检查返回 1400。

PE32 类名最多 255 个 UTF-16 单元，宽名称保留旧入口对未配对代理项的替换，窄名称按 CP932 严格解码，再按 Unicode 小写匹配；AMD64 最多 256 个 UTF-8 字节，按 ASCII 忽略大小写匹配。重复注册返回 1410；PE32 已存在全局类阻止跨 instance 重名。atom 从 0xc000 单调分配，PE32 包含 0xffff，AMD64 在 next 为 0xffff 时拒绝；注销不复用身份。创建允许 CS_GLOBALCLASS；PE32 注销也允许全局类跨 instance，AMD64 注销要求完整 instance。

PE32 查询或创建 Button、Edit、Static、ListBox、ComboBox、ScrollBar 时按旧规则注册相应全局基本类。InitCommonControls/Ex 按既有顺序补入这六项和 SysListView32、SysTreeView32、SysTabControl32、msctls_progress32，保留已有同名类。Ex 只读取 dwSize=8，dwICC 继续忽略。每个类拥有真实元数据、16 字节 window extra 和同 JIT 的 DefWindowProcW；只有普通窗口/文本行为，列表、树、标签页和进度条的专用控件语义尚未接入。容量失败保留已经完成的有序注册。

PE32 跨 A/W 创建按类编码转换 CREATESTRUCT 标题和名称，标题保存于经过完整预检的回调栈区，长标题及故障重试沿同一续点处理。图标/光标字段已保存和查询，实际图标绘制及控件专用消息仍需对应后端。窗口生命周期和变长暂存区见[窗口控制](window-control.md)。
