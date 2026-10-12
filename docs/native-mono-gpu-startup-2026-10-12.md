# 原生 Mono GC 与 GPU buffer 启动接口（2026-10-12）

实施目录main。上一批代码提交`e728422`；游戏仍未进入Naninovel标题或可交互循环，
full_game_backend=false。原游戏、冻结Mono runtime与support仍只读，实测使用APFS副本。

probe-114实际执行Screen.SetResolution(1280,720,Windowed)，290 draw/97 Present；
最后frame是深色过渡背景，不能当作标题或游戏可玩验收。停止在mono_gc_max_generation。
probe-115转发真实max_generation与collect，collect执行8个原Unity GC callback，
所有现有roots/字段/数组核对通过；随后缺SystemInfo.MaxGraphicsBufferSize与ComputeBuffer.InitBuffer。

GC控制成组接入12个固定export：max generation、full/incremental collect、start incremental、
collection count、heap/used size、incremental mode、time slice、pending finalizers。
原冻结C ABI执行，64bit位型不截断；generation/Boolean/time slice预检。
只在live root-domain/main-thread owner准入；收集前同步guest字段/数组，收集后复用
原诊断收集的完整root核对，不将主动collection替换为no-op。完整feature510 passed/
7 ignored，all-target release Clippy通过。

ComputeBuffer/GraphicsBuffer各11个固定adapter成组接入：创建、销毁、validity、count/
stride/usage、name、counter/copy count，以及托管Array的SetData/GetData。所有method/
signature/type/原engine target先完整核对，再注册预编译ARM64入口，没有动态trampoline。
另外增加10个SystemInfo GPU scalar查询，返回实际engine能力与64bit MaxGraphicsBufferSize。

buffer的managed IntPtr保存bridge opaque token；原Unity allocation保持private guest地址。
token单调分配、永不复用，exact live lookup含Compute/Graphics分族；释放后旧token不能
因engine复用原地址恢复效力。真实创建失败保持NULL；registration失败释放刚创建的原buffer。
VM cleanup后释放剩余原buffer。native地址、guest地址、token不混用。

Array上传仅接受exact rank-one SZ array与metadata完整pure scalar布局，拒绝reference、
Boolean、padding和IntPtr元素；精确element stride、源数组/目标buffer范围先校验。
复用pinned typed array registry与同步机制，让原engine读取guest views并写回native Array。
没有把native Array指针交给Win64，也没有假造GPU上传或Readback成功。raw native-data、
mapped Begin/EndWrite、GetNativeBufferPtr等不同pointer ABI尚不准入；需要实际调用证据后接入。

GPU族108项Mono定向回归通过，release build和all-target Clippy clean。
probe-116真实返回MaxGraphicsBufferSize=0x80000000，并创建count=64、stride=64的
ComputeBuffer。随后停在Shader.PropertyToID_Injected；VM清理时ComputeBuffer真实
finalizer调用Debug.LogWarning，因没有same-thread guest owner而abort，尚非完整验收。

后续接入Shader常用16个固定接口：PropertyToID、keyword enable/disable/query、global
int/float与Vector4/Matrix4x4读写、Compute/Graphics buffer及constant buffer绑定。
property ID仍由原Unity engine生成；float使用Win64 positional XMM；value布局先按
metadata核对，ref/out只传owned guest bytes。buffer消费者查询exact live opaque token
与allocation family后传原engine pointer；constant range还须落在原分配范围内。

终结线程logger仅允许真实Mono profiler标记的actual finalizer scope、合法LogType/
LogOption与NULL context，copy bounded UTF16到队列。只有原guest owner能drain，原logger
在queue lock外执行；owner入口/出口及VM cleanup join后均drain，最后关闭队列。
没有给foreign thread发布Memory/Runtime借用，也没有将引擎日志替换为host打印。
下一轮probe-117验证上述启动与退出路径；仍以可交互游戏为最终目标。

probe-117是installer误放owner检查导致的提前退出，已经修正。probe-118实际Shader property
ID与global ComputeBuffer绑定执行成功，资源加载返回TextAsset；止于get_bytes_Injected。
ComputeBuffer真实finalizer警告已原engine logger转发，queued=1/drained=1/pending=0，
unsafe buffer释放与shutdown callback正常。完整feature513 passed/7 ignored，Clippy clean。

