# 原创解释器验收输入

`amd64_audit.py` 是 2026-10-09 正式 V2 测量使用的原创输入生成器，
从 ignored audit 输出目录提取，未改变 guest PE 字节、预期 work、retired 或 checksum。
相邻 golden JSON 由迁移前生成器固定了 12K 与 100M work 的六组输入哈希。
`bit-test` 仍使用 `interpreter_v2_perf.py` 中已有的跟踪生成器。

```sh
python3 -B -m unittest discover -s tools -p 'test_perf_fixtures.py'
python3 -B tools/interpreter_v2_perf.py --binary before=path/to/frozen-wire \
  --out wire-out/v2-smoke --work 12000 --pairs 1 \
  --cases sse-register code-alias code-control data-alias data-control fp-scalar bit-test
```

`--audit-probes PATH` 继续接受历史模块路径。默认运行不依赖 ignored 输出；
PE 容器与其余生成器复用 `tools/amd64_perf.py`、`tools/interpreter_perf.py` 的跟踪源码。
