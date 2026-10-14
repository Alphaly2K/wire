# 测试与性能工具边界

2026-10-14：保留 `x64`、`win32` integration target，把已有测试按 CPU/SIMD、memory、kernel、files、modules、sync、USER、COM 和图形等实际领域组织到子模块。共享小型 PE/VM/调用 harness 位于 `tests/support/`，跨领域的 PE32 DLL builder 与 guest COM 调用也在此处；领域私有 helper 和测试一起移动。

完整函数名新增领域前缀，例如原 `amd64_integer_flags_and_rex_register_aliases_match_unicorn` 现在为 `cpu::amd64_integer_flags_and_rex_register_aliases_match_unicorn`。旧的函数名 substring 过滤继续可用，`--exact` 过滤需要新名称。完整映射保存在 `tests/test-name-map-2026-10-14.json`；R7 迁移时的 130 个 x64 测试与 109 个 Win32 测试逐名保持，后续新增用例不属于名称迁移项。5 个原始 guest Mono 观察测试留在 `x64.rs`，不涉及 native Mono。

```sh
cargo test --release --test x64 -- cpu::
cargo test --release --test win32 -- user::
cargo test --release --test x64 -- cpu::amd64_integer_flags_and_rex_register_aliases_match_unicorn --exact
```

`tools/perf/measurement.py` 只负责 child 的 wall/CPU/RSS、timeout、report、hash 和 AB/BA 顺序。架构脚本继续拥有 fixture、work、retired、结果和 API count 校验；不会把不同架构或模式混成一个吞吐量。旧报告字段保留，新增 `timed_out` 和测量库 hash；V2 另记录使用的 replay 源 hash。PE32 wall 窗口现在与公共库一致，不包含结果文本读取；macOS RSS 仍为 bytes，Linux 的 KiB 转成 bytes。

`tools/perf/freeze.py` 保存一次已构建 binary、source diff、toolchain 和输入 hash。旧 `tools/freeze_interpreter.py` 委托此入口；默认包含跟踪文件及 src/tests/tools/examples/native 下的新源码，额外输入须用 `--include` 指定仓库相对文件。未跟踪的 docs 设计稿/PDF、IDE 元数据不自动进入快照。符号链接保存链接及其目标文本 hash，不复制目标素材；复现者需要另行提供该链接引用的外部输入。

```sh
python3 -B tools/freeze_interpreter.py --binary target/release/wire --out wire-out/new-frozen-interpreter
python3 -B -m unittest discover -s tools -p 'test_*.py'
```

PE32、AMD64 和 V2 的公开脚本入口保持不变，正式 fixture 位于 `tools/perf/fixtures/`。native/probe 和 inventory 的现有入口仍保留；native source、FFI 和 third_party 构建路径没有搬动。本页描述当前工具边界，后续领域迁移仍需独立验收。

2026-10-15：PE32 和 AMD64 测量入口均接受可选 `--qos-clamp utility|background|maintenance`，通过 macOS taskpolicy 对每个被测 child 施加同样的调度上限；默认不改变 QoS。manifest 明确记录模式与 taskpolicy hash，逐轮 command 保留 wrapper，不把受控调度结果混成默认前台吞吐量。

PE32 headless 整数 fixture 的同二进制 700M 对照曾出现 2.31–7.28s 两档，而 IR/fallback/scalar-run 计数完全相同。开启采样后没有复现慢档；原生 UI 初始化另有显式 USER_INITIATED QoS。因此调查必须保持 headless/UI、采样和调度条件一致，不能沿用 capture 默认 UI 的结果替代 headless 计时，也不能直接认定为代码回退。utility 同二进制三对的差异约 +0.70%/−1.88%/+2.49%，该受控条件可用于后续成对调查；默认条件的失败、离散轮次仍全部保留。
