# SRW、条件变量与 InitOnce 边界

新 Wasm 入口通过 `crates/wire-wasm/src/win32/inline_sync.rs` 接入十四项服务。PE32 接受 kernel32，Win64 接受 kernel32/kernelbase；调用参数、返回值与指针分别按对应 ABI 处理。共享契约目录中的旧执行策略仅用于旧入口，不代表新运行时的接入状态。

SRW 的独占 owner 与共享计数由进程持有，guest token 宽度为四或八字节；先验证 token、所有权、溢出和完整写权限，写回后才提交状态。共享可与已有读者并行，独占要求没有 owner 或读者；Try 竞争返回 FALSE，普通 Acquire 合作阻塞。共享持有者不逐线程记账，不提供额外公平性保证；非法释放和外来 token 明确拒绝。

InitializeConditionVariable 写入指针宽度的零；非零外来 wait-list token 或带活动等待的重新初始化明确拒绝。SleepConditionVariableSRW 验证 flags、token 和释放权限，SleepConditionVariableCS 要求当前线程恰好持有一层临界区；全部检查完成后，在同一合作调度步骤释放锁并登记等待。两类等待共用登记顺序，等待记录按加载/回调层级保存，嵌套服务不会覆盖外层调用。

Wake/WakeAll 标记当前未超时、未通知的匹配等待，不积累信号；single 选择最早登记者，all 标记全部，包含暂时处于外层回调的等待。已经超时的等待不消耗后来的 wake；先收到通知的等待在随后超过 deadline 时仍成功。唤醒或超时后必须重新取得原临界区或原共享/独占 SRW，再返回。成功保留 LastError，超时返回 FALSE/1460，非法 flags 返回 FALSE/87；零超时保留锁并直接返回超时。

有限正超时保留首次登记的 deadline，使用统一单调 10 MHz 来源；通知或已确认到期后不继续采样时间。重获锁或错误输出故障保留已释放的锁状态、原参数与返回帧，修复后继续。无可运行线程时返回 Waiting，后续宿主恢复或线程进展重试，不在 Wasm 内忙等。阻塞和故障不计 work；完成计一 work，不计客体指令退休。

InitOnceBeginInitialize 支持同步初始化和 CHECK_ONLY；初始化失败可用 INIT_FAILED 还原待初始化状态，竞争者随后可以取得初始化权。已完成状态返回完整指针宽度的 Context，要求低两位为零；异步 flags 返回错误 50，非法 flags/context 返回 87，未完成的 CHECK_ONLY 返回 31。输出先统一预检，再按 state、pending、context 的顺序发布，保留输出别名顺序与保存的返回帧。同步竞争不读取平台时钟。

便携的 SRW 账本与 condition 等待规则分别位于 `src/win32/kernel/srw.rs` 和 `src/win32/kernel/condition.rs`，均不持有 CPU、客体内存或宿主锁。Critical section、地址等待及线程对象使用各自的服务入口，并共用同一调度器。
