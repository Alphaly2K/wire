# 真实 Unity 的原生 UnityTLS 与 image/assembly（2026-10-09）

本阶段在原生 Mono 独立 worktree 继续同一个真实 UnityPlayer，默认不开启。
原始 Mono 无映射或 TLS/DllMain；完整 game Admission 仍 false。
浮点、SSE、翻译缓存、native Mono source/patch/dylib 均未改动。

## 实测

`wire-out/native-mono-unity-images-probe-01`：真实 Unity 主动发起 **82** 次 image open、
**82** 次 assembly load、**82** 次 caller image close、**82** 次 assembly-get-image，
均使用 native Mono。这是游戏自身的循环，不是人工替代程序集清单。
止于第一个 class metadata query 之前；输入对应82个实际Managed DLL，每项hash保留。

UnityTLS runtime table的33个原始Win64目标已捕获；owned native table只包含编译函数指针。
两个error-state叶函数经实际table入口验证，16B hidden-sret、ref参数及first-error保留语义
与原始Win64解释对照一致。其余31个 typed callbacks 激活时明确fail-stop，尚未实现TLS网络。
表保持到native cleanup，随后清除Mono全局table pointer再释放，避免悬空。

真实注册表10,824名/PC仍逐项匹配，两个实际CoreModule Boolean InternalCall、原生GC
事件mask0xfe1及shutdown1次仍通过。noexec guard **178 checks / 0 denied**。
已有native graphics/MoltenVK仅用于继续真实引擎初始化；工具先冻结库/hash，不改图形实现。
此前pre-images边界（`native-mono-unity-pre-images-probe-02`）guard98/0已通过。
有限诊断耗时不是完整游戏加速数据；本阶段没有托管游戏逻辑运行或场景渲染。

## ownership

image输入先复制为owned bytes，并与GuestFs解析后的clone文件逐字节核对。native open必须
need_copy=1、非refonly；status输出先检查guest写权限，FFI只接收本地status及owned字符串。
无guest/native借用payload跨越调用，返回值为unmapped opaque image/assembly token。

每个image额外持有provider ref；caller close释放其原生open ref，但assembly借用的image
identity仍可查询。所有provider/caller剩余refs在runtime cleanup前释放；assembly本身由
native domain/runtime管理，不编造额外assembly_close。退休后token不能恢复；native地址
不暴露给客体。伪造token、double-close、provider保活和幂等退休有独立回归。

所有image native调用均安装同owner callback scope，可能发生的真实GC可解释重入Win64。
生产Mono worker owner queue、通用managed object/root/metadata ABI仍未接入。

## 工具

新边界 `pre-images|images`；已有root/main-thread/pre-bindings/bindings/profiler保留。
`images`止于第一个class query；新输出为 `api.tsv.mono-unitytls.tsv`、
`api.tsv.mono-images.tsv`；工具检查四类image调用均82次，注册表匹配及GC/TLS/cleanup证据。

```sh
python3 tools/mono/probe_startup.py \
  --game '/Users/alphaly/Downloads/パパがアリスをママにして♪/AliceToPapaADV_1-0-0.exe' \
  --skip-etw --boundary images --max-instr 200000000 \
  --graphics-backend /Users/alphaly/Projects/wire/wire-out/libwire-graphics-resource-perf-2026-10-09.dylib \
  --moltenvk /private/tmp/wire-moltenvk-1.4.2/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib \
  --output wire-out/native-mono-unity-images-fresh
```

当前feature完整Rust回归 **394 passed / 7 ignored**，feature全目标Clippy clean。
默认完整回归仍在运行，结果记录到 `native-mono-images-default-tests.log`，不得提前声称通过。
下一步继续typed class/method/signature/type查询、iterator及已知userdata字段的严格边界，
再连接managed对象/roots。不能把raw native metadata heap开放给guest读取。
