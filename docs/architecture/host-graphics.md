# Host / graphics 边界

PE32 进程和 API 的轮询入口位于 `win32/x86/host.rs`；`host/ui.rs` 只接收 owned `WindowRequest`、受尺寸检查的 RGBA `Frame`，并返回普通 `WindowEvent`。host 不借用 guest memory、ApiState、Runtime64、调度器或 CPU。`platform::Presenter` 保留公开兼容路径。

PE32 顺序仍为：DSound 轮询 → enabled/capture 与 16ms gate → guest 窗口筛选 → native 窗口更新/删除 → 最多 512 个事件逐个送 USER 入队 → 判断并消费 frame sequence → guest 像素快照 → RGBA 上传/抓帧。像素快照不能提前到事件消费前；frame sequence 在 snapshot failure 时也会消耗，保持旧的重试行为。host/native 回调只排事件，不执行 guest CPU。

PE32 的 `win32/user/pe32/host.rs` 现拥有窗口筛选、owned 窗口快照和事件入队；guest adapter 直接分别借 Presenter、USER、错误槽与帧资源，不再临时 take/还原 Presenter。公开 `user32::host_message` 兼容入口保持；窗口条件、标题生成、事件过滤、键盘状态和 frame 顺序保持。

[DirectSound](directsound.md) 的普通方法只借所属音频资源及只读 event/window/Presenter；poll 和调用后 signal 显式借 kernel 对象与 scheduler，PCM 和 AudioQueue 主体保持。

AudioQueue 的 Rust 句柄与 FFI 归 `host/audio.rs::PcmOutput`，仅接收宿主样本/统计；guest buffer 读取、混音、通知和计时仍由 DirectSound 负责。关闭先于 guest 资源字段销毁，原 C 回调只消费宿主 ring buffer。

CoreText 测量的 Rust FFI 归 `host/font.rs`，接收 face slice 与标量，返回宿主 metrics；LOGFONT/TEXTMETRICW 布局、错误映射和缓存归 GDI。host 不解析 guest 字节，原 font.c 的测量与资源释放顺序保持。

原生 Vulkan 图形库 owner 和 draw/geometry helper 位于 `host/graphics.rs` 及子目录，公开 `gpu` 路径只作兼容重导出。静态 Metal raster owner 位于 `host/raster.rs`，D3D9 内部以原 gpu 名称引用；该 owner 只接收宿主 slice/标量，guest raster 准备和软件路径仍在 D3D9。原生源码、shader include 目标和 build.rs 路径保持。

Win64 的 `Presenter64` 继续拥有 guest HWND → native ID 映射，保留隐藏 bootstrap window、强制 poll、topmost 与带符号 lparam 扩展规则；owned window 请求交给同一 host UI。正常 D3D11 Present 仍通过 GPU surface 提交 texture，诊断 capture 才读取像素。保留的 MetalLayer 用 Rc 管理 native retain/release，surface 在 native 窗口删除后仍持有 layer。

Win64 User 的 host 入口负责窗口快照与 owned event 入队，只借原 start 时间；Runtime64 保留公开 facade。HWND/容量检查、时间取样、鼠标坐标调整、键盘过滤和入队顺序保持，host 侧不执行 guest callback。

随后补齐的[键鼠事件](input-events.md)共用 AppKit 回调与 Keyboard owner，包含物理键码/文本、左右修饰键、五键鼠标、拖动和双轴滚轮。native 统一提供 client 坐标，两个 guest adapter 再转换滚轮的 screen 坐标，并记录 MSG.pt；Win64 保留 ClipCursor。原有重构阶段的等价性结论不覆盖这些新增行为。

人工试用发现 Retina 裁剪后，CAMetalLayer 的 drawableSize 改用 view bounds 的实际 backing 像素尺寸，并同步当前窗口 backingScaleFactor；guest 输入仍使用逻辑 client 像素。真实 Vulkan/Metal 原创图案探针验证了横竖窗口切换、同一保留图层、drawable 尺寸与输入中心，旧错误已由 backing 尺寸断言独立复现。

