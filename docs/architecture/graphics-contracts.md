# 图形创建与 COM 契约

新 Wasm 入口在 AMD64 接通 CreateDXGIFactory/1/2、D3D11CreateDevice、D3D11CreateDeviceAndSwapChain、D3D12CreateDevice 和 D3D12GetDebugInterface。PE32 七项保持原 absent 范围，D3D9 使用[独立契约](d3d9.md)。公开创建入口的 ABI、架构和 provider 来自 `win32/catalog/graphics.rs`。

创建入口保留旧静态导入的实际图形 DLL、kernel32/kernelbase 别名和区分大小写的内部 `wire.dxgi`；fresh GetProcAddress 仍只在实际图形 DLL 中发现创建入口，已有绑定沿原契约查询。D3D12 的 #101/#102 序数继续归一。内部 COM 方法由 vtable 安装时绑定，不能通过公开名称任意绑定。

两种 ABI 另提供十八项可选后端接口，复用当前 GDI/USER 身份和服务调用帧。它们不会创建图形对象或初始化 GPU；程序可按失败结果选择已有后端。

| provider | 接口 | 当前协议 |
| --- | --- | --- |
| opengl32.dll | wglCreateContext、wglCreateLayerContext、wglDeleteContext、wglGetCurrentContext、wglGetCurrentDC、wglGetProcAddress、wglMakeCurrent、wglCopyContext、wglShareLists、wglSwapLayerBuffers | 没有 GL context，当前 context/DC 查询返回零并保留 LastError；MakeCurrent 的 NULL context 请求成功，忽略 DC。有效 DC 的创建或 swap 返回零/错误 50，非法 DC 和不存在的 context 操作返回零/错误 6。GetProcAddress 在无当前 context 时返回零/错误 6，不读取扩展名，也不提供扩展入口。 |
| gdi32.dll | ChoosePixelFormat、SetPixelFormat、DescribePixelFormat、SwapBuffers | 有效 DC 返回零/错误 50，非法 DC 返回零/错误 6；不读写不支持的描述符。原 GetPixelFormat 直接沿用 GDI 服务，有效 DC 返回未设置格式零。 |
| dxgidebug.dll / dxgi.dll | DXGIGetDebugInterface / DXGIGetDebugInterface1 | 没有调试接口，检查 IID 和输出后清空完整宽度的输出指针，返回 E_NOINTERFACE；NULL IID/输出返回 E_INVALIDARG。Flags 当前忽略。 |
| d3d11.dll | D3D11On12CreateDevice | 没有 D3D12 互操作，返回 DXGI_ERROR_UNSUPPORTED；可选 device/context 指针及 DWORD feature-level 输出全部预检后清零。NULL 输入 device 返回 E_INVALIDARG 并保持输出；其余后端专用输入不解引用。 |
| dwmapi.dll | DwmSetWindowAttribute | 有效逻辑窗口返回 DWM_E_COMPOSITIONDISABLED，非法完整 HWND 返回 E_HANDLE；不修改属性，不读取属性数据。 |

HRESULT 调用保持 LastError，Win32 失败调用只写当前线程真实错误槽；全部故障保持原调用帧、计数与尚未发布的输出，返回槽别名使用已捕获的返回地址。新入口在 `crates/wire-wasm/src/win32/graphics_probe.rs`，ABI 在 `src/win32/catalog/graphics_probe.rs`。GL 渲染状态、GLSL 前端、像素格式表、D3D12 互操作、debug 对象和宿主 compositor 均未实现；D3D/WebGPU 资源与呈现协议不提供这些语义。

