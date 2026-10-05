# 首次真实游戏像素与 macOS 呈现

## 检查点结果

按用户指定，以 Hangover 的固定 Wine 子模块为主要 Win32/COM ABI 参考，
不链接其模拟器、Wine server、Linux 驱动或 D3D 渲染实现。
已真正运行游戏资源解码、D3D9 纹理写入、三角形绘制和 Present。
TVPMainWindow 为 `0x40010008`，初期 1920×1080 client，随后进入 1024×768
窗口布局。此次捕获的提示页和 STAFF 画面来自游戏自己的资源，未经资产修改。

最新完成运行：`wire-out/game-graphics/d3d-z`。
退休 **2,808,011,674** guest 指令，**3,747,880** API，
成功 **13** 次 DrawPrimitiveUP、**12** 次 Present、**12** 次 AppKit 帧上传。
CPU 真值截图：`frames/frame-000002.png`（提示页）、
`frames/frame-000008.png`（STAFF）；PPM 是原始捕获，PNG 用系统 sips 转换。
日志、运行副本、截图和游戏资产均不提交。

这不是完整可玩验收：该运行最后停止在缺少 `66 0f d3` PSRLQ 的指令路径。
现已补齐 SSE2 可变 packed logical/arithmetic shifts，完整低64位计数、
越界计数、源/目标别名、m128 与 FLAGS 不变，Unicorn 差分通过。
原 DirectSoundCreate 的默认 S_OK/空对象崩溃已改为明确 DSERR_NODRIVER；
游戏能够处理这一失败并继续呈现。音频后端仍未实现，不能声称音频已工作。
字体 LOGFONT 对象已实现，但字形/文本绘制未实现；截图内文字属于游戏图片。

## 实现范围

- geometry：固定96 DPI初始 profile，统一 AdjustWindowRectEx、client rect/origin、
  screen/client/map points、默认 WM_MOVE/WM_SIZE/WM_NCCALCSIZE。
- paint：真实 update/erase 状态、Invalidate/Validate/GetUpdateRect、Begin/EndPaint、
  WM_ERASEBKGND guest callback、低优先级 WM_PAINT、DC clip、默认背景像素。
  synthetic paint 会在 region 验证后消失，显式 PostMessage(WM_PAINT) 保留。
  默认 WM_CLOSE 执行真实 DestroyWindow。
- GDI：logical LOGFONT A/W、完整14参数 CreateFont、stock/current/selected font；
  32bpp BI_RGB StretchDIBits/GetDIBits，checked rows、上下行序、nearest scale。
- D3D9：guest COM head/vtable，IDirect3D9/device/texture/surface/swapchain 的已验证子集。
  external refs 与内部 pins/leases 分开，绑定资源和外部子对象保活，避免循环引用；
  失败输出先检查，内存/对象数量有界，最后引用释放 guest storage。
  纹理、surface/backbuffer 为真实 guest BGRA内存，LockRect返回pitch/bits；
  32位 A8R8G8B8/X8R8G8B8、单mip、Clear/fill/copy、viewport/scissor、有限 fixed-function
  state、XYZRHW FVF 0x104/0x144、三角形 list/strip/fan、top-left边界、
  point/bilinear sample、diffuse/texture组合、有限alpha blend、真实Present。
  Z必须在[0,1]，RHW=1；未实现深度、shader、复杂resource/state和不支持参数明确失败。
- macOS：公开 AppKit/CoreGraphics 上传 CPU 真值surface；不生成可执行guest机器码。
  main线程轮询只排队host事件，guest WndProc仍通过CPU执行。
  message wait有16ms UI polling deadline，cooperative timeslice也会poll。
  鼠标/键盘/关闭事件路径初步接入；完整按键状态、IME、菜单、resize交互仍未完成。

Present只支持完整destination client rect、无dirtyRegion，source rect有界。
GPU栅格化和Metal未引入；此阶段CPU像素是真值。

## 复现

```sh
scripts/package-macos.sh
cp -cR '/Users/alphaly/Downloads/王様恋愛【体験版】' wire-out/game-runtime
wire-out/Wire.app/Contents/MacOS/wire run \
  'wire-out/game-runtime/王様恋愛【体験版】.exe' \
  --present --ir wire-out/game-v3.wir --max-instr 10000000000 \
  --log-dir wire-out/game-graphics/replay \
  --capture-frames wire-out/game-graphics/replay/frames
```

运行副本是完整APFS clone，供隔离的GuestFs合法写savedata；不放宽文件路径隔离。
目录已经存在时不要重复cp。IR需事先用 `wire prepare`生成；此处复用已有匹配EXE的v3 IR。
CLI macOS默认开启呈现；`--headless`用于无窗口诊断，`--capture-frames DIR`最多导出16张不同CPU帧。

验证包含真实COM gateway fixture、逐像素alpha/shared-edge/scissor、资源保活和失败不变、
paint消息、geometry/font/DIB，以及CPU packed shift差分。
完整 **125项**通过（3 unit、36 CPU differential、5 FPU、14 IR、1 Wasm、66 Win32）。
release Clippy all-targets、fmt和diff检查通过。

## 用户指定的后续顺序

2026-10-06用户要求当前部分收尾后暂停功能扩展，先做性能优化。
首帧需要约一分钟；现有43 MIPS量级是包含初始化/解码/日志/呈现的整体运行值，
不能作为优化收益。下一步抓同一游戏启动到首张真实图片的profile，
分别确认解释器、资源解码、API/heap与CPU raster占比，然后用固定负载重复复测。
音频、IME、额外图形功能先不扩展。

后续已完成完整长启动采样和三对最终版AB/BA复测：
首 Present 中位数25.031→18.996秒（约−24%），测量段37.191→29.534秒，
128项回归通过。该数据只针对同一真实游戏启动负载，尚未解决性能问题；
具体实现、时间口径和剩余热点见 [启动性能记录](performance-startup-2026-10-06.md)。
