# 整体开发计划

## Phase 0：确定总体架构

```
                    Windows PE x86/x86-64
                              │
                        PE / Wine Loader
                              │
                        x86 Frontend
                              │
                 Decode + CFG + Dataflow
                              │
                       Internal IR
                              │
            LazyFLAGS / DCE / Block Fusion
                              │
              ┌───────────────┴───────────────┐
              │                               │
       Interpreter Backend              Wasm Backend
              │                               │
       Fixed-width Exec IR             IR → WebAssembly
              │                               │
       Threaded Interpreter            WebKit Wasm Engine
              │                               │
              └───────────────┬───────────────┘
                              │
                     Guest/Host Gateway
                              │
              ┌───────────────┴──────────────┐
              │                              │
       Wine Win32/NT Layer          Native Replacement
              │                    FFmpeg/zlib/codec/...
              │                              │
              └───────────────┬──────────────┘
                              │
                     iOS/macOS Host Layer
                   Metal / Audio / FS / etc.
```

核心原则：**Interpreter 保证兼容性，Wasm 提供可选高性能路径，Native Stub 消除重量级热点。**

------

## Phase 1：x86 Frontend

1. 实现 x86/x86-64 decoder。
2. 建立 guest register model。
3. 建立 guest PC 与原始 x86 byte mapping。
4. 构建 Basic Block。
5. 构建 CFG。
6. 支持直接/间接 CALL、JMP、RET。
7. 实现异常边界信息。
8. 建立 code page tracking。
9. 支持运行时发现新代码。
10. 支持自修改代码导致翻译缓存失效。

------

## Phase 2：Internal IR

设计独立于具体执行后端的寄存器式 IR。

IR 用于：

- x86 语义表达；
- CFG；
- 数据流分析；
- FLAGS 分析；
- guest state recovery；
- Interpreter lowering；
- Wasm lowering。

保持：

```
x86
 ↓
Semantic IR
 ↓
optimization
 ├─→ Execution IR
 └─→ Wasm
```

**不要直接把 Wasm 当唯一 IR。**

------

## Phase 3：LazyFLAGS

建立 FLAGS 独立数据流：

```
CF PF AF ZF SF OF
```

逐位分析：

```
READ
WRITE
DEAD
LAZY
MATERIALIZED
```

实现：

1. FLAGS liveness。
2. dead FLAGS elimination。
3. LazyFLAGS recipe。
4. CMP+Jcc fusion。
5. TEST+Jcc fusion。
6. arithmetic+Jcc fusion。
7. ADC/SBB/CMOV/SETcc 等特殊处理。
8. 异常边界 FLAGS reconstruction。
9. guest CONTEXT 精确恢复。

先做 block-local，再逐渐扩展到跨块分析。

------

## Phase 4：Execution IR

Semantic IR lowering 成专门适合解释执行的定长 IR。

目标：

```
IR / guest instruction ≤ 1.5
```

逐步加入 superinstructions：

```
CMP_BRANCH
TEST_BRANCH
LOAD_ADD
LOAD_CMP_BRANCH
ADD_STORE
CALL_DIRECT
...
```

避免为了 IR 形式漂亮而制造大量 dispatch。

------

## Phase 5：高性能 Interpreter

依次实现：

1. switch interpreter baseline。
2. threaded dispatch。
3. handler specialization。
4. register caching。
5. superinstructions。
6. fast guest-memory access。
7. branch prediction 优化。
8. block chaining。
9. translation cache。
10. hot-block profiling。

Interpreter 作为**永久 fallback backend**，不能依赖 JIT。

第一阶段目标：

```
现代 Apple Silicon：
100 MIPS+

成熟目标：
150–200 MIPS guest x86

优秀 microbenchmark：
200–300+ MIPS
```

------

## Phase 6：Guest VMM

实现统一 Guest Virtual Memory Manager：

```
Guest VA
 ├─ committed
 ├─ protection
 ├─ backing
 ├─ executable
 ├─ dirty
 ├─ translated
 └─ native mapping
```

支持：

```
VirtualAlloc
VirtualFree
VirtualProtect
MapViewOfFile
PAGE_NOACCESS
PAGE_READONLY
PAGE_READWRITE
PAGE_EXECUTE_*
```

同时负责：

```
code write
 ↓
dirty executable page
 ↓
invalidate IR
 ↓
invalidate Wasm
```

Interpreter 与 Wasm 共用同一个 VMM 语义。

------

## Phase 7：Wine 移植

将 Wine 视为需要深度适配的 Win32/NT compatibility layer，而不是普通静态库。

优先实现：

1. PE loading。
2. NTDLL 基础能力。
3. PEB/TEB。
4. TLS。
5. heap。
6. handles。
7. filesystem。
8. synchronization。
9. threading。
10. exception/SEH。
11. Win32 message loop。
12. window/input。
13. audio。
14. graphics。

逐步移除对桌面 Wine 环境的依赖。

------

## Phase 8：Guest ↔ Host ABI Gateway

统一所有跨边界调用：

```
Guest x86
 ↓
Gateway
 ↓
Native ARM64
```

建立：

```
GuestPtr<T>
HostPtr<T>
GuestHandle
NativeHandle
GuestCallback
```

禁止直接混用 guest/native pointer。

支持反向调用：

```
Native
 ↓
Callback Token
 ↓
Guest Context
 ↓
Interpreter / Wasm
 ↓
x86 callback
```

不依赖运行时生成 ARM64 trampoline。

------

## Phase 9：Native Replacement Framework

建立：

```
Native Replacement Registry
```

通过：

```
DLL name
export table
version
hash
ABI profile
```

识别库。

