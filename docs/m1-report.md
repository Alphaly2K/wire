# M1 里程碑报告：x86 解释器核心 + 差分测试

日期：2026-10-04。宿主机：macOS 26.5.1 / Apple Silicon (arm64)。

## 验收对照

| 验收项 | 结果 |
| --- | --- |
| 覆盖目标 EXE 实际用到的指令集 | 通过（游戏路径出现 103 种 opcode，全部被实现并纳入测试；另含 16 位操作数、位操作、x87 控制类等未出现但大概率用到的族） |
| 差分测试：同一指令序列在解释器与参照系上执行，比对寄存器/标志位/内存 | 通过（参照系 = Unicorn 2.1.5，QEMU 系 x86 核，ARM64 macOS 原生运行，仅 dev-dependency） |
| 差分测试全绿 | 通过（24 个测试族、约 6000 个随机用例，`cargo test --release` 全绿，0.5s） |
| 对照程序在纯解释路径下跑出正确结果 | 通过（退出码 0，与 M0 一致） |

## 方法（tests/difftest.rs）

- 模板化指令生成：指针类寄存器恒指向 DATA 区保证内存安全；需要受控值的场景
  （SIB 索引、除法防 #DE）用前置 flag-preserving mov 内联解决。
- 初始状态随机化：8 寄存器、EFLAGS（FLAG_MASK 内随机）、16KB 数据区。
- 终态比对：全部 GPR、EFLAGS（按族裁剪掩码，剔除 ISA 未定义位）、数据区、栈区。
- 故障等价：双侧都 fault 时要求 fault EIP 一致。
- 未定义标志按族处理：移位 AF/OF(次数≠1)、乘除 SZP/AF、BT 的 CF 外其余位、
  BSF/BSR 的 ZF 外其余位。
- 附属工具 `examples/lockstep.rs`：单步锁步比对器，输入任意字节序列逐指令
  定位首个分歧点（本轮全部疑难 bug 均由它秒级定位）。

## 差分测试发现的解释器 bug（已全部修复）

1. **移位组 /5、/6 错位**（高危）：Intel 编码 /5=SHR、/6=SAL(=/4)。
   实现把 SHR 当成 SAR、把 SAL 当成 SHR——正数时 SHR/SAR 结果相同所以
   偶发通过，符号位置位时结果错误。游戏因此多推进了 3 倍指令数。
2. **MUL r/m32（无符号 32 位乘）CF/OF 写反**：溢出时清零、未溢出时置位。
   8/16 位与有符号路径正确，仅此一处常量写反。
3. **CMOVcc 内存操作数在条件不成立时未读取**：x86 架构上 cmov 无论条件
   成立与否都会读内存（可能产生缺页），只读条件成立一侧会漏掉 fault。
4. **POPF 可置位 TF**：真实硬件下一条指令会触发 #DB 单步异常。本实现不
   模拟 #DB，TF 恒屏蔽（已登记为简化假设；unicorn 侧则会 UC_ERR_EXCEPTION，
   测试族对 popf 仅使用 pushf;popf 配对规避）。

## 游戏 EXE 回归（M0 → M1 对比）

- M0 终点：16985 指令，C++ `throw`（RaiseException）终止。
- M1 终点：**51186 指令 / 470 次 API 调用**，引擎初始化大幅推进：
  出现了 `CreateThread`（引擎工作线程）与 `CreateEventW`。
  当前阻断点全部是 API 级：线程/事件未实现（返回失败假值）→ 后续路径
  空指针解引用。解释器本身无未支持指令。
- 指令吞吐（release）：对照程序 2.7 Minstr/s，游戏路径 11 Minstr/s
  （无参考意义之外的观测：API 密集的 CRT 路径比纯代码路径慢）。

## M2 输入（按游戏日志优先级，更新）

1. RaiseException / SEH 展开（M0 遗留，仍在清单）
2. **线程与同步原语**（CreateThread/CreateEventW 已出现；临界区 no-op 假设到期）
3. 真实堆（bump heap 已支撑 470 次调用无压力，但 free/realloc 语义即将需要）
4. 文件 I/O（data.xp3 读取在即）
5. GetCPInfo、IsProcessorFeaturePresent（SSE2 检测——SSE 子集排期信号）

## 复现

```sh
cargo test --release                 # 差分测试（24 族全绿）
cargo run --example lockstep -- <hex> [--regs ...] [--eflags ...]
wire run "path/to/game.exe" --max-instr 50000000
```
