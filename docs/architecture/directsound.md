# PE32 DirectSound 资源与调用边界

DSound 私有拥有 COM 对象、vtable、PCM buffer、通知队列和 native sink。`dsound/calls.rs` 负责对象创建及方法处理，只借 guest memory、gateway table、heap 和显式只读依赖：event registry、window 集合与 Presenter。对象/vtable 分配进一步只借 heap 与 process heap ID，不借其它音频调用依赖。公开组合根仍负责参数、结果和诊断提交。

调用直接借 DSound，去掉临时 take 整个设备产生的默认实例。原先 pump → call → note → signal 的顺序保持；poll 仍先 pump 后 signal。通知 drain 仅借 DSound pending、KernelObjects32 和 Sched。Notify 对 buffer 的引用、最后释放的递归清理、buffer 创建失败时的引用/分配回滚、事件类型和 HWND 校验保持原行为。

buffer codec、锁定/回绕、播放控制、重采样/循环/混音、时钟累计、AudioQueue 调用与统计均保留原主体。没有改变声音后端、buffer 限额或播放能力；ABI 仍来自既有 Hangover 方法表和 dsound.spec。

基线 `c0d296b`。对象方法及分配归一核对、PCM/mixer/native pump 与 ABI parser 对照一致。release audio 4、mixer 1、完整 Win32 255、fmt/release lib/bin strict Clippy/build 通过。普通资源借用调整不跑性能或游戏矩阵。证据：`wire-out/refactoring-2026-10-14/r3-dsound32-{source-checks,verification}.json`。