第一批：

```
FFmpeg
zlib
libpng
libjpeg
WebP
Vorbis
Opus
```

随后考虑：

```
SDL
OpenAL
常见图形/音频中间件
```

规则：

> 能识别 → native ARM64
> 无法识别 → fallback x86

因此 Native Stub 永远是优化，不成为兼容性依赖。

复杂结构使用：

```
Guest Object
     ↕
Shadow Object Mapping
     ↕
Native Object
```

------

## Phase 10：iOS/macOS Host Backend

实现宿主平台层：

```
Filesystem → Apple sandbox / POSIX
Window     → UIKit/AppKit
Input      → UIKit/AppKit
Graphics   → Metal
Audio      → CoreAudio/AVAudio
Timing     → Mach APIs
Threading  → pthread/GCD
```

Windows guest 不直接访问 Apple API。

------

## Phase 11：Wasm Backend 可行性验证

**先不要实现完整 x86→Wasm。**

先验证三个问题：

1. WebKit Wasm ↔ Native crossing 开销。
2. Native 是否能够高效访问 Wasm linear memory。
3. Guest VA → Wasm memory translation 成本。

重点验证：

```
Native ReadFile
      ↓
Wasm linear memory
```

是否可以接近 zero-copy。

如果必须大量：

```
native memory
 ↓ copy
Wasm memory
 ↓ copy
native memory
```

则降低 Wasm backend 优先级。

------

## Phase 12：IR → Wasm

通过可行性验证后：

```
Optimized Semantic IR
          ↓
     Wasm Lowering
          ↓
    WebKit/JSC
          ↓
      ARM64 JIT
```

直接 lower：

```
ADD → i32.add
LOAD → i32.load
CMP/Jcc → comparison + br_if
```

避免把 Wasm 做成另一个解释器：

```
错误：
Wasm → interpret IR

目标：
IR → native Wasm operations
```

LazyFLAGS 在进入 Wasm 之前完成优化。

------

## Phase 13：Wasm Memory Fast Path

建立两类 guest memory access：

```
Provably Safe
     ↓
direct Wasm load/store

Unknown / protected
     ↓
checked slow path
     ↓
Guest VMM
```

针对：

```
stack
known heap
known module data
```

尽可能生成 direct memory access。

对动态地址、特殊映射、保护页使用 slow path。

重点解决 x86-64 地址空间与 Wasm memory64/地址映射问题。

------

## Phase 14：双执行后端调度

最终：

```
                  Optimized IR
                    /      \
                   /        \
          Interpreter      Wasm
              │             │
          guaranteed      accelerated
              │             │
              └──────┬──────┘
                     ↓
              Native Gateway
```

支持：

```
Wasm available + performant
→ Wasm

Wasm unavailable
→ Interpreter

Wasm unsupported block
→ Interpreter

Native replacement
→ Native
```

三种执行模式可以共存。

------

## Phase 15：图形栈

按实际目标软件逐步支持：

```
GDI
DirectDraw
D3D9
D3D10/11
```

建立：

```
Windows Graphics
       ↓
compatibility layer
       ↓
Metal
```

必要时评估 DXVK/MoltenVK 路线，但不把它作为唯一方案。

------

## Phase 16：Profiler

从第一版开始统计：

```
Guest x86 MIPS
IR MIPS
IR / Guest
Dispatch / Guest

FLAGS materialization %
Translation cache hit %
Wasm compilation time
Wasm/native crossing count

Native replacement hit %
Native/Guest CPU ratio

Guest memory bandwidth
VMM slow-path %
Code invalidation count
```

**MIPS 始终定义为：**

\[ Guest\ MIPS= \frac{\text{retired original x86 instructions}} {\text{wall-clock seconds}\times10^6} \]

严格区分：

```
Guest MIPS
IR MIPS
Host ARM64 IPS
```

------

## Phase 17：Benchmark Suite

固定公开 x86/x86-64 测试二进制：

```
ALU
FLAGS-heavy
branch-heavy
indirect branch
load/store
random memory
SSE
x87
atomic
syscall/API-heavy
mixed workload
```

然后测试真实：

```
Win32 utility
2D application
Galgame/AVG
media-heavy application
```

分别测：

```
Interpreter
Interpreter + LazyFLAGS
Interpreter + Fusion
Interpreter + Native Stub
Wasm
Wasm + Native Stub
```

------

# 最终产品结构

```
Windows Application
        │
        ▼
    Wine / PE
        │
        ▼
   x86 Frontend
        │
        ▼
   Semantic IR
        │
   Optimization
   LazyFLAGS
        │
 ┌──────┼───────────────┐
 │      │               │
 ▼      ▼               ▼
Exec IR Wasm       Native Replacement
 │      │               │
 ▼      ▼               │
Interp WebKit JIT       │
 │      │               │
 └──────┼───────────────┘
        ▼
 Guest/Host Gateway
        │
        ▼
  iOS/macOS Host
        │
 ┌──────┼────────┬─────────┐
Metal  Audio  Filesystem  UIKit/AppKit
```

### 最终性能目标

```
Interpreter fallback:
100–200 MIPS sustained

优秀 interpreter workload:
200–300+ MIPS

Wasm/WebKit JIT:
数百 MIPS级作为目标
具体上限以实测确定

FFmpeg/codec/compression/rendering:
尽可能 Native ARM64 化
```

最终原则可以浓缩成一句：

> **IR 保证语义与可移植性，Interpreter 保证任何环境都能运行，WebAssembly/WebKit JIT 提供高性能执行路径，Wine 提供 Windows 语义，Native Replacement 则让真正昂贵的工作根本不进入 x86 执行层。**