# Unity logo阶段的AMD64解释执行优化（2026-10-08）

接续功能提交`8ce9dbf`，按用户要求先优化性能。本轮只修改CPU内存、IR故障回滚和
Win64 gateway查找；图形后端、游戏资产和功能终点冻结。解释执行仍是主路径，
FLAGS完整eager计算，没有宿主x86执行或JIT。
两组真实窗口完整启动对照中，到同一兼容终点的wall中位数 **148.256→97.417秒**，
缩短34.3%；退休指令吞吐 **22.34→33.16 MIPS**，增加48.4%。这不是首帧或标题界面计时。

## Profile与改动

基线完整启动采样覆盖Unity/Mono加载、正式窗口及guest Draw/Present，
133,919个运行期样本。解码/取指分类占20.67%，其中包含cached_ir，并非全部是重译；
read_access自身16.87%、ScalarIr::step自身17.54%、Process64::run自身13.35%、
cached_ir自身8.41%、write自身5.48%、memmove自身5.11%。inclusive与self不可相加。
采样的163.446秒包含profiler开销，不作为无采样AB基线。
原始栈与汇总在`wire-out/perf-amd64-logo-2026-10-08/{before-profile,before-summary}/`。

最终实现：

- 仅非执行数据页的map/protect/unmap/commit/decommit/文件视图变化保留已有IR。
  旧或新权限包含EXECUTE时仍全局失效，代码写仍失效；所有映射/保护变化清除执行权限证明。
- 页数据由owned槽位持有，64项地址缓存保存完整guest页地址和整数槽位；
  miss使用整数页号哈希表。没有裸宿主指针、权限缓存或unchecked数据访问。
  每次访问仍检查当前Page的权限及guard，移除页先清地址项再回收槽位。
  VirtualQuery使用独立有序地址索引，保留reserved hole的下一已提交页边界。
- 单页I8/I16/I32/I64读写使用固定宽度拷贝；跨页仍先检查全部页，保持写入原子性。
- 标量IR只保存显式目标GPR、RSP、RFLAGS/RIP，用于故障回滚。
  当前opcode只能改写这两个GPR；POP RSP别名和POP [RSP]更新后寻址均保持原语义。
- gateway表维护实际注册地址的最小/最大边界，普通guest PC在边界外直接排除；
  边界内仍查完整地址。动态登记、任意loader地址和中间的guest代码空隙均有回归。

标准HashMap最初改善真实启动，但使小型SSE负载回退；最终的整数哈希及owned槽位缓存
消除了此回退。16字节拷贝专项没有稳定收益，已撤掉。
页槽位及哈希容量按峰值保留并复用，不宣称新增缓存没有内存成本。
IR的24B布局、16,384项translation cache及完整AMD64上下文保持原设计，详见[IR设计](ir-aot-design.md)。

## 确定性负载

Apple M4，release默认优化、保留调试符号。五类原创PE32+每类约50M退休指令，
三轮交替before/中间版本/最终版本及相反顺序，共45次计时、约22.5亿退休指令。
所有版本的返回checksum和精确退休数一致。下表为本轮基线与最终候选的全进程wall MIPS中位数，
包含装载和结束报告；CPU统计、采样和GPU关闭，另有不计入时间的覆盖重放。

| 负载 | 基线MIPS | 最终MIPS | 吞吐增加 |
| --- | ---: | ---: | ---: |
| 寄存器/FLAGS/CMOV | 79.58 | 84.45 | 6.1% |
| checked内存 | 52.55 | 75.02 | 42.7% |
| SSE与标量混合 | 32.83 | 34.85 | 6.2% |
| 16,384条指令/约53KiB代码 | 32.57 | 34.05 | 4.5% |
| 间接CALL/RET/栈 | 75.34 | 86.43 | 14.7% |

数据为`wire-out/perf-amd64-logo-2026-10-08/slot-cache-matrix/compare.json`。
矩阵不能替代真实游戏启动时间；表内不是此前逐条解码版本的旧收益。

