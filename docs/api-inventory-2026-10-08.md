# 四个游戏的 Win32 API 批量清单（2026-10-08）

已实现 [tools/api_inventory.py](../tools/api_inventory.py)，不启动游戏即可批量扫描原始目录。
本次四个目录共 198 个 PE，读取、解析并对照 wire ABI 表耗时 **17.91秒**。
扫描所有具有 MZ/PE 头的文件，包含子目录 DLL、托管程序集和改名插件；原始样本只读。

完整结果保存在本机，不提交游戏数据：

- [按 DLL 汇总与补齐候选](../wire-out/api-inventory-render-2026-10-08/summary.md)
- [可筛选 CSV](../wire-out/api-inventory-render-2026-10-08/inventory.csv)
- [完整 JSON：来源、哈希、导出、轨迹和警告](../wire-out/api-inventory-render-2026-10-08/inventory.json)

## 本次结果

普通和延迟导入按 **DLL+符号** 去重，序数导入也计入；这里包括Win32、CRT和guest库。
下表覆盖整个目录，包含可选或尚未加载的插件，
不是入口 EXE 必须执行的 API 数量，也不是需要全部实现成功的数量。

| 游戏 | 架构 | PE | 普通导入 | 延迟导入 | 两者合并去重 | CLR P/Invoke | native 导入 ABI 缺失 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Rewrite | x86 | 6 | 448 | 0 | 448 | 0 | 0 |
| Koikake | x86 | 13 | 443 | 192 | 635 | 0 | 30 |
| Totsulover | x86 | 19 | 863 | 301 | 1152 | 0 | 179 |
| AliceToPapa | x64 | 160 | 877 | 108 | 984 | 1304 | 441 |

`abi_known` 只表示当前 wire 有 ABI/分派登记，不能证明所有参数分支或实际后端功能都已支持。
例如 Rewrite 的 ABI 缺失为 0，不表示画面、音频和游戏流程已经完整可用。
工具共用 PE32 `native_export` 和 Win64 实际 dispatch 的 `import_supported`，不维护第二份手写支持名单。
API-set 别名按当前运行时映射查询；不宣称支持完整 Windows API-set schema。

合并已有 PE32 `api.log`、Win64 启动诊断及 pass123–142 全程统计后：

| 游戏 | 已观测外部 DLL 调用/尝试的符号 | 已观测且来源明确的外部 DLL 动态查询符号 |
| --- | ---: | ---: |
| Rewrite | 281 | 0 |
| Koikake | 193 | 0 |
| Totsulover | 46 | 0 |
| AliceToPapa | 240 | 118 |

PE32 旧日志中的 `GetProcAddress` 只有函数名，缺少 DLL 来源，保留为 `provider_unresolved`，
没有计入上表动态查询列。0 不表示游戏没有动态查询。
计数覆盖 JSON 中列出的历史日志，包含多次重放，不是某一次游戏的总调用量；
不要同时合并同一次运行的完整 TSV 和包含相同调用的文本轨迹来测量调用频率。

## 证据层级

- `import` / `delay_import`：PE 导入表的声明，包含 IAT RVA，明确 DLL 和符号或序数。
- `pinvoke`：CLR ECMA-335 ImplMap 的声明，含 ModuleRef、member token、mapping flags；
  未设置 ExactSpelling 时保留 A/W 绑定候选。大量 BCL 可选互操作不会自动列为启动前置缺口。
- `export_forwarder`：guest DLL 导出的转发目标。
- `runtime`：所选轨迹中已调用、接受调度或尝试调用的 gateway；Win32 返回 FALSE/HRESULT 失败仍然算实际调用。
- `runtime_lookup`：GetProcAddress 请求，独立于实际调用；查询可能返回 NULL，也可能是可选探测。
- `string_candidate`：看起来像 API 名称的 ASCII 字符串及文件偏移，未证明它会被使用。

同名 bundled DLL 只标为 provider 候选，完整 exports 保留在 JSON 中；不保证候选会按实际加载搜索顺序被选中。
`entry_import_reachable` 是导入、延迟导入和 P/Invoke 声明图可达标记，包含可选边，不能证明运行时已经加载。
guest Mono/Unity DLL 的动态查询独立标记为 bundled；内部 `wire.dxgi` COM gateway 不计入外部 DLL 汇总。

