# Wire

## 该项目完全由AI生成，且目前项目还未完善，请勿用于生产环境

## 该项目已停止维护并归档

Wire 的目标是面向 WebAssembly 的 x86/AMD64 Windows 二进制转译运行时，复用既有 Win32、Direct3D 和精确指令语义组件。当前通过 Wasm JIT 执行客体程序；预转译缓存和按热度分层编译尚在实现阶段。许可证为 LGPL-2.1-only，第三方声明见 [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES)。

入口位于 [crates/wire-wasm](crates/wire-wasm)，已能运行只使用已迁移服务的 PE32/PE32+ 控制程序。通用 Windows 应用与图形兼容仍在迁移；根 Cargo 工作区默认构建 Wasm 核心，原生解释器及旧执行 IR 已移除。

运行环境需要支持 WebAssembly SIMD、引用类型与多值返回。

安装 Rust 的 `wasm32-unknown-unknown` 目标后，可从完整安装目录启动程序：

```sh
python3 tools/run_wasm.py installation/app.exe --root installation
```

启动器增量构建核心与宿主采样库，并打开本地浏览器页面，点击“启动”运行。安装目录映射为虚拟 `C:\`，包含空目录，最多 4096 个文件。主程序最多 256 MiB，其余文件按需读取，支持大文件；运行期间须保持安装文件不变。目录须使用普通文件和目录，不含符号链接。页面提供窗口、键鼠、标准输入和输出；程序正常退出后，启动器返回其退出码的低八位。

`--audio` 启用 Web Audio；DXGI/D3D11 需通过 `--shader-artifacts <目录>` 提供与当前源码匹配的 `build_wasm_shaders.py` 输出。`--stdin <文件>` 提供初始标准输入并结束输入，`--core <文件>` 和 `--host-library <文件>` 使用已构建产物。macOS/Linux 已接入真实系统 CPU 时间，默认采用宿主时区；`--timezone utc` 使用 UTC。其他选项见 `python3 tools/run_wasm.py --help`。

构建检查：

```sh
cargo check --locked --target wasm32-unknown-unknown --target-dir target/wasm-rewrite
```

当前进度与验收条件见 [GOAL.md](GOAL.md)，组件边界见[运行时架构](docs/architecture/wasm-runtime.md)，API 契约入口见[文档索引](docs/index.md)。仓库不包含游戏二进制或资产。