## 真实窗口完整启动

相同隔离游戏资产、真实AppKit窗口、冻结MoltenVK后端；不启用capture、API聚合、
CPU统计或采样。预算4.5B，但每次必须到达**精确相同的既有SRV错误**，不能以耗尽预算代替。
两组AB/BA共约130.8亿退休指令；timing.json保留完整命令、wall和进程CPU。

| 顺序 | 基线wall秒 | 最终wall秒 | 基线MIPS | 最终MIPS |
| --- | ---: | ---: | ---: | ---: |
| 1，before/after | 147.761 | 97.641 | 22.410 | 33.087 |
| 2，after/before | 148.751 | 97.194 | 22.270 | 33.232 |
| 中位数 | 148.256 | 97.417 | 22.340 | 33.160 |

| 路径状态 | 1基线 | 1最终 | 2基线 | 2最终 |
| --- | ---: | ---: | ---: | ---: |
| 退休guest指令 | 3,311,316,462 | 3,230,690,964 | 3,312,739,527 | 3,229,941,660 |
| 已完成API | 6,292,241 | 6,192,692 | 6,294,093 | 6,192,411 |
| native GPU Draw/Present | 399/144 | 413/149 | 406/147 | 415/150 |
| Player.log程序集加载秒 | 19.069 | 12.368 | 19.266 | 12.453 |
| Player.log domain重置秒 | 0.425 | 0.283 | 0.428 | 0.287 |

进程CPU中位数146.572→95.659秒。两版均为thread 23 entry、
RIP=`0x7ffe00002d30`、HWND=`0x7ffd00630000`、visible=true、1024×720；
资源是128×1、1 mip、1 layer、R16 typeless(53)、bind=40，SRV为format54，
`layers=UINT_MAX`仍未归一化，真实backend继续拒绝该view。完整错误保存在各report中。
本轮没有补齐这个功能或改变执行终点以获得更短时间。

真实计时器/worker使退休数减少约2.5%、API数减少约1.6%，所以不能宣称执行了同一指令序列，
或把总延迟的1.52倍加速全归为每条IR的成本。按退休数得到的吞吐改善约1.48倍。
程序集加载中位数19.168→12.411秒同样改善，不能把总耗时当作程序集加载时间。
这组较长路径覆盖实际guest Draw/Present；标题画面和可玩性尚未验收。
用户此前确认过Unity logo动画，末次深色帧不否定曾显示动画。
证据在`wire-out/perf-amd64-logo-2026-10-08/alice-full/compare.json`及各轮独立保存的Player.log。

冻结文件及SHA-256：

```text
wire-logo-before-perf-2026-10-08
f9c1b70dd038ffaeeed49130d0eacea7185c42e6d5fcc7f5ea7c31da3cfd936e
wire-logo-slot-cache-2026-10-08
ebb5d763787e922a50af1ca24a07dc6437b81356834a91e062b9c789a57a9188
libwire-graphics-logo-before-perf-2026-10-08.dylib
5f64ccef922ebdf983ac35ac532a0bbe5e04028911aad5b8fc981f40b75a21b0
```

均在`wire-out/`。真实AB中后端固定为第三个文件；没有重建GPU库来混入图形功能收益。

## 独立缓存诊断

相同200M work的真实窗口重放单独启用CPU统计，不混入AB计时。

| 指标 | 基线 | 最终 |
| --- | ---: | ---: |
| IR work | 181,469,313 | 182,300,291 |
| fallback work | 18,146,294 | 17,314,985 |
| translations | 17,487,161 | 14,069,221 |
| 全局invalidations | 23,359 | 18,843 |
| 退休guest指令 | 199,596,317 | 199,595,176 |
| 已完成API | 383,297 | 383,967 |

翻译和失效约减少19.5%/19.3%。真实计时器和worker导致路径不同，不能把此百分比
当作同一指令序列的纯缓存命中改善。单独的selective-epoch初筛没有稳定wall收益，
本轮提升需要结合页查找和IR执行成本判断。
报告在`coverage-200m-{before,after}/`，MIPS仍按退休guest指令而不是IR条数计算。