静态扫描不能完全确定加密/拼接的动态函数名、运行时新文件、COM vtable 调用或未执行分支。
清单先覆盖可以直接读出的声明，再用实际轨迹补充；不会把字符串猜测当成调用事实。

本次唯一解析警告为 Totsulover 的
`DimensionToTsuLovers_cn/Plugin/DimensionToTsuLovers_cn.dll` delay-import attributes 未识别。
该文件普通导入仍保留，其延迟导入暂不能声称完整。其它 197 个 PE 无解析警告。

## 复跑

在项目根目录执行，只需 Python 标准库和项目的覆盖查询 helper：

```sh
cargo build --release --example api_coverage
python3 tools/api_inventory.py \
  --game 'rewrite=/Users/alphaly/Downloads/Rewrite_高压汉化版 V2/Rewrite.exe' \
  --game 'koikake=/Users/alphaly/Downloads/恋×シンアイ彼女/koikake.exe' \
  --game 'totsulover=/Users/alphaly/Downloads/PC_ディメンション凸ラバース/totsulover.exe' \
  --game 'alice=/Users/alphaly/Downloads/パパがアリスをママにして♪/AliceToPapaADV_1-0-0.exe' \
  --coverage-helper target/release/examples/api_coverage \
  --trace alice=wire-out/games64/alice/api-pass-123.tsv \
  --output wire-out/api-inventory-new
```

`--trace NAME=PATH` 可重复添加 PE32 api.log 或 Win64 文本/TSV；
上面的示例只合并 pass123，历史轨迹的准确输入列表保存在本次 JSON 的 `games[].traces`。
省略 helper 仍能提取 API，但 coverage 为 `not_checked`。

Win64 新增可选全程聚合，不受最近调用环形缓冲区的长度限制：

```sh
# 在游戏运行副本目录执行；其它图形/窗口参数按实际运行配置添加。
/Users/alphaly/Projects/wire/target/release/wire probe64 AliceToPapaADV_1-0-0.exe \
  --startup --max-instr 3500000000 \
  --api-trace /Users/alphaly/Projects/wire/wire-out/games64/alice/api-next.tsv
```

TSV 包含 `completed`、`lookup`、`failed` 三种独立记录及 DLL/符号/次数。
`completed` 表示 gateway 已完成或已接受回调/调度，不等同于 API 成功返回；
`failed` 表示 gateway 分派返回宿主错误，不自动等同于未实现，也不覆盖所有异步回调故障。
查询保留请求的实际逻辑模块及序数；TSV 对反斜杠、制表和换行转义，扫描工具对应解码。
统计默认关闭，普通与性能运行不维护聚合表。分派故障后仍在最终错误传播前保存结果；
强杀进程、加载失败或无法写入目标文件不保证得到 TSV。

## 按清单成组推进

Alice pass123 全程记录了 3,854,319 次已完成/接受的 gateway 调用（228 个符号，其中38个内部COM gateway），
455 次 GetProcAddress 查询（429 个 DLL+符号组合）和1次 gateway 故障：`hid.dll!HidD_GetHidGuid`。
实际窗口仍为隐藏初始化窗口，尚无 guest Draw/Present，不能据此判定游戏跑通。

清单已经列出同一设备发现路径的完整声明，可以按组核对并实现：

- HID：7 个 `HidD_*`，包括 GUID、attributes、preparsed data 和设备字符串；另有7个 `HidP_*` report 解析函数。
- SetupAPI：`SetupDiGetClassDevsA`、`SetupDiEnumDeviceInfo`、`SetupDiEnumDeviceInterfaces`、
  `SetupDiGetDeviceInterfaceDetailW`、`SetupDiDestroyDeviceInfoList`。

