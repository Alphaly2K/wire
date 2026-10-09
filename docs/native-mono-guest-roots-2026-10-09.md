# 暂停 guest 的 GC roots 生命周期

`src/mono/roots.rs` 为默认关闭的native后端提供受控object admission及暂停root snapshots。
这是生产provider可复用的ownership模块，尚未把Process64所有线程/寄存器/stack/static/heap
root sources接入。M3和真实游戏native后端继续关闭，synthetic fixture admission不修改game profile。

对象进入guest前立即获得一个真实Mono v2 strong handle，同时建立weak admission和不复用
ObjectToken。只有经native API/typed mapping验证的对象header/payload/byref spans才能登记，
该动作是unsafe边界，不能把未知整数传给mono_gchandle_new。spans只用于识别roots，不注册
额外Memory，也不免除mapping权限、generation或pin要求。跨对象overlap拒绝，同对象spans
合并；snapshot按有序区间索引查找，避免对每个object重扫全部guest数据。

snapshot先受检复制暂停GPR和显式登记的pointer slots。范围须完整8B槽且总量不超过16MiB，
读取任何一个range失败就拒绝整个snapshot，旧roots保持。copy完成之后才调用Mono handle API，
没有Memory borrow或native buffer lease跨越Mono API边界。已登记exact或interior pointer均能
保活其真实object。一般整数偶然落入known span时也保守保活，符合conservative root语义。

synchronize必须由provider证明覆盖全部guest持有者，并保持相关execution owners暂停，
故提供明确unsafe契约。它先取得所有新增strong handles，全部成功后才释放旧root；中途
失败会释放已取得的新句柄、保留原root集合。weak target已NULL的旧entry不会复活对象；
forget_reclaimed仅清理已回收对象的weak admission/spans，旧token不会恢复。handle registry
本身持ManagedRoot ResourceLease，先释放实际weak/strong handles、再释放VM owner。

registry为owner-only、不能Send/Sync，要求真实attached Mono线程贯穿生命周期；owner在
registry drop之前不得detach。只在Shutdown中清理owners，不能切到第二VM。它不替provider
暂停CPU、不自动扫描未知内存、不处理guest raw stores/write barrier，也不撤销native mapping。
漏扫live roots或只扫一个线程就调用synchronize是违规调用，不能用这样获得的回收/速度当成果。

真实BDWGC fixture创建三个对象，只由暂停GPR/exact pointer或栈的interior pointer保活。
创建线程保持attach，native旧帧清理后由另一线程执行16次GC：完整snapshot时三对象均存活，
完整空snapshot之后均可回收。坏range snapshot不丢旧root。另注入第二次strong获取失败：
第一次新句柄被真实free、第三个原root仍存活，独立handle tracker核对无泄漏，重试成功。
expired弱记录清理后旧token无效；registry尚在时cleanup-ready为false，释放全部真实handles
并drop registry后才可cleanup。全部M2原有行为继续通过，noexec guard checks=540、denied=0。

下一步接production safepoints及全guest根源集合，再与domain generations/shared ownership/
callback lifetime一起绑定provider。当前完成是实际root owner模块和GC闭环，不是已经启用
Unity/Naninovel或证实端到端性能收益。
