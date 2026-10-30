# 对象与混合等待

新 Wasm 入口在 PE32 与 Win64 均实现 WaitForSingleObject、WaitForSingleObjectEx、WaitForMultipleObjects 和 WaitForMultipleObjectsEx。接受具有 SYNCHRONIZE 权限的同组进程、远程初始线程、本地线程、事件、信号量、互斥体和可等待定时器；当前进程/线程 pseudo handle 分别按进程/线程对象解析。Ex 的 alertable=TRUE 支持[用户 APC](thread-control.md)：参数与目标先验证，已排队 APC 优先于信号获取及超时，投递完成返回 WAIT_IO_COMPLETION。

multiple 数量为 1–64，数组元素使用客体指针宽度。先捕获并验证所有对象与权限，再选择结果；无效句柄返回错误 6，权限不足返回错误 5。wait-all 拒绝重复对象身份，包括同一对象的不同别名，返回错误 87。wait-any 从低索引选择首个就绪对象，wait-all 只有在全部就绪后才统一消耗信号和取得所有权；不因部分就绪提前减少信号量或清除事件。

已退出线程、signaled event/timer、正计数信号量、无 owner 或当前线程持有的互斥体为就绪。进程在运行期间保持未就绪，退出后就绪；句柄关闭不改变已挂起等待的进程身份。普通 wait-any 返回 WAIT_OBJECT_0 加索引，wait-all 返回 WAIT_OBJECT_0；取得 abandoned mutex 的 wait-any 返回 WAIT_ABANDONED_0 加所选索引，wait-all 返回 WAIT_ABANDONED_0。活动线程只有完成通知和资源回收后才就绪。

零超时在对象未就绪时返回 WAIT_TIMEOUT，有限正超时从第一次调用固定 deadline，使用与 QPC 相同的单调 10 MHz 来源。INFINITE 不建立超时 deadline。仅当有限时间或目标 timer 的活动 deadline 需要推进时采样时间；普通线程、事件、信号量和互斥体的零超时/INFINITE 等待不采样。到达超时的同一次检查仍先尝试取得就绪对象。

阻塞不完成返回帧、不修改 CPU、LastError 或输出，也不计 work。参数、返回地址、对象身份、all/any 模式及首次 deadline 随线程保存，修改原调用帧或数组不会重置挂起调用。等待引用独立于客体句柄，关闭别名不会删除等待目标；完成、线程退出、启动拒绝及外部直接编辑活动 CPU 上下文会释放引用。未引用的已退出线程身份和对象随后可以回收。

运行循环优先执行其他可运行线程。线程退出、服务推进、时间片结束及宿主恢复运行提供重试机会；所有线程均无法推进时返回 Waiting。后续 run 或其他线程推进时重新取时间，核心不调用宿主 sleep，也不忙等消耗预算。run(0) 不采样或推进等待。成功或错误返回计一 work，不计客体退休。

MsgWaitForMultipleObjects/Ex 共用上述对象身份、引用与获取规则，并添加当前线程的消息条件。对象数为 0..63，数组为空时不访问数组指针；wait-all 需要全部对象和消息同时就绪。MWMO_INPUTAVAILABLE 允许旧排队输入，默认只接受尚未检查的新输入；完整分类/返回值和消息计时器规则见[消息等待](messages.md)。有限 timeout、活动可等待 timer 或包含 QS_TIMER 且有活动消息计时器时才需要单调样本。

线程执行、APC 和加载锁规则见[线程生命周期](thread-control.md)，Sleep 和让出见[延迟协议](delays.md)。

SignalObjectAndWait 接入两种 ABI。先验证返回帧和等待目标，再发信号：event/semaphore 要求 modify 权限，mutex 要求当前 owner 且只释放一层递归；满 semaphore 返回 298，非 owner mutex 返回 288，其他无效对象返回 6。后续等待复用相同 APC、引用和 deadline；挂起调用在首次 signal 后立即保存，阻塞重入或 APC 准备故障修复均不重复 signal。当前线程调度边界保持 signal 到 wait 的连续提交。`node crates/wire-wasm/tests/signal-and-wait.mjs` 使用信号计数检查单次提交和故障恢复。