## 优化后完整采样

再独立采样完整启动，77,182个运行期样本，仍到同一SRV错误，实际406 Draw/146 Present。
带采样wall104.848秒不混入前述97.417秒的无采样AB。

| 热点 | 基线self占比 | 最终self占比 |
| --- | ---: | ---: |
| ScalarIr::step | 17.54% | 22.87% |
| Memory::read_access | 16.87% | 0.91% |
| cached_ir | 8.41% | 12.67% |
| Process64::run | 13.35% | 10.79% |
| Memory::read_int | 0.53% | 2.77% |
| Pages::slot | 无该函数 | 2.11% |

最终read_int的inclusive占比3.77%，基线为10.84%；基线的标量读经read_access，
最终固定宽度读直接查owned页，不能只比较read_int的self而遗漏它原来的子调用。
ScalarIr::step与cached_ir的占比上升也不表示绝对变慢：总采样长度和总耗时均变化。
解码/取指分类最终18.88%，仍包含translation hit，不能据此说18.88%都是iced重译。
剩余优先方向为标量IR分发、缓存冲突/取指及cold fallback，继续用确定性负载和完整游戏终点判断。
本轮按实测收益收尾，没有扩大FLAGS丢弃或继续叠加未经验证的优化。
栈、summary.json、folded和flame.svg在`after-profile/`与`after-summary/`。

## 正确性

完整release **328 passed、6 ignored**，包括既有Unicorn差分及PE32回归。
新增5项独立回归覆盖数据页变化保留IR但保留guard/NOACCESS/unmapped故障、
四种标量宽度的未对齐/页尾/跨页/用户地址上界及写入原子性、
稀疏地址冲突/owned槽位回收/跨Memory合并、VirtualQuery乱序commit/decommit/释放重用，
以及真实Process64任意gateway地址分发和guest代码空隙执行。
既有自修改、EXEC-only、同页/跨页guard、重映射、POP [RSP]回滚及条件指令差分均通过。
本轮没有修改GPU后端；原生GPU功能验证见此前功能记录。
日志为`/private/tmp/wire-logo-perf-final-all-tests.txt`。
Clippy release all-targets `-D warnings`、fmt和diff检查通过；Clippy日志为
`/private/tmp/wire-logo-perf-final-clippy.txt`。

## 复现

```sh
python3 tools/amd64_perf.py matrix \
  --before wire-out/wire-logo-before-perf-2026-10-08 \
  --cached wire-out/wire-logo-fast-pages-2026-10-08 \
  --after wire-out/wire-logo-slot-cache-2026-10-08 \
  --out wire-out/perf-amd64-logo-replay/matrix --work 50000000 --pairs 3

python3 tools/amd64_perf.py game \
  --before wire-out/wire-logo-before-perf-2026-10-08 \
  --after wire-out/wire-logo-slot-cache-2026-10-08 \
  --out wire-out/perf-amd64-logo-replay/alice-full --work 4500000000 --pairs 2 \
  --skip-coverage --window \
  --expected-error 'wire.dxgi!D3D.Device.CreateShaderResourceView: image view format/range/usage unsupported; resource=TextureDesc { width: 128, height: 1, mips: 1, layers: 1, format: 53, bind: 40 }; view=ImageViewDesc { format: 54, mip: 0, mips: 1, layer: 0, layers: 4294967295, kind: 0 }' \
  --player-log Users/wire/AppData/LocalLow/TukiMahiru/AliceToPapaADV_1-0-0/Player.log \
  --cwd wire-out/games64/alice/runtime \
  --graphics-backend wire-out/libwire-graphics-logo-before-perf-2026-10-08.dylib \
  --moltenvk /private/tmp/wire-moltenvk-1.4.2/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib
```

`capture --window --work 4500000000 --sample-seconds 180`用于另行完整采样；
不得将它的耗时混入上述AB。`--cpu-stats`仅用于独立coverage。
