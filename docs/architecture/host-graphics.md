# Host / graphics 边界

PE32 进程和 API 的轮询入口位于 `win32/x86/host.rs`；`host/ui.rs` 只接收 owned `WindowRequest`、受尺寸检查的 RGBA `Frame`，并返回普通 `WindowEvent`。host 不借用 guest memory、ApiState、Runtime64、调度器或 CPU。`platform::Presenter` 保留公开兼容路径。

PE32 顺序仍为：DSound 轮询 → enabled/capture 与 16ms gate → guest 窗口筛选 → native 窗口更新/删除 → 最多 512 个事件逐个送 USER 入队 → 判断并消费 frame sequence → guest 像素快照 → RGBA 上传/抓帧。像素快照不能提前到事件消费前；frame sequence 在 snapshot failure 时也会消耗，保持旧的重试行为。host/native 回调只排事件，不执行 guest CPU。

[DirectSound](directsound.md) 的普通方法只借所属音频资源及只读 event/window/Presenter；poll 和调用后 signal 显式借 kernel 对象与 scheduler，PCM 和 AudioQueue 主体保持。

Win64 的 `Presenter64` 继续拥有 guest HWND → native ID 映射，保留隐藏 bootstrap window、强制 poll、topmost 与带符号 lparam 扩展规则；owned window 请求交给同一 host UI。正常 D3D11 Present 仍通过 GPU surface 提交 texture，诊断 capture 才读取像素。保留的 MetalLayer 用 Rc 管理 native retain/release，surface 在 native 窗口删除后仍持有 layer。

Presenter 的资源限制在主线程上，类型明确不可 Send/Sync；初始化继续要求 AppKit main thread。构造 Frame 时检查尺寸与 RGBA 长度，避免把不足的缓冲区送到 native FFI。frame upload failure 保留错误并继续可选 capture；displayed 只统计成功上传。抓帧仍使用 RGB PPM、完整 RGBA hash 去重、最多 16 个不同 frame，错误文本和 sequence 文件名保持。

GDI 的 DLL/export 接受范围、GetDC 的 USER client dimensions、LastError 和 API diagnostics 位于 `win32/gdi/x86.rs`。`graphics` 拥有 bitmap/DC/font/pixel 资源，checked object/raster operations 只借用 Graphics、PE32 GuestMem 和固定参数数组，不依赖总 API 状态。当前 DIB/font codec 仍使用 PE32 布局；这一边界没有强制统一两种架构的地址空间。D3D9、DXGI/D3D11 的其它 guest COM 适配器按后续领域迁移继续保留原路径。

[GDI+](gdiplus.md) 的 matrix、allocation 和 startup token 归 GdiPlus owner；PE32 调用显式借内存和 process heap，公开 facade 保留，原 Wine ABI parser 与数学算法不变。

回归入口：

Win64 的七项 DXGI/D3D 创建入口已接入[图形契约](graphics-contracts.md)，创建 handler 与 COM 方法分开。现有静态/动态 provider 差异、未知 PE32 ABI 和 GPU 资源语义保持；COM 方法元数据继续迁移。

```sh
cargo test --release --all-targets --locked
cargo test --release --lib --locked -- --ignored --test-threads=1
cargo run --release --example host_ui_probe -- CAPTURE_DIR
python3 -B tools/interpreter32_perf.py --binary before=FROZEN_BEFORE --binary after=FROZEN_AFTER --out NEW_RESULTS --cases gateway-gdi --discard-api-log
```

第二条需要按 [当前状态](../status.md) 显式提供 native graphics、MoltenVK 和 shader fixture 环境；第三条在 macOS 主线程创建自己的探针窗口，验证 upload/failure、resize/remove 与抓帧，不使用游戏素材。原 native `.m/.c`、build.rs 和构建工具路径未迁移。正式原始日志与输入 SHA 位于本轮报告关联的 `wire-out/refactoring-2026-10-14/`。
