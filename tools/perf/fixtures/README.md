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

`guid_gateway.py`、`module_gateway.py` 提供双架构 API 密集输入；
`standard_gateway.py` 的 Win64 GetFileType 循环校验标准 pipe 类型与 API count；
其 PE32 同名 case 只读打开生成的 `gateway-file-type.exe` 自身，校验文件句柄类型、
返回 checksum、退休数和 API count，不需要游戏素材或额外输入文件；
`kernel_gateway.py` 的 PE32 CreateEvent → SetEvent/ResetEvent 循环校验共享 handle 寿命和每次返回值。
`path_gateway.py` 的双架构 GetCurrentDirectoryA(0, NULL) 循环校验根目录
`C:\` 的四字符容量、累计 checksum、退休数和 API count，不访问外部素材。
`metadata_gateway.py` 对 GetFileAttributesExA 使用无效信息类和空指针：
Win64 按现有顺序返回错误，PE32 保持 ABI-only 零返回；两架构均校验
累计返回值、退休数和每一次 API 调用，不访问 host 文件或时间戳。
`search_gateway.py` 的双架构 FindClose(NULL) 循环校验零返回、退休数和
API count，经过真实目录枚举错误分支，不访问 host 文件或时间戳。
这些 case 需显式选择，默认 case 集合不变。PE32 正式 API 计时可加
`--discard-api-log`：仍格式化 trace、记录频率和计数，将全量 trace 写入空设备。