空 context 与扩展查询遵循公开的 [wglMakeCurrent](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-wglmakecurrent) 和 [wglGetProcAddress](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-wglgetprocaddress) 契约；可选调试层与互操作签名见 [DXGIGetDebugInterface1](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_3/nf-dxgi1_3-dxgigetdebuginterface1) 和 [D3D11On12CreateDevice](https://learn.microsoft.com/en-us/windows/win32/api/d3d11on12/nf-d3d11on12-d3d11on12createdevice)。

DXGI/D3D11 的处理体由现行 Wasm 适配器复用，包含真实 COM 身份/引用、父子对象、adapter/output 缓存、private data、设备/上下文、资源/视图、状态、swapchain 和方法布局。共享入口在 `src/graphics/dxgi.rs`，新适配在 `crates/wire-wasm/src/win32/graphics.rs`；旧执行器、CPU、USER owner 和原生图形提交不进入新核心。窗口存在检查、客户区尺寸/可见性和显示 profile 来自当前 USER，查询单个窗口不复制整个窗口列表。

COM vtable 和对象页由运行时拥有并只读密封，客体 VirtualFree/VirtualProtect 不能退休或改写它们；对象通过 Release 递归释放父对象、绑定资源、映射页和 GPU 句柄。现有 1024 对象、private-data 项数/字节及资源描述符限制保持。线程之间共享进程图形 owner，VM 之间独立。

调用帧先捕获完整参数和返回位置，DWORD 按声明收窄，标量 float 参数从相应 XMM 寄存器读取。输出可以覆盖原返回槽，完成仍使用保存的目标。共享内存适配器保留结构化访问/guard 故障；失败不计服务 work，已完成服务计一 work、零客体退休。COM 查询/资源操作的输出检查及原失败协议继续由共享处理体执行。

装载 AMD64 映像后，在暂停且尚未创建图形对象时调用 `await vm.enableGraphics(options)` 配置便携 WebGPU；普通指令已经执行不妨碍配置。可通过 `options.canvas` 或同步的 `options.getCanvas(hwnd, processId)` 提供呈现目标，通过 `options.shaderCompiler` 提供原有便携 shader 编译器。canvas 回调在图形服务中调用，应使用传入的身份和宿主数据，不重入 VM。shader 编译器由调用者管理，不随进程或 VM 关闭。配置失败不发布 backend；实际 WebGPU 不可用时明确报错。普通 JIT 指令继续在同一 Wasm 引擎驻留执行。

子进程继承图形选项，在首次可用的 DXGI/D3D11 创建调用前按需初始化独立后端；普通控制台子进程不创建 GPU device。选项可以经另一架构的中间进程传递给后代。初始化失败保留原服务和配置供重试；进程退出或启动拒绝关闭本进程后端，父进程先退出不关闭子进程后端。多个进程使用同一 canvas 时，每次 Present 按当前 device 重新配置，旧 owner 的关闭或缓存驱逐不会撤销其他 owner 的配置。

异步 GPU 操作按 bridge 的 typed pending 状态暂停，错误文本不决定阻塞。运行时保存原调用帧并固定当前线程，待同一 GPU job 完成后重试该 API，完成前不计 work；随后发生输出故障时继续保留原帧。`run()` 自动等待 GPU flush，显式 `service()` 返回 `Exit.Graphics`，可用 `await vm.flushGraphics()` 后重试。pending 期间允许修复客体内存，CPU 编辑要求先完成该图形调用，避免改变在途请求的身份。

绘制命令在运行区间内批量提交；Present 和读回按后端自身的同步边界提交。VM 销毁先释放 Rust 持有的资源，再关闭 WebGPU owner 并归还可复用 bridge 槽；静态启动拒绝和进程退出也释放其图形 owner。

D3D11 保留已有兼容子集，尚不满足完整 feature-level 一致性。未支持的专用方法继续明确失败；combined device/swapchain 创建当前报接口错误，应分开创建。D3D12 没有设备或 debug 后端，分别返回 DXGI_ERROR_UNSUPPORTED 和 E_NOINTERFACE，并按旧规则清空有效输出。shader 编译、阶段连接、实际 Draw 和 canvas Present 使用共享编译器及 WebGPU 处理体；资源创建、更新、清屏、复制、异步读回及最终释放使用同一后端。

各阶段状态 getter、Dispatch、间接绘制、命令列表、UAV、mip 生成和部分 factory/output 操作仍有缺口；GeometryShader 创建在 WebGPU 的 stage 检查处失败。逐项方法名、共享处理源码和实际失败协议见[接口台账](../api-migration.json)的 `legacy_com_methods` 与 `graphics_capability_gaps`。
