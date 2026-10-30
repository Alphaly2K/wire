# PE32 DirectSound 与 PCM 输出

新旧入口共用 `src/graphics/dsound.rs` 与 `src/graphics/dsound/calls.rs` 的 COM、PCM buffer、锁定、游标、通知和混音处理体。Wasm 适配使用当前 VMM、进程堆、逻辑网关、窗口和事件 owner，不依赖 AudioQueue、Presenter 或旧执行器。

PE32 接通 dsound.spec 的 14 项公开签名及数字 ordinal。DirectSoundCreate/DirectSoundCreate8 可创建对象，其余 12 项返回 E_NOTIMPL；48 个 COM 方法由 vtable 注册，内部名称不能用公开 bind 获取。Initialize 三项、DuplicateSoundBuffer 两项及 VerifyCertification 返回 E_NOTIMPL。AMD64 保留 absent。

宿主在装载后的 idle PE32 上调用 `await machine.enableAudio(options)`。自定义 `options.sink` 提供同步 `write(Int16Array) -> boolean` 与 `close()`，或通过可异步的 `options.createSink(processId)` 为各进程创建独立消费者；显式 sink 优先用于当前进程。默认使用 Web Audio。没有输出配置时 Create 清零输出并返回 DSERR_NODRIVER。默认 live context 必须能够启动；调用者也可提供 `options.context`，包括 OfflineAudioContext 离线渲染。每个 VM 同时只接受一个输出。

子进程继承 context、工厂及其余音频选项，在首次 DirectSoundCreate/8 前按需建立自己的消费者；显式 sink 对象仅属于当前进程，子进程使用工厂或默认 Web Audio。未使用音频或只调用未实现导出的子进程不初始化输出。配置可经 AMD64 中间进程继续传递；初始化失败保留服务和配置供重试。父进程先退出不关闭后代的消费者；借用的 context 只停止各 owner 自己的节点。

PCM 支持 mono/stereo、8/16 位，复用原 volume、pan、frequency、重采样、循环及裁剪规则，输出为 44100 Hz 双声道 i16。primary buffer 提供原格式/控制子集；环形 Lock/Unlock、buffer/notify 引用和最终释放保持原 owner。限额为 256 对象、单 buffer 16 MiB、总 PCM 64 MiB、1024 个通知位置。通知要求当前可修改的 event 句柄，混音后使用现有同步 owner 唤醒等待线程；已关闭的句柄被忽略。

音频只在已配置输出后的服务与外层执行边界采样，不向普通指令添加 host callback。至少累计 5ms 后混音，单次最多一秒；超出的帧计入 droppedFrames。每次混音先检查全部活动 backing，故障保留全部声部游标。pending PCM 在消费者返回 true 后确认；false 或抛异常保留同一 packet。`audioStats()` 返回 mixedFrames、droppedFrames、lockedPlaybackFrames 和 pendingFrames。

背景混音故障独立于 CPU 故障槽保存；执行结果的 `audio: true` 表示音频渲染访存失败，fault 保留其地址和访问类型，既有 CPU 异常现场保持有效。进程退出、静态启动拒绝和 dispose 退休音频 owner，并关闭消费者一次。Web Audio 使用 AudioBuffer/BufferSource，以 `/32768` 转换样本，最多排到当前时间后两秒；close 停止本 owner 的节点，只关闭自己创建的 context。

独立验证入口 `python3 tools/probes/check_wasm_audio.py --out target/wasm-audio-check` 需要 macOS SDK、clang、WKWebView 和 OfflineAudioContext，使用仓库生成的 PE 输入，不在默认测试发现路径内。它检查真实 JIT/COM、离线样本及释放；实际扬声器、iOS 设备和完整游戏音频尚待验收。