TextAsset byte[]/preview byte[]/dataSize三接口随后按真实metadata成组接入。原engine创建
真正的Mono Byte[]，bridge同步guest payload并返回原rooted native identity；未发布原数据指针。
probe-119实际创建19642B数组触发旧4096元素限制；scalar数组改用既有64KiB identity slot
容量（payload≤65504B），reference/Boolean仍≤4096元素。增加准确边界回归，继续probe-120。

probe-120已真实执行两次TextAsset.get_bytes，并完成Naninovel.EngineTypes::.ctor，进入其
JSON反序列化。这里达到旧String registry的1024条上限；旧错误是容量，并非任意未知指针。
String registry改为按完整immutable header/UTF16长度分配4KiB对齐的never-reused槽位，
总arena仍是原128MiB地址区间，最多32768身份；反向BTreeMap避免线性identity扫描。
GC强根、exact native System.String检查、UTF16 terminator、readonly/noexec view和retirement
保持原契约。添加1500条长短字符串、identity重用、互不重叠和GC/退休回归，继续probe-121。

probe-121/122定位同步GC resize重入：第二份TextAsset的mono_array_new在Metadata/Memory
RefCell借用内扩容BDWGC堆，触发profiler_resize(slot4)，SharedStatic验证无法借Memory。
probe-122的完整native/Rust调用栈明确到reference_arrays::WorkerRequest::execute；不是
string长度或任意foreign thread故障。嵌套Array/String new改为prepare/execute/finish，
原native分配在无Rust borrow期间执行并保留真实临时pin，再按原registry发布identity。
GC仍开启，resize仍同步调用原Unity profiler；新增实际guest指令回调回归，定向112 passed。

probe-123实际完成19639 UTF16单位的Naninovel.EngineTypes JSON，并进入EngineConfiguration
和AudioConfiguration构造/资源加载。下一边界是普通reference对象private storage写barrier；
nested new路径改复用metadata-derived reference_storage_specs，Unity.Object继续既有serialization/
cached-pointer独立规则。没有增加Naninovel类名或构造函数名单，继续probe-124。

probe-124仍在audio resource边界；function诊断确认是mono_gc_wbarrier_set_field，目标为
native constructor刚创建并已rooted的子reference对象，不是guest stack。原nested new的
storage推广只覆盖guest主动new；现在barrier按现有root查找candidate owner，再完整核对
metadata声明storage才映射并精确store。candidate地址本身不授予write，未知地址、错owner、
非reference槽位、Unity cached指针均仍拒绝。完整feature516 passed/7 ignored。

probe-125在原Unity直接读取List backing array时遇到未映射视图；nested refresh/barrier
路径现发布有界的metadata容器图，统一真实root、typed SZ array和private corlib容器字段。
probe-126/127随后定位Dictionary.Entry.value的类型码0x15，确认为封闭泛型引用而非raw pointer。
value_layout复用closed_types验证完整实参，reference class使用reference slot，generic value
递归检查完整布局；System.Object字段同样按实际class检查。原生指针、开放泛型、伪造root、
引用伪装成scalar、padding与错位仍拒绝，不添加Naninovel类名单。

当前源码完整release feature517 passed/7 ignored，all-target Clippy -D warnings clean，
fmt与diff检查通过。日志为wire-out/native-mono-container-full-{tests,clippy,build}.log。
probe-128实测已越过AudioConfiguration及Dictionary/List图发布，进入
ProjectConfigurationProvider配置克隆。实际缺Internal_CloneSingle_Injected，随后异步异常路径
缺IsNativeAwaitableCompleted/ReleaseNativeAwaitable；下一managed入口缺
GetCurrentPlayerLoopInternal。最后158draw/53Present只是过渡画面，尚未达到标题/交互验收，
full_game_backend仍false。此次guard319/0、真实shutdown1及finalizer logger/buffer drain完成，
managed异常退出不是完整游戏成功。
