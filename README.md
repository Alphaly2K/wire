# Watari-Is-a-Runtime-Environment (wire)

非 JIT x86 解释 + Win32 兼容层可行性验证（spike）。目标：在 Apple Silicon（最终 iOS）上
运行真实 VN 引擎的原始 32 位 Windows 二进制。许可证 LGPL-2.1（与 Wine 一致，见 LICENSE）。

## 里程碑状态

- [x] **M0** PE 装载器 + API 调用日志
- [x] **M1** x86 解释器核心 + 差分测试（Unicorn 参照，24 族全绿）
- [ ] M2 Win32 最小子集（需求驱动）
- [ ] M3 图形与音频落地（目标引擎为 GDI blit 路径，见 docs/m0-report.md）
- [ ] M4 性能测量与 Go/No-Go 报告

## 架构决策（v0，实现方敲定）

- Rust；核心库与平台无关，宿主（macOS/iOS）走薄移植层
- 单进程；guest 4 GiB 地址空间经 mmap 保留、按宿主页提交
  （Apple Silicon 为 16 KiB 页，提交粒度须对齐——M0 实测排掉的第一个坑）
- 全部 Win32 API 在宿主机原生实现；guest 代码永不原生执行（无 JIT 前提从第一天成立）
- IAT 指向保留 stub 地址区间（0x6fff0000..），EIP 进入即陷入宿主分发；
  stdcall 栈清理由内置 arity 表驱动，未知函数按 cdecl 处理并打警告日志
- 宿主→guest 回调机制已就位（CRT `_initterm` 已在用）
- 解释器 decode-dispatch，正确性优先；未支持指令即致命错误并记入统计，
  覆盖范围由目标程序实际执行的 opcode 统计驱动

## 使用

```sh
cargo build --release

# 打印 PE import 清单（装载器自检 / 加壳启发式检测）
wire imports path/to/program.exe

# 运行；API 全量 trace 与频率表写入 wire-out/api.log
wire run path/to/program.exe [--max-instr N] [--log-dir DIR] [--trace-cpu]
```

对照程序（自编译，仓库只含源码）：

```sh
brew install mingw-w64   # 一次性
tests/control/build.sh   # 生成 tests/control/control.exe（32 位，文件 I/O + 窗口 + GDI blit）
wire run tests/control/control.exe
```

## 合规

- 仓库不含任何游戏文件/资产；测试目标一律经本地路径指定
- 不实现任何针对已知保护方案（DRM/加壳）的处理逻辑，也不接受此类 PR
- 第三方代码来源与版本记录于 THIRD_PARTY_NOTICES
