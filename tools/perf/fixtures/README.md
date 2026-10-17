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
`clock_gateway.py` 的双架构 QueryPerformanceFrequency 循环校验 10MHz 输出、
累计 BOOL 返回、退休数和 API count，不读取宿主变化的时间戳。PE32 的
retired=`6*n+11`，Win64 的 retired=`5*n+11`、work=`6*n+11`，其中
n=`max(1, work//6)`；PE32 raw/IR 和 Win64 均在旧 handler 上独立预检。
`context_gateway.py` 先直接写 TEB LastError 为 42，再循环 GetLastError，
校验累计值、退休数和 API count。PE32 的 n=`max(1, work//4)`、
retired=`4*n+8`；Win64 的 n=`max(1, work//5)`、retired=`4*n+11`、
work=`5*n+11`。它也在旧 handler 上完成双架构 12K smoke。
`local_gateway.py` 使用三个真实导入，先分配 TLS、写入 42，再循环读取累计。
PE32 的 n=`max(1, work//5)`、retired=`5*n+14`；Win64 的
n=`max(1, work//6)`、retired=`5*n+17`、work=`6*n+19`，
两端 API count=`n+2`。双架构在旧 TLS handler 上完成 12K smoke。
这些 case 需显式选择，默认 case 集合不变。PE32 正式 API 计时可加
`--discard-api-log`：仍格式化 trace、记录频率和计数，将全量 trace 写入空设备。

`virtual_memory_gateway.py` 的双架构 `gateway-vmem` 先分配 4096 字节，
再循环 VirtualQuery，累计返回结构大小以及查询 BaseAddress 与实际分配地址的差值。
PE32 返回值为 `28*n`、retired=`11*n+17`；Win64 返回值为 `48*n`、
retired=`10*n+18`、work=`11*n+19`，其中 n=`max(1, work//11)`，
API count 均为 `n+1`。不依赖宿主分配地址，需显式选择。

`heap_gateway.py` 的双架构 `gateway-heap` 先取进程堆并分配 23 字节，
再循环 HeapSize 累加原始分配长度。n=`max(1, work//8)`，两端返回值为 `23*n`，
API count 为 `n+2`；PE32 retired=`7*n+18`，Win64 retired=`7*n+21`、
work=`8*n+23`。使用独立 OFT/IAT 和三项导入，不依赖实际分配地址。

`local_memory_gateway.py` 的双架构 `gateway-local-memory` 先 LocalAlloc(0x40, 23)，
再循环 GlobalSize，验证成对接口共享同一分配归属。n=`max(1, work//6)`，
返回值为 `23*n`、API count=`n+1`；PE32 retired=`5*n+13`，
Win64 retired=`5*n+16`、work=`6*n+17`。默认 case 集合保持。

`winrt_gateway.py` 的 Win64 `gateway-winrt` 通过现有 string API set 导入，
每轮在调用者栈上创建 HSTRING reference，再读取长度并消费返回值。
UTF-16 输入包含嵌入 NUL 和孤立 surrogate；循环不增加 owned allocation。
n=`max(1, work//13)`，返回值为 `4*n`、API count=`2*n`，
retired=`11*n+10`、work=`13*n+10`。旧 handler 上独立通过 13K 预检；
需显式选择，PE32 没有对应已实现路径。
