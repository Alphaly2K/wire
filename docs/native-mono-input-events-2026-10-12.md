# 原生 Mono 输入事件桥（2026-10-12）

当前实际游戏仍未到可玩界面，full_game_backend=false。以下是正在实测的代码与边界，
不是完整游戏或性能验收。实施目录main；冻结Mono/runtime support与游戏输入不修改。

## 实际边界

probe-105执行ShouldRunUpdate(enum,out bool)并进入NotifyDeviceDiscovered(int,string)，
JsonUtility.FromJson回调缺nested mono_unity_string_empty_wrapper。新路由调用原Mono接口，
核对当前domain，复用canonical字符串strong roots与immutable payload；没有把空字符串改成NULL。

probe-106枚举真实Keyboard/Mouse（RawInput），执行设备JSON与容器/unsafe allocations，
随后捕获MallocTracked、IOCTL等缺失异常，最后停止在NotifyUpdate(enum,IntPtr)。
不能把捕获异常后继续启动当成设备创建成功。probe-107完成tracked ABI安装与InputModule
布局验证，但键盘初始化申请allocator=Temp（2）时被旧仅TempJob/Persistent范围拒绝。

## 固定ABI与所有权

- MallocTracked实际为Void*(Int64,Int32,Allocator,Int32 skip,IntPtr context)，冻结目标
  0x1800674d0；FreeTracked(Void*,Allocator)为0x1800675a0。保留原engine allocator计账，
  native backing私有；record区分tracked/untracked，释放、finalizer queue、失败回滚及
  VM cleanup都调用匹配的原engine释放族。context仍须0。
- Temp仅允许同步owner route scope，原native IL显式Free/FreeTracked后退休native backing。
  外层scope返回引擎调度器之前验证没有live Temp。越过这个边界仍有Temp则停止，
  不伪造scheduler reset，也不准许跨帧Temp。原engine Free为no-op的情况仍执行该实际API。
- IOCTL实际Int64(Int32 device,Int32 FourCC,IntPtr data,Int32 size)，0x180132940。
  只允许已核对的inline payload commands，FourCC采用冻结IL的big-endian字符构造：
  QENB/QBGD为1B；ENBL/DSBL/RSET/SYNC/HIDS为0B；KBLT为256B；HIDD为受限descriptor bytes。
  载荷复制到private guest slot，原engine返回与输出完整传回；不传递宿主指针或未知嵌套指针。
  空载荷仍保持NULL/非NULL区别。
- NotifyUpdate是原managed IL，不是stub。安装时逐项核对concrete static managed方法、IL
  header、enum/IntPtr/void签名。IntPtr只在这条固定callback解释为event buffer，不扩大
  ordinary Frame的IntPtr/raw-pointer准入。
- NativeInputEventBuffer真实pack1/size20，eventBuffer@0、eventCount@8、sizeInBytes@12、
  capacityInBytes@16；核对所有instance field名称/offset/type与Void*元素。callback header
  必须是当前live guest callback stack；event data须owned noncode private engine data。
  count/size/capacity检查为非负、size≤capacity≤1MiB、count≤size/20。
- 原event data复制到8B对齐私有InputSpan，跨度由拥有backing的RAII capability管理。
  原IL/delegate接收native header与native event pointer；UnsafeUtility.MemCpy可访问此
  同步input span，但不能用allocator/free释放它。复制检查两端完整范围后才写。
- 结束时原IL若保留借入buffer，容量不能增加；若返回新buffer，必须是已登记live
  native Unity allocation，转回其真实guest地址。检查全部输出后写回事件及header；
  header绝不含native地址。scope退出撤销copy capability。尚待真实输入循环验证。

安装后97项Mono定向回归通过；后续新增copy capability退休与tracked family回归待运行。
input family release build与all-target Clippy已通过（Temp scope新增后另验）。

probe-108实际完成Keyboard/Mouse创建，QENB两次原engine返回-1、SYNC两次返回1；
tracked Temp均在IL的Dispose中调用原FreeTracked，没有设备创建MissingMethodException。
随后输入header被错误地要求VirtualAlloc ownership：callback stack由私有Memory.map建立，
不是guest VirtualAlloc。已修为header用当前callback stack和noncode检查；event payload
另外核对Runtime live heap/buffer或guest private VirtualAlloc，二者不混用。继续复测。

## probe-109/110 与后续接口

probe-109/110实际执行NotifyUpdate原IL/delegate：2个事件、100字节输入和输出，
Keyboard/Mouse初始化中原QENB返回-1、SYNC返回1均保留。没有把输入回调替换为stub。
随后新增逐事件size与4字节对齐链校验，在调用native IL与输出写回之前检查完整span。

11个输出字符串adapter与24个scalar接口已安装：Object名称/ToString、Application路径/
产品信息，Debug.isDebugBuild、帧率/vSync与Time getter。返回字符串使用私有UTF16副本，
BindingsAllocator.Free只接受exact live output，原engine执行释放；native backing退休至
VM cleanup，NULL与empty保持区别。probe-110原Debug.isDebugBuild实际返回false，
最后停止在PlayerPrefs.HasKey_Injected；未到Naninovel标题。

字符串/input/scalar批次完整feature回归506 passed、7 ignored，all-target release Clippy
通过。PlayerPrefs下一批成组实现10个整数/浮点/字符串读写、查询、删除、Save接口，
原Unity存储逻辑继续执行；多个UTF16 span合并为单个scoped guest allocation，XMM参数
保留位型，输出字符串复用同一arena与原engine Free。首轮103项Mono定向回归通过，
新增逐事件链回归及实际游戏复测正在继续。

probe-111原PlayerPrefs.HasKey实际返回false。probe-112实际Resources.Load执行后返回NULL，
游戏正常进入fallback创建ScriptableObject；停止于其中`_data`嵌套值字段的旧纯scalar限制。
资源族7接口已成组验证metadata并安装，LoadAll返回真实rooted array，普通Unity.Object
返回真实native v2 GC handle；ResourceRequest/UnloadUnusedAssets按实际IL marshaller返回
受检原engine opaque pointer，不能错误地当GC handle转换。

字段桥新增metadata-derived nested value groups：native/guest每个声明leaf精确读写，
引用槽转canonical strong-root token，写回使用完整reference receiver的write barrier。
所有padding新建为零，不复制native padding；每批同步、刷新、GC核对前验证全部
Boolean、引用与padding，再进行native写入。整字段mono_field_get/set仍按父字段ABI，
不能把子值类型的MonoClassField当作完整receiver字段。回归验证后部非法字段不能让
前部scalar提前写入、原native padding不泄漏、GC owner、whole-field读写与退休撤销。
105项Mono定向回归通过，继续probe-113；可交互游戏尚未验收。

probe-113实际完成两次Unity.VisualScripting.VariablesAsset构造并到Screen.SetResolution。
Screen分辨率/当前分辨率adapter验证RefreshRate=8B/align4、Resolution=16B/align4的
实际metadata layout，保留Win64第4参数位置的ref刷新率；全屏Boolean/enum、sleepTimeout
共6个scalar接口同步补齐。当前完整feature509 passed/7 ignored，Screen后定向105 passed。
继续probe-114，仍未到游戏标题。