本轮已据此成组接入上述14个HID导入与5个SetupAPI导入，并补A/W对应入口；
Alice native导入ABI缺失先由488降为469；再接入设备通知注册/注销后为467。
这是ABI登记与当前profile的失败/空枚举语义，不是HID report解析器或物理设备支持。
SetupAPI使用owned高地址HDEVINFO、32字节Win64记录和ERROR_NO_MORE_ITEMS，销毁后句柄失效。
没有PnP/HID backend，HidD设备查询返回INVALID_HANDLE并保留输出；无owned preparsed data的HidP调用返回
HIDP_STATUS_INVALID_PREPARSED_DATA，MaxDataListLength为0，不改变LastError。DEFAULT/PROFILE flags明确不支持。
RegisterDeviceNotification A/W接受有效窗口与interface GUID/all-class订阅，返回owned高地址token；
Unregister及窗口销毁释放它。当前设备数据库静态为空，没有虚构arrival消息；service/handle类通知明确不支持。
优先处理已经出现的故障及同组声明，再根据 native 导入可达候选处理下一批库。
目前隔离 profile 没有 HID/gamepad 后端，设备枚举反映真实为空；不能用虚构设备或成功结果填平缺口。
USER32 消息循环及extra/time/position元数据已成组接入，当前实际游戏已触发宿主消息分支；
更多可选API仍需保留能力失败与回退语义。

## 正式窗口与logo之后的清单更新

pass126–142已经接入DXGI output、windowed交换链、InputLayout、真实guest Draw/Present，
以及BSTR named/ordinal组、WinHTTP proxy discovery、Winsock事件和IP地址/字节序转换组。
Alice native导入ABI缺失由467降为441，已观测外部调用/尝试符号由200增为240。
当前profile没有网络transport，socket/WSASocket创建明确返回64位INVALID_SOCKET；
pass141证明游戏自行处理失败继续运行，不把它统计成网络支持。
CriticalSectionEx flags=1曾为参数分支故障，ABI本来已登记；修复低位flags校验后pass142继续推进。

最新pass142：退休指令2,282,797,097，已完成/接受API4,472,765，
335次真实GPU Draw、112次成功Present；1024×720正式窗口由guest自己显示。
用户已观察到Unity logo动画，最后成功Present的单帧捕获为深色背景；尚未验收游戏标题画面或可玩。
当前明确阻塞为 `user32.dll!GetKeyboardLayout`，下一组候选是USER32键盘布局/输入语言查询。
这些接口应与现有guest locale/codepage/profile一致，再验证宿主输入分发；不靠强显窗口或空成功推进。

所选全部历史轨迹中的外部调用/尝试ABI缺失共2个：最新的GetKeyboardLayout，
以及pass128的DialogBoxParamW。后者是当时NOTOPMOST定位失败引出的错误对话框路径，
定位修复后最新重放已不触发；清单保留历史证据，不把它误当作最新启动前置API。
COM vtable gateway仍在JSON/TSV独立保存，不属于普通PE导入计数。

## 验证

5项独立 Python fixture 回归覆盖 PE32/PE32+、序数、RVA/legacy-VA delay descriptor、
越界/截断/终止项、CLR ImplMap、轨迹证据区分、TSV转义、改名PE和bundled动态查询分类。
新增2项Win64回归覆盖默认关闭、真实调用计数、完整模块/序数/转义查询、checked fault不算完成及process故障留存。
四个原始目录扫描已实际执行；GUI游戏仅使用隔离运行副本。
pass124已经实际通过HID/SetupAPI空枚举及set销毁，pass125通过设备通知注册；
`wire.dxgi!Adapter.EnumOutputs`/隐藏初始化窗口是当时的历史边界，当前pass142已越过真实Draw/Present。
这些COM虚表方法没有普通Win32导入项，完整TSV仍能保存它们的真实调用/故障。

设备发现组另有2项回归覆盖Windows GUID布局、64位set生命周期、空枚举记录、失效句柄、
输出fault原子性、HidD输出保留、HidP错误/LastError与DLL专属动态导出；相关Win64服务51项回归通过。
通知组另有1项回归覆盖过滤/recipient校验、故障不取得token、注销/窗口清理与DLL导出归属。
最新全项目release回归313项通过、6项ignored，Clippy all-targets `-D warnings`通过。
本轮新增BSTR、消息元数据、WinHTTP、WSA事件、IP转换、无transport创建失败与CriticalSectionEx分支回归；
图形后端的GPU像素/状态及真实WSI验证见 [图形设计记录](d3d11-dxvk-moltenvk-design.md)。