Presenter 的资源限制在主线程上，类型明确不可 Send/Sync；初始化继续要求 AppKit main thread。构造 Frame 时检查尺寸与 RGBA 长度，避免把不足的缓冲区送到 native FFI。frame upload failure 保留错误并继续可选 capture；displayed 只统计成功上传。抓帧仍使用 RGB PPM、完整 RGBA hash 去重、最多 16 个不同 frame，错误文本和 sequence 文件名保持。

GDI 的 DLL/export 接受范围、GetDC 的 USER client dimensions、LastError 和 API diagnostics 位于 `win32/gdi/x86.rs`。`graphics` 拥有 bitmap/DC/font/pixel 资源，checked object/raster operations 只借用 Graphics、PE32 GuestMem 和固定参数数组，不依赖总 API 状态。当前 DIB/font codec 仍使用 PE32 布局；这一边界没有强制统一两种架构的地址空间。[D3D9](d3d9.md)的分配/引用/绑定方法只借 heap，设备调用再借只读 window 与 Graphics，公开组合根保留；DXGI/D3D11 的其它 guest COM 适配器继续按领域迁移。

[GDI+](gdiplus.md) 的 matrix、allocation 和 startup token 归 GdiPlus owner；PE32 调用显式借内存和 process heap，公开 facade 保留，原 Wine ABI parser 与数学算法不变。

回归入口：

Win64 的七项 DXGI/D3D 创建入口已接入[图形契约](graphics-contracts.md)，创建 handler 与 COM 方法分开。现有静态/动态 provider 差异、未知 PE32 ABI 和 GPU 资源语义保持；COM 方法元数据继续迁移。

重构阶段将 Win64 COM 对象分配和递归资源释放归入 Graphics，内存、gateway 和分配游标明确借用。该阶段原生 backend 未改，两个已有 GPU 资源生命周期 fixture 已显式通过。

2026-10-20 的功能接续将单个 texture 的总容量上限统一为 **256 MiB**，包含全部 mip、layer 与 depth slice；guest 创建检查、Rust readback 和 native 分配使用相同边界。大于 64 MiB 的上传/读回按不超过 8 MiB 的行带同步传输，避免突破通用 staging buffer 的 64 MiB 上限；普通传输路径保持。8192×8192 RGBA8 的完整写入/读回 SHA-256、超限拒绝、view 保留 texture/device，以及 guest COM 输出故障和释放顺序均有显式原生测试。NULL resource 创建 view 返回 E_INVALIDARG，并清零有效的输出槽。详见[执行报告](../reports/alice-execution-2026-10-20.md)。

shader stage 接口允许 producer 向 consumer 提供同 location/component、同 scalar 类型的向量子集，例如 float4 → float3。该放宽仅在设备实际支持并启用 Vulkan maintenance4 时生效；组件不足、缺少 location 或 scalar 类型不同仍拒绝。原创 `tools/probe_graphics_varyings.py` 使用自包含 DXBC fixture 验证 4096 个实际输出像素，并检查三类非法接口拒绝且 target 不变。

DXGI/D3D11 普通调用和 swapchain 创建/resize 也已收窄资源借用；Graphics 提供 owned frame 的 native texture/backend 引用快照与成功计数。Runtime/Process/Presenter 继续执行原 present 协议，surface 提交、返回帧和 unblock/trace 顺序保持。阶段整合全目标 694 passed / 7 ignored，两个真实 GPU fixture 显式通过。

```sh
cargo test --release --all-targets --locked
cargo test --release --lib --locked -- --ignored --test-threads=1
cargo run --release --example host_ui_probe -- CAPTURE_DIR
python3 -B tools/interpreter32_perf.py --binary before=FROZEN_BEFORE --binary after=FROZEN_AFTER --out NEW_RESULTS --cases gateway-gdi --discard-api-log
```

第二条需要按 [当前状态](../status.md) 显式提供 native graphics、MoltenVK、shader fixture 和可打开的 AudioQueue 输出设备环境；第三条在 macOS 主线程创建自己的探针窗口，验证 upload/failure、resize/remove 与抓帧，不使用游戏素材。原 native `.m/.c`、build.rs 和构建工具路径未迁移。正式原始日志与输入 SHA 位于本轮报告关联的 `wire-out/refactoring-2026-10-14/`。
