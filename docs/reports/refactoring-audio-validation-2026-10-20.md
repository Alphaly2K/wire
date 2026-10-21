# 原生音频收尾验收（2026-10-20）

生产代码基线 `85ced07`。R5 最后待验的真实 PCM 输出已通过；本批仅加强 `src/host/audio.rs` 的 ignored fixture，并将原创诊断探针放入 `tools/probes/audio.c`。Rust owner 的生产部分、`src/platform/audio.c`、DirectSound 和构建输入与基线一致。

## 打开失败的执行环境对照

最初的 Rust fixture 在首次 `open()` 失败，独立主线程 C 探针也未能打开输出，后续断言没有执行。这些失败保留在 `wire-out/refactoring-2026-10-14/r5-host-audio-{native,baseline-native}.txt`，不改写为通过。

诊断探针直接 include 生产 `audio.c`，只包装三个 AudioQueue 调用以打印 OSStatus，不维护第二份音频实现。同一个已编译探针在受限与沙箱外执行的结果如下：

| 调用 / 结果 | 受限执行 | 沙箱外执行 |
| --- | --- | --- |
| AudioQueueNewOutput | 0 | 0 |
| 三次 AudioQueueAllocateBuffer | 均为 0 | 均为 0 |
| AudioQueueStart | −66680 | 0 |
| 提交样本 | 未打开，退出码 2 | accepted=2048，consumed=0，queued=2048，退出码 0 |

SDK 的 `AudioQueue.h` 将 −66680 定义为 `kAudioQueueErr_InvalidDevice`。该对照定位到当前受限执行条件下的 Start 失败；探针立即读取统计，只证明打开、提交与计数守恒，实际回调消费由下面的 Rust fixture 验证。原始对照见 `r5-audio-tracked-{sandbox,native}.txt`，早期诊断见 `r5-audio-diagnose-{sandbox,native}.txt`，均位于同一证据目录。

可从仓库根目录编译并运行探针：

```sh
clang -std=c11 -Wall -Wextra tools/probes/audio.c -framework AudioToolbox -o wire-out/audio-probe
wire-out/audio-probe
```

## 实际回调与资源生命周期

加强后的 `host::audio::tests::native_pcm_output_accounts_samples_and_closes_once` 使用真实 AudioQueue，在两秒上限内轮询并要求：

- 接受全部 2048 个静音 i16 样本，每次取样满足 consumed+queued=accepted。
- consumed 最终等于 accepted，且 callback 计数非零。
- 重复打开复用当前输出；关闭后 inactive，重复关闭安全，push 返回 0，stats 不改调用方缓冲。
- 重新打开创建独立计数，consumed 与 queued 均为 0。

```sh
CARGO_INCREMENTAL=0 cargo test --release --lib host::audio::tests::native_pcm_output_accounts_samples_and_closes_once --locked -- --ignored --exact
```

此前加强后的 fixture 已显式通过，日志为 `r5-audio-callback-native.txt`；本次沙箱外复跑也通过，日志为 `r7-audio-closeout-native-recheck.txt`。既有混音回归通过的日志为 `r5-audio-mixer-recheck.txt`。这些结果验证 native PCM 消费和 owner 生命周期，不代表人工听音或完整游戏音频验收。

该 fixture 继续默认 ignored，因为它需要可打开的真实输出设备；显式通过的 1 项不计入默认集成检查的 passed 数。原生图形、窗口和字体的历史验证仍按各自批次解释，见[收尾审计](refactoring-closeout-2026-10-20.md)。

## 最终集成复核

本次重新执行 release 全目标，结果 **695 passed / 0 failed / 8 ignored / 21 suites**；Python 工具 **31 项**、fmt、release all-targets/all-features strict Clippy 与 release build 通过。全目标包含既有混音回归。第一次受限执行在 Unicorn 差分套件收到 SIGILL，随后在允许参照执行的宿主环境完整通过，失败日志继续保留。

日志统一使用 `wire-out/refactoring-2026-10-14/r7-audio-closeout-` 前缀：`tests-sandbox.txt` 为受限失败，`tests.txt` 为完整通过，`native-recheck.txt` 为显式原生 PCM，另有 `tools.txt`、`fmt.txt`、`clippy.txt` 和 `build.txt`。`verification.json` 保存各命令、结果、源码及日志 SHA-256。README/docs 的相对 Markdown 文件链接也已核对，缺失的本报告已补齐。

本批没有生产算法、CPU/IR、memory/cache 或热点布局改动，按[验证策略](../architecture/validation-tools.md)不重跑性能或游戏矩阵。原计划 R0–R5 与 R7 的本轮交付及验收完成，R6 仍按用户要求排除。
