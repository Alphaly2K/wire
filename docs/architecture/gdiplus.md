# PE32 GDI+ 契约

新 Wasm 入口在 PE32 绑定 gdiplus.dll 的 617 项已声明 stdcall 签名，静态导入和 GetProcAddress 使用同一资格。声明与清栈宽度复用仓库 Wine spec，包含按值 GUID；未知名称、未声明序数及 AMD64 入口拒绝绑定。已知签名不表示已实现对应绘图功能。

启动、分配和矩阵的 25 项原处理分支由新旧入口共用；其余 592 项返回 NotImplemented（6），不写入调用者输出或创建对象。GdiplusStartup 接受版本 1/2，返回进程拥有的单调 token，GdiplusShutdown 解除对应 token；抑制后台线程所需的客体 notification hooks 尚未接通，有效请求返回 6。GdipAlloc 返回清零的进程堆存储，失败返回 NULL；GdipFree 仅释放该 owner 登记的分配。

矩阵支持创建、三点矩形映射、克隆、读取/设置、比较、求逆，以及 prepend/append 的乘法、平移、缩放、剪切、旋转。矩阵使用原六个 f32 的仿射布局，保留 Wine 的运算顺序、求逆阈值、逐位相等和整数点舍入；点和向量变换区分平移项，整数结果溢出或非有限时保留 INT_MIN 规则。接口返回 GpStatus，原始 float 参数经 PE32 栈传递，不修改客体浮点状态或 LastError。

matrix、普通 allocation 和 startup token 分开登记；矩阵必须通过 GdipDeleteMatrix 释放，未知矩阵返回 InvalidParameter（2）。进程内线程共享 owner，VM 之间隔离。静态启动拒绝清理 owner 及进程堆 backing；进程退出清理元数据，最终内存快照保留至 VM 销毁。

参数与返回帧在输出前捕获。矩阵创建预检输出后才分配，写入成功后发布成员；startup 输出成功后才发布 token。点变换在第一处写入前检查整个批次的读取和写入范围，故障后修复重试不会重复变换前缀。NULL 必需指针返回 2；非 NULL 访存故障交还 JIT，保持 CPU、work 和未完成的资源操作。成功完成的服务计一个 work，不增加客体退休数。

共享处理体位于 `src/graphics/gdiplus.rs` 和 `src/graphics/gdiplus/calls.rs`，新适配器仅连接当前 VMM 与进程堆，不引入旧 CPU、IR 或原生图形宿主。图像、字体、画刷、文本、effect 和其余 GDI+ 绘制能力仍未实现，详见 [接口台账](../api-migration.json) 的逐项能力记录。

`node crates/wire-wasm/tests/gdiplus.mjs` 使用稳定项目输入，检查矩阵结果、对象寿命、真实 JIT/PE 导入、动态查询、不同 stdcall 宽度、失败状态、别名及故障重试，不依赖游戏或本机数据。
