# R2 矩形与窗口边框（2026-10-18）

状态：旧行为独立基线通过，13 项契约、纯核和适配器草稿尚未接入。

## 独立基线

在 `f6f6fbc` 的旧 geometry 运行时上，六项独立测试通过。PE32 固定十项 RECT 的 signed/wrapping/empty/交并集规则、重叠 copy、PtInRect 三个 stack words、handler 后读取返回槽、跨页输出 fault 前缀不变；三项边框调整保留 96-DPI profile 与 unsupported-DPI 的 LastError 120。Win64 固定 packed 64-bit POINT、DWORD 高位截断、静态 kernel32/kernelbase/user32 与动态 user32 范围、七项 rectangle helper absent、unsupported-DPI 的 LastError 50、saved return frame 和跨页输出 fault 不提交 CPU/API。

首轮测试 module 缺少显式 path 属性，编译失败；修正注册路径后六项在旧运行时上通过，运行时未修改。证据 `r2-geometry-baseline-tests.txt`、`r2-geometry-baseline-tests-corrected.txt`。

原创可选 `gateway-rectangle` 循环 OffsetRect(+1, −1)，stack 输出，消费 BOOL 与累计 x 坐标，checksum 2n。旧冻结 `r2-input-profile-wire/wire` 上双架构 12K 验证通过，PE32 raw/static IR 均通过。首次 PE32 预期把终止 int3 误计为退休指令，结果相差一条；修正为 8n+15 后通过，未改变 fixture 机器码或运行时。原失败与修正证据 `r2-geometry-fixture-baseline32{,-corrected}`，Win64 为 `r2-geometry-fixture-baseline64`。工具 20 项通过，默认 case 集合保持。

证据根为 `wire-out/refactoring-2026-10-14/`。本批尚不计入已验收 **424 exports**。
