use wire::x64::{
    cpu::Cpu64,
    memory::{Memory, EXECUTE, READ, WRITE},
    pe::Pe64,
};
const BASE: u64 = 0x140000000;
const STACK: u64 = 0x7ff000000000;
fn put16(b: &mut [u8], at: usize, n: u16) {
    b[at..at + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn put64(b: &mut [u8], at: usize, n: u64) {
    b[at..at + 8].copy_from_slice(&n.to_le_bytes());
}
fn fixture() -> Vec<u8> {
    let mut b = vec![0; 0x600];
    b[..2].copy_from_slice(b"MZ");
    put32(&mut b, 0x3c, 0x80);
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    put16(&mut b, 0x84, 0x8664);
    put16(&mut b, 0x86, 1);
    put16(&mut b, 0x94, 0xf0);
    let opt = 0x98;
    put16(&mut b, opt, 0x20b);
    put32(&mut b, opt + 16, 0x1000);
    put64(&mut b, opt + 24, BASE);
    put32(&mut b, opt + 32, 0x1000);
    put32(&mut b, opt + 36, 0x200);
    put32(&mut b, opt + 56, 0x2000);
    put32(&mut b, opt + 60, 0x200);
    put64(&mut b, opt + 72, 0x100000);
    put64(&mut b, opt + 80, 0x1000);
    put32(&mut b, opt + 108, 16);
    let s = opt + 0xf0;
    b[s..s + 5].copy_from_slice(b".text");
    put32(&mut b, s + 8, 0x400);
    put32(&mut b, s + 12, 0x1000);
    put32(&mut b, s + 16, 0x400);
    put32(&mut b, s + 20, 0x200);
    put32(&mut b, s + 36, 0xe0000020);
    // mov r8,0x123456789abcdef0; mov rax,r8; ret
    b[0x200..0x20e].copy_from_slice(&[
        0x49, 0xb8, 0xf0, 0xde, 0xbc, 0x9a, 0x78, 0x56, 0x34, 0x12, 0x4c, 0x89, 0xc0, 0xc3,
    ]);
    b
}
#[test]
fn pe32plus_loads_high_addresses_dir64_iat_and_rejects_bad_ranges() {
    let mut b = fixture();
    let opt = 0x98;
    put32(&mut b, opt + 112 + 5 * 8, 0x1100);
    put32(&mut b, opt + 116 + 5 * 8, 12);
    put32(&mut b, 0x300, 0x1000);
    put32(&mut b, 0x304, 12);
    put16(&mut b, 0x308, 0xa080);
    put64(&mut b, 0x280, BASE + 0x1234);
    put32(&mut b, opt + 112 + 8, 0x1140);
    put32(&mut b, opt + 116 + 8, 40);
    put32(&mut b, 0x340, 0x1180);
    put32(&mut b, 0x34c, 0x11c0);
    put32(&mut b, 0x350, 0x1190);
    put64(&mut b, 0x380, 0x11d0);
    b[0x3c0..0x3cd].copy_from_slice(b"kernel32.dll\0");
    b[0x3d2..0x3de].copy_from_slice(b"ExitProcess\0");
    let pe = Pe64::parse(&b).unwrap();
    let imports = pe.imports(&b).unwrap();
    assert_eq!(imports[0].functions[0].name.as_deref(), Some("ExitProcess"));
    let relocated = BASE + 0x200000;
    let mem = pe
        .load(&b, relocated, |dll, f| {
            assert_eq!(dll, "kernel32.dll");
            assert_eq!(f.iat_rva, 0x1190);
            Ok(0x7ffe12345678)
        })
        .unwrap();
    assert_eq!(
        mem.read_int(relocated + 0x1080, 8).unwrap(),
        relocated + 0x1234
    );
    assert_eq!(mem.read_int(relocated + 0x1190, 8).unwrap(), 0x7ffe12345678);
    assert!(mem.fetch(relocated, &mut [0]).is_err());
    let mut code = [0; 2];
    mem.fetch(relocated + 0x1000, &mut code).unwrap();
    assert_eq!(code, [0x49, 0xb8]);
    put16(&mut b, 0x308, 0x3080);
    assert!(pe.load(&b, relocated, |_, _| Ok(0)).is_err());
    put32(&mut b, 0x98 + 0xf0 + 12, 0x100);
    assert!(Pe64::parse(&b).is_err());
    for n in [0, 63, 128, 200, 511] {
        assert!(Pe64::parse(&b[..n]).is_err());
    }
}
#[test]
fn sparse_high_address_memory_is_checked_and_cross_page_writes_are_atomic() {
    let mut m = Memory::default();
    m.map(BASE, 8192, READ | WRITE).unwrap();
    m.write(BASE + 4094, &[1, 2, 3, 4]).unwrap();
    m.protect(BASE + 4096, 4096, READ).unwrap();
    assert!(m.write(BASE + 4094, &[5, 6, 7, 8]).is_err());
    let mut b = [0; 4];
    m.read(BASE + 4094, &mut b).unwrap();
    assert_eq!(b, [1, 2, 3, 4]);
    assert!(m.fetch(BASE, &mut [0]).is_err());
    assert!(m.read(1 << 47, &mut [0]).is_err());
    assert!(m.read(u64::MAX, &mut [0; 2]).is_err());
    assert!(m.map(BASE + 4096, 8192, READ).is_err());
    assert!(m.read(BASE + 8192, &mut [0]).is_err());
}
#[test]
fn amd64_high_registers_widths_rip_relative_and_gateway_shadow_space() {
    let b = fixture();
    let pe = Pe64::parse(&b).unwrap();
    let mut m = pe
        .load(&b, BASE, |_, _| Err("unexpected import".into()))
        .unwrap();
    m.map(STACK, 4096, READ | WRITE).unwrap();
    let rsp = STACK + 0xfc8;
    m.write_int(rsp, 8, BASE + 0x1300).unwrap();
    let mut cpu = Cpu64::new(BASE + 0x1000, rsp);
    for _ in 0..3 {
        cpu.step(&mut m).unwrap();
    }
    assert_eq!(cpu.regs[0], 0x123456789abcdef0);
    assert_eq!(cpu.regs[8], cpu.regs[0]);
    assert_eq!(cpu.rip, BASE + 0x1300);
    assert_eq!(cpu.regs[4], rsp + 8);
    // 32-bit writes zero extend; high byte retains the other 56 bits; REX selects SPL.
    m.write(
        BASE + 0x1300,
        &[
            0x41, 0xb8, 1, 0, 0, 0, 0xb4, 0x7f, 0x48, 0x8b, 0x05, 0x11, 0, 0, 0,
        ],
    )
    .unwrap();
    cpu.rip = BASE + 0x1300;
    cpu.step(&mut m).unwrap();
    assert_eq!(cpu.regs[8], 1);
    cpu.step(&mut m).unwrap();
    assert_eq!(cpu.regs[0], 0x123456789abc7ff0);
    m.write_int(BASE + 0x1320, 8, 0x12345678ffffffff).unwrap();
    cpu.step(&mut m).unwrap();
    assert_eq!(cpu.regs[0], 0x12345678ffffffff);
    cpu.regs[4] = rsp;
    cpu.regs[1] = BASE;
    cpu.regs[2] = 1;
    cpu.regs[8] = 2;
    cpu.regs[9] = 3;
    m.write_int(rsp + 40, 8, STACK).unwrap();
    m.write_int(rsp + 48, 8, 0xfedcba9876543210).unwrap();
    assert_eq!(
        cpu.arguments(&m, 6).unwrap(),
        [BASE, 1, 2, 3, STACK, 0xfedcba9876543210]
    );
    cpu.return_integer(&m, 0x123456789abcdef0).unwrap();
    assert_eq!(cpu.regs[4], rsp + 8);
    assert_eq!(cpu.regs[0], 0x123456789abcdef0);
}
#[test]
fn amd64_faults_do_not_commit_registers_flags_or_store_and_fetch_stops_at_page_end() {
    let mut m = Memory::default();
    m.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    m.write(BASE + 4095, &[0x90]).unwrap();
    let mut cpu = Cpu64::new(BASE + 4095, STACK);
    cpu.step(&mut m).unwrap();
    assert_eq!(cpu.rip, BASE + 4096);
    m.write(BASE, &[0x48, 0x83, 0x00, 1]).unwrap();
    cpu.rip = BASE;
    cpu.regs[0] = STACK;
    let before = cpu.clone();
    assert!(cpu.step(&mut m).is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rflags, before.rflags);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(cpu.retired, before.retired);
    for code in [
        [0x0f, 0x18, 0x00],
        [0x0f, 0x18, 0x08],
        [0x0f, 0x18, 0x10],
        [0x0f, 0x18, 0x18],
        [0x0f, 0x0d, 0x08],
    ] {
        m.write(BASE, &code).unwrap();
        cpu.rip = BASE;
        cpu.regs[0] = 1 << 48; // Hints must not fault on an inaccessible/noncanonical operand.
        let before = cpu.clone();
        cpu.step(&mut m).unwrap();
        assert_eq!(cpu.regs, before.regs);
        assert_eq!(cpu.rflags, before.rflags);
        assert_eq!(cpu.rip, BASE + 3);
    }
    m.write(BASE, &[0xf0, 0x0f, 0x18, 0x00]).unwrap();
    cpu.rip = BASE;
    let before = cpu.clone();
    assert!(cpu.step(&mut m).is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
}
#[test]
fn amd64_integer_flags_and_rex_register_aliases_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let registers = [
        U::RAX,
        U::RCX,
        U::RDX,
        U::RBX,
        U::RSP,
        U::RBP,
        U::RSI,
        U::RDI,
        U::R8,
        U::R9,
        U::R10,
        U::R11,
        U::R12,
        U::R13,
        U::R14,
        U::R15,
    ];
    let code = [
        0x4d, 0x01, 0xc8, 0x4d, 0x11, 0xd0, 0x4d, 0x29, 0xd8, 0x4d, 0x19, 0xe0, 0x49, 0xff, 0xc0,
        0x49, 0xff, 0xc8, 0x4d, 0x31, 0xe8, 0x4d, 0x09, 0xf0, 0x4d, 0x21, 0xf8, 0x4c, 0x89, 0xc0,
        0xb4, 0x7f, 0x40, 0xb5, 0x12, 0x41, 0xb9, 1, 0, 0, 0,
    ];
    let mut random = 0x1324bfe3718d6a90u64;
    for _ in 0..256 {
        let mut memory = Memory::default();
        memory.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.write(BASE, &code).unwrap();
        let mut cpu = Cpu64::new(BASE, STACK);
        for r in &mut cpu.regs {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            *r = random;
        }
        cpu.rflags = 0x202 | (random & 0x8d5);
        let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
        uc.mem_write(BASE, &code).unwrap();
        for (r, value) in registers.iter().zip(cpu.regs) {
            uc.reg_write(*r, value).unwrap();
        }
        uc.reg_write(U::RFLAGS, cpu.rflags).unwrap();
        while cpu.rip < BASE + code.len() as u64 {
            uc.emu_start(cpu.rip, BASE + code.len() as u64, 0, 1)
                .unwrap();
            cpu.step(&mut memory).unwrap();
            for (r, value) in registers.iter().zip(cpu.regs) {
                assert_eq!(
                    uc.reg_read(*r).unwrap(),
                    value,
                    "{r:?} after {:#x}",
                    cpu.rip
                );
            }
            // Logical AF is undefined; arithmetic comparison includes it.
            assert_eq!(
                uc.reg_read(U::RFLAGS).unwrap() & 0x8c5,
                cpu.rflags & 0x8c5,
                "flags after {:#x}",
                cpu.rip
            );
        }
    }
}

#[test]
fn amd64_cli_probe_executes_high_entry_and_fails_on_unresolved_import() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("wire-x64-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("probe.exe");
    let mut b = fixture();
    std::fs::write(&path, &b).unwrap();
    let binary = env!("CARGO_BIN_EXE_wire");
    let output = Command::new(binary)
        .args(["probe64", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("entry returned RAX=0x123456789abcdef0; retired=3"));
    // call [RIP-relative IAT] reaches the unresolved import marker after a real CALL.
    put32(&mut b, 0x98 + 120, 0x1140);
    put32(&mut b, 0x98 + 124, 40);
    put32(&mut b, 0x340, 0x1180);
    put32(&mut b, 0x34c, 0x11c0);
    put32(&mut b, 0x350, 0x1190);
    put64(&mut b, 0x380, 0x11d0);
    b[0x3c0..0x3cd].copy_from_slice(b"kernel32.dll\0");
    b[0x3d2..0x3de].copy_from_slice(b"ExitProcess\0");
    b[0x200..0x207].copy_from_slice(&[0xff, 0x15, 0x8a, 1, 0, 0, 0xc3]);
    std::fs::write(&path, &b).unwrap();
    let output = Command::new(binary)
        .args(["probe64", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("import boundary: kernel32.dll!ExitProcess; retired=1"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not run the game"));
    std::fs::remove_dir_all(dir).unwrap();
}

fn startup() -> (Memory, Cpu64, wire::x64::runtime::Runtime64) {
    let mut mem = Memory::default();
    mem.map(STACK, 8192, READ | WRITE).unwrap();
    let rsp = STACK + 0x1f88;
    mem.write_int(rsp, 8, BASE).unwrap();
    let mut cpu = Cpu64::new(BASE + 0x1000, rsp);
    cpu.gs_base = wire::x64::runtime::TEB;
    let runtime =
        wire::x64::runtime::Runtime64::new(&mut mem, BASE, STACK, 8192, "\"C:\\probe.exe\"")
            .unwrap();
    (mem, cpu, runtime)
}
fn startup_call(
    mem: &mut Memory,
    cpu: &mut Cpu64,
    runtime: &mut wire::x64::runtime::Runtime64,
    name: &str,
    args: &[u64],
) -> u64 {
    startup_dll_call(mem, cpu, runtime, "KERNEL32.dll", name, args)
}
#[test]
fn win64_get_proc_address_null_module_resolves_main_and_missing_exports_return_null() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let symbol = STACK + 0x200;
    mem.write(symbol, b"Exported\0").unwrap();
    let mut exports = wire::x64::pe::Exports64::default();
    exports.names.insert("Exported".into(), BASE + 0x2345);
    exports.ordinals.insert(7, BASE + 0x2345);
    runtime.register_module("probe.exe", BASE, exports);
    for module in [0, BASE] {
        for name in [symbol, 7] {
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    "GetProcAddress",
                    &[module, name]
                ),
                BASE + 0x2345
            );
        }
    }
    for (module, error) in [(0, 127), (BASE, 127), (0x123456780, 126)] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetProcAddress",
                &[module, 8]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            error
        );
    }
}
#[test]
fn win64_dynamic_kernel_exports_are_available_before_static_gateway_registration() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let text = STACK + 0x200;
    runtime.register_provider("kernel32.dll");
    mem.write(text, b"kernel32.dll\0").unwrap();
    let module = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleHandleA",
        &[text],
    );
    for name in [
        "GetConsoleCP",
        "GetThreadContext",
        "GetFileAttributesExW",
        "CreateWaitableTimerExW",
        "SetThreadErrorMode",
        "GetThreadErrorMode",
    ] {
        assert!(runtime
            .gateways
            .values()
            .all(|n| !n.ends_with(&format!("!{name}"))));
        mem.write(text, format!("{name}\0").as_bytes()).unwrap();
        let p = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[module, text],
        );
        assert_ne!(p, 0, "{name}");
        assert_eq!(runtime.gateways[&p], format!("kernel32.dll!{name}"));
    }
    for name in [
        "CoCreateInstance",
        "BCryptGenRandom",
        "MessageBoxW",
        "DoesNotExist",
    ] {
        mem.write(text, format!("{name}\0").as_bytes()).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetProcAddress",
                &[module, text]
            ),
            0,
            "{name}"
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            127
        );
    }
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetConsoleCP", &[]),
        932
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetThreadErrorMode",
            &[0x8000, text]
        ),
        1
    );
    assert_eq!(mem.read_int(text, 4).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadErrorMode", &[]),
        0x8000
    );
}
#[test]
fn win64_extended_codepage_info_has_windows_layout_and_checked_full_outputs() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x400;
    for (name, size, cp, maximum, unicode_default) in [
        ("GetCPInfoExW", 544, 0, 2, 0x30fb),
        ("GetCPInfoExA", 284, 1252, 1, 0x3f),
        ("GetCPInfoExW", 544, 65001, 4, 0xfffd),
    ] {
        mem.write(output, &[0xaa; 560]).unwrap();
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, name, &[cp, 0, output]),
            1
        );
        assert_eq!(mem.read_int(output, 4).unwrap(), maximum);
        assert_eq!(mem.read_int(output + 18, 2).unwrap(), unicode_default);
        assert_eq!(
            mem.read_int(output + 20, 4).unwrap(),
            if cp == 0 { 932 } else { cp }
        );
        assert_ne!(mem.read_int(output + 24, 1).unwrap(), 0);
        assert_eq!(mem.read_int(output + size - 1, 1).unwrap(), 0);
        assert_eq!(mem.read_int(output + size, 1).unwrap(), 0xaa);
    }
    mem.write(output, &[0xaa; 560]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetCPInfoExW",
            &[0, 1, output]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1004
    );
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = 0;
    cpu.regs[2] = 0;
    cpu.regs[8] = STACK + 8192 - 16;
    mem.write(cpu.regs[8], &[0xaa; 16]).unwrap();
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "GetCPInfoExW", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(
        mem.read_int(STACK + 8192 - 16, 8).unwrap(),
        0xaaaaaaaaaaaaaaaa
    );
}
#[test]
fn win64_small_heap_blocks_pack_reuse_and_reallocate_without_overlapping_live_data() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let heap = startup_call(&mut mem, &mut cpu, &mut runtime, "GetProcessHeap", &[]);
    let committed = |m: &Memory| {
        let s = m.memory_status();
        u64::from_le_bytes(s[8..16].try_into().unwrap())
            - u64::from_le_bytes(s[16..24].try_into().unwrap())
    };
    let baseline = committed(&mem);
    let mut blocks = Vec::new();
    for n in 0..4096u64 {
        let p = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapAlloc",
            &[heap, 8, 28],
        );
        assert_ne!(p, 0);
        assert_eq!(p % 16, 0);
        assert_eq!(mem.read_int(p, 8).unwrap(), 0);
        mem.write_int(p, 8, n + 1).unwrap();
        blocks.push(p);
    }
    assert!(committed(&mem) - baseline <= 128 * 1024);
    for (n, &p) in blocks.iter().enumerate() {
        assert_eq!(mem.read_int(p, 8).unwrap(), n as u64 + 1);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "HeapSize", &[heap, 0, p]),
            28
        );
    }
    let p = blocks.pop().unwrap();
    startup_call(&mut mem, &mut cpu, &mut runtime, "HeapFree", &[heap, 0, p]);
    let reused = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "HeapAlloc",
        &[heap, 8, 28],
    );
    assert_eq!(reused, p);
    assert_eq!(mem.read_int(reused, 8).unwrap(), 0);
    mem.write_int(reused, 8, 0x123456789abcdef0).unwrap();
    let grown = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "HeapReAlloc",
        &[heap, 8, reused, 100],
    );
    assert_eq!(mem.read_int(grown, 8).unwrap(), 0x123456789abcdef0);
    assert_eq!(mem.read_int(grown + 92, 8).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapSize",
            &[heap, 0, grown]
        ),
        100
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapReAlloc",
            &[heap, 0, grown, 20_000_000]
        ),
        0
    );
    assert_eq!(mem.read_int(grown, 8).unwrap(), 0x123456789abcdef0);
    for p in blocks.into_iter().chain(Some(grown)) {
        startup_call(&mut mem, &mut cpu, &mut runtime, "HeapFree", &[heap, 0, p]);
    }
    assert!(committed(&mem) - baseline <= 128 * 1024); // one empty slab retained per used bucket
}

#[test]
fn win64_com_missing_factory_and_multimedia_timer_reference_balance() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let data = STACK + 0x200;
    mem.write(data, &[0x55; 40]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "CoCreateInstance",
            &[data, 0, 1, data + 16, data + 32]
        ),
        0x800401f0
    );
    assert_eq!(mem.read_int(data + 32, 8).unwrap(), 0);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "CoInitializeEx",
            &[0, 0]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "CoCreateInstance",
            &[data, 0, 1, data + 16, data + 32]
        ),
        0x80040154
    );
    for (name, period, expected) in [
        ("timeEndPeriod", 1, 97),
        ("timeBeginPeriod", 0, 97),
        ("timeBeginPeriod", 1, 0),
        ("timeBeginPeriod", 1, 0),
        ("timeEndPeriod", 1, 0),
        ("timeEndPeriod", 1, 0),
        ("timeEndPeriod", 1, 97),
    ] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "winmm.dll",
                name,
                &[period]
            ),
            expected
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "winmm.dll",
            "timeGetDevCaps",
            &[data, 8]
        ),
        0
    );
    assert_eq!(mem.read_int(data, 4).unwrap(), 1);
    assert_eq!(mem.read_int(data + 4, 4).unwrap(), u32::MAX as u64);
}
#[test]
fn win64_waitable_timers_reset_consume_cancel_and_share_named_object_identity() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let data = STACK + 0x200;
    mem.write(data, &[0; 8]).unwrap();
    mem.write(
        data + 16,
        &"Timer\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for manual in [0, 1] {
        let h = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateWaitableTimerExW",
            &[0, 0, manual, 0x1f0003],
        );
        assert_ne!(h, 0);
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[h, 0]
            ),
            258
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SetWaitableTimer",
                &[h, data, 0, 0, 0, 0]
            ),
            1
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[h, 0]
            ),
            0
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "CancelWaitableTimer",
                &[h]
            ),
            1
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[h, 0]
            ),
            if manual != 0 { 0 } else { 258 }
        );
        mem.write_int(data, 8, (-100_000_000i64) as u64).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SetWaitableTimer",
                &[h, data, 0, 0, 0, 0]
            ),
            1
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[h, 0]
            ),
            258
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[h]),
            1
        );
        mem.write_int(data, 8, 0).unwrap();
    }
    let h = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateWaitableTimerW",
        &[0, 0, data + 16],
    );
    let alias = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "OpenWaitableTimerW",
        &[0x100000, 0, data + 16],
    );
    assert_ne!(alias, 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetWaitableTimer",
            &[alias, data, 0, 0, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        5
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetWaitableTimer",
            &[h, data, 0, 0, 0, 0]
        ),
        1
    );
    mem.write_int(data + 64, 8, h).unwrap();
    mem.write_int(data + 72, 8, alias).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, data + 64, 1, 0]
        ),
        u32::MAX as u64
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        87
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[alias, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[h, 0]
        ),
        258
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateEventW",
            &[0, 0, 0, data + 16]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
}
#[test]
fn amd64_suspended_thread_context_reads_saved_guest_registers_across_single_step_budgets() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x590000000;
    const WORKER: u64 = CODE + 4096;
    const DATA: u64 = CODE + 8192;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[u64]) {
        for (n, &value) in args.iter().enumerate() {
            mov(code, if n < 4 { [1, 2, 8, 9][n] } else { 0 }, value);
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        mov(code, 0, p.runtime.register("kernel32.dll", name));
        code.extend([0xff, 0xd0]);
    }
    let path =
        std::env::temp_dir().join(format!("wire-thread-context64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    p.memory
        .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    let parameter = 0x8877665544332211;
    let magic = 0x123456789abcdef0;
    for offset in [0, 1536] {
        p.memory.write(DATA + offset, &[0xaa; 1232]).unwrap();
        p.memory.write_int(DATA + offset + 48, 4, 0x100003).unwrap();
    }
    let mut main = vec![0x48, 0x83, 0xec, 0x38];
    call(
        &mut p,
        &mut main,
        "CreateThread",
        &[0, 0, WORKER, parameter, 4, 0],
    );
    main.extend([0x49, 0x89, 0xc4, 0x4c, 0x89, 0xe1]); // r12=handle, rcx=r12
    mov(&mut main, 2, DATA);
    call(&mut p, &mut main, "GetThreadContext", &[]);
    mov(&mut main, 7, DATA + 3072);
    main.extend([0x48, 0x89, 0x07, 0x4c, 0x89, 0xe1]);
    call(&mut p, &mut main, "ResumeThread", &[]);
    call(&mut p, &mut main, "SwitchToThread", &[]);
    main.extend([0x4c, 0x89, 0xe1]);
    call(&mut p, &mut main, "SuspendThread", &[]);
    main.extend([0x4c, 0x89, 0xe1]);
    mov(&mut main, 2, DATA + 1536);
    call(&mut p, &mut main, "GetThreadContext", &[]);
    main.extend([0x48, 0x83, 0xc4, 0x38, 0xc3]);
    p.memory.write(CODE, &main).unwrap();
    let mut worker = Vec::new();
    mov(&mut worker, 13, magic);
    worker.extend([0xeb, 0xfe]);
    p.memory.write(WORKER, &worker).unwrap();
    p.cpu.rip = CODE;
    loop {
        match p.run(p.cpu.work + 1).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 20000),
            result => {
                assert_eq!(result, Outcome::Returned(1));
                break;
            }
        }
    }
    assert_eq!(p.memory.read_int(DATA + 3072, 8).unwrap(), 1);
    assert_eq!(p.memory.read_int(DATA + 128, 8).unwrap(), parameter);
    assert_eq!(p.memory.read_int(DATA + 248, 8).unwrap(), WORKER);
    assert_eq!(p.memory.read_int(DATA + 1536 + 224, 8).unwrap(), magic); // r13
    assert_eq!(
        p.memory.read_int(DATA + 1536 + 248, 8).unwrap(),
        WORKER + 10
    );
    assert_eq!(
        p.memory.read_int(DATA + 1536 + 256, 8).unwrap(),
        0xaaaaaaaaaaaaaaaa
    );
    assert_eq!(
        p.memory.read_int(DATA + 1536, 8).unwrap(),
        0xaaaaaaaaaaaaaaaa
    );
    std::fs::remove_file(path).unwrap();
}
#[test]
fn amd64_user_apc_wakes_alertable_target_and_preserves_64_bit_parameter_and_return() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x5a0000000;
    const WORKER: u64 = CODE + 4096;
    const CALLBACK: u64 = WORKER + 256;
    const DATA: u64 = CODE + 8192;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[u64]) {
        for (n, &value) in args.iter().enumerate() {
            mov(code, if n < 4 { [1, 2, 8, 9][n] } else { 0 }, value);
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        mov(code, 0, p.runtime.register("kernel32.dll", name));
        code.extend([0xff, 0xd0]);
    }
    let path = std::env::temp_dir().join(format!("wire-apc64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    p.memory
        .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    let mut main = vec![0x48, 0x83, 0xec, 0x38];
    call(&mut p, &mut main, "SetErrorMode", &[1]);
    call(&mut p, &mut main, "CreateThread", &[0, 0, WORKER, 0, 4, 0]);
    main.extend([0x49, 0x89, 0xc4, 0x4c, 0x89, 0xe1]);
    call(&mut p, &mut main, "ResumeThread", &[]);
    call(&mut p, &mut main, "SwitchToThread", &[]);
    mov(&mut main, 1, CALLBACK);
    main.extend([0x4c, 0x89, 0xe2]); // rdx=r12 target thread
    mov(&mut main, 8, 0x8877665544332211);
    call(&mut p, &mut main, "QueueUserAPC", &[]);
    main.extend([0x4c, 0x89, 0xe1]);
    mov(&mut main, 2, u32::MAX as u64);
    call(&mut p, &mut main, "WaitForSingleObject", &[]);
    call(&mut p, &mut main, "GetThreadErrorMode", &[]);
    main.extend([0x48, 0x83, 0xc4, 0x38, 0xc3]);
    p.memory.write(CODE, &main).unwrap();
    let mut worker = vec![0x48, 0x83, 0xec, 0x28];
    call(&mut p, &mut worker, "GetThreadErrorMode", &[]);
    mov(&mut worker, 7, DATA + 24);
    worker.extend([0x48, 0x89, 0x07]);
    call(&mut p, &mut worker, "SetThreadErrorMode", &[0x8000, 0]);
    call(&mut p, &mut worker, "SleepEx", &[u32::MAX as u64, 1]);
    mov(&mut worker, 7, DATA + 16);
    worker.extend([0x48, 0x89, 0x07, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(WORKER, &worker).unwrap();
    let mut callback = vec![0x48, 0x83, 0xec, 0x28];
    mov(&mut callback, 0, DATA);
    callback.extend([0x48, 0x89, 0x08]);
    call(&mut p, &mut callback, "GetCurrentThreadId", &[]);
    mov(&mut callback, 7, DATA + 8);
    callback.extend([0x48, 0x89, 0x07]);
    call(&mut p, &mut callback, "GetThreadErrorMode", &[]);
    mov(&mut callback, 7, DATA + 32);
    callback.extend([
        0x48, 0x89, 0x07, 0xb8, 99, 0, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0xc3,
    ]);
    p.memory.write(CALLBACK, &callback).unwrap();
    p.cpu.rip = CODE;
    loop {
        match p.run(p.cpu.work + 1).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 1000),
            result => {
                assert_eq!(result, Outcome::Returned(0));
                break;
            }
        }
    }
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0x8877665544332211);
    assert_eq!(p.memory.read_int(DATA + 8, 8).unwrap(), 2);
    assert_eq!(p.memory.read_int(DATA + 16, 8).unwrap(), 0xc0);
    assert_eq!(p.memory.read_int(DATA + 24, 8).unwrap(), 1);
    assert_eq!(p.memory.read_int(DATA + 32, 8).unwrap(), 0x8000);
    std::fs::remove_file(path).unwrap();
}
fn startup_dll_call(
    mem: &mut Memory,
    cpu: &mut Cpu64,
    runtime: &mut wire::x64::runtime::Runtime64,
    dll: &str,
    name: &str,
    args: &[u64],
) -> u64 {
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    for (i, &value) in args.iter().enumerate() {
        if i < 4 {
            cpu.regs[[1, 2, 8, 9][i]] = value;
        } else {
            mem.write_int(cpu.regs[4] + 40 + (i as u64 - 4) * 8, 8, value)
                .unwrap();
        }
    }
    assert_eq!(
        runtime.dispatch(dll, name, cpu, mem).unwrap(),
        wire::x64::runtime::Control::Returned
    );
    assert_eq!(cpu.regs[4], STACK + 0x1f90);
    assert_eq!(cpu.rip, BASE);
    cpu.regs[0]
}
#[test]
fn amd64_window_creation_executes_messages_with_full_createstruct_and_rolls_back_callback_rejection(
) {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x6a0000000;
    const DATA: u64 = CODE + 4096;
    const PARAMETER: u64 = 0x123456789abcdef0;
    let path = std::env::temp_dir().join(format!("wire-window64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for (nccreate, create, expected) in [
        (1u32, 0u32, vec![0x24u64, 0x81, 0x83, 1]),
        (0, 0, vec![0x24, 0x81, 0x82]),
        (1, u32::MAX, vec![0x24, 0x81, 0x83, 1, 2, 0x82]),
    ] {
        let mut p = Process64::load(&path, false).unwrap();
        let entry = p.cpu.clone();
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        p.memory
            .write(
                DATA,
                &"WireProbe\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        p.memory
            .write(
                DATA + 64,
                &"Actual guest window\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let class = DATA + 0x100;
        p.memory.write_int(class, 4, 80).unwrap();
        p.memory.write_int(class + 8, 8, CODE).unwrap();
        p.memory.write_int(class + 20, 4, 16).unwrap();
        p.memory.write_int(class + 24, 8, BASE).unwrap();
        p.memory.write_int(class + 64, 8, DATA).unwrap();
        let atom = startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "RegisterClassExW",
            &[class],
        );
        assert_ne!(atom, 0);
        p.cpu = entry;
        let gateway = p.runtime.register("user32.dll", "CreateWindowExW");
        let mut code = vec![0x48, 0x83, 0xec, 0x68];
        let args = [
            0,
            DATA,
            DATA + 64,
            0x00cf0000,
            10,
            20,
            640,
            480,
            0,
            0,
            BASE,
            PARAMETER,
        ];
        for (i, arg) in args.into_iter().enumerate() {
            if i < 4 {
                let (rex, op) = [(0x48, 0xb9), (0x48, 0xba), (0x49, 0xb8), (0x49, 0xb9)][i];
                code.extend([rex, op]);
                code.extend(arg.to_le_bytes());
            } else {
                code.extend([0x48, 0xb8]);
                code.extend(arg.to_le_bytes());
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (i as u8 - 4) * 8]);
            }
        }
        code.extend([0x48, 0xb8]);
        code.extend(gateway.to_le_bytes());
        code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x68, 0xc3]);
        p.memory.write(BASE + 0x1000, &code).unwrap();
        let mut callback = vec![0x48, 0xb8];
        callback.extend((DATA + 0x400).to_le_bytes());
        callback.extend([0x48, 0xff, 0x00, 0x48, 0x8b, 0x08, 0x89, 0x54, 0xc8, 0]); // record message
        let mut nc = vec![0x48, 0xb8];
        nc.extend((DATA + 0x500).to_le_bytes());
        nc.extend([
            0x4c, 0x89, 0x08, 0x49, 0x8b, 0x09, 0x48, 0x89, 0x48, 8, 0x49, 0x8b, 0x49, 8, 0x48,
            0x89, 0x48, 16,
        ]);
        nc.push(0xb8);
        nc.extend(nccreate.to_le_bytes());
        nc.push(0xc3);
        callback.extend([0x81, 0xfa, 0x81, 0, 0, 0, 0x75, nc.len() as u8]);
        callback.extend(nc);
        callback.extend([0x83, 0xfa, 1, 0x75, 6, 0xb8]);
        callback.extend(create.to_le_bytes());
        callback.push(0xc3);
        callback.extend([0xb8, 1, 0, 0, 0, 0xc3]);
        p.memory.write(CODE, &callback).unwrap();
        let result = loop {
            match p.run(p.cpu.work + 1).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 400),
                Outcome::Returned(value) => break value,
                result => panic!("unexpected {result:?}"),
            }
        };
        let success = nccreate != 0 && create != u32::MAX;
        assert_eq!(result != 0, success);
        assert_eq!(
            p.memory.read_int(DATA + 0x400, 8).unwrap(),
            expected.len() as u64
        );
        for (i, msg) in expected.into_iter().enumerate() {
            assert_eq!(
                p.memory.read_int(DATA + 0x408 + i as u64 * 8, 4).unwrap(),
                msg
            );
        }
        let temporary = p.memory.read_int(DATA + 0x500, 8).unwrap();
        assert!(p.memory.read_int(temporary, 1).is_err());
        assert_eq!(p.memory.read_int(DATA + 0x508, 8).unwrap(), PARAMETER);
        assert_eq!(p.memory.read_int(DATA + 0x510, 8).unwrap(), BASE);
        assert_eq!(p.cpu.regs[4], STACK + 0xfff70);
        if success {
            assert!(result > u32::MAX as u64);
            let info = p.runtime.window_info();
            assert_eq!(info.len(), 1);
            assert_eq!(info[0].handle, result);
            assert_eq!(info[0].title, "Actual guest window");
            assert!(!info[0].visible);
            assert_eq!((info[0].width, info[0].height), (640, 480));
            let event = wire::x64::runtime::WindowEvent {
                window: result,
                message: 0x401,
                wparam: PARAMETER,
                lparam: !PARAMETER,
            };
            p.runtime.queue_window_event(event).unwrap();
            let queued = p.runtime.take_window_events();
            assert_eq!(queued.len(), 1);
            assert_eq!(
                (queued[0].window, queued[0].wparam, queued[0].lparam),
                (result, PARAMETER, !PARAMETER)
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "IsWindow",
                    &[result]
                ),
                1
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "SetWindowLongPtrW",
                    &[result, (-21i32) as u32 as u64, PARAMETER]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "GetWindowLongPtrW",
                    &[result, (-21i32) as u32 as u64]
                ),
                PARAMETER
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "SetWindowLongPtrW",
                    &[result, 0, PARAMETER]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "GetWindowLongPtrW",
                    &[result, 0]
                ),
                PARAMETER
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "GetClientRect",
                    &[result, DATA + 0x200]
                ),
                1
            );
            assert_eq!(p.memory.read_int(DATA + 0x208, 4).unwrap(), 640);
            let dc = startup_dll_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "user32.dll",
                "GetDC",
                &[result],
            );
            assert!(dc > u32::MAX as u64);
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "ReleaseDC",
                    &[result + 16, dc]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "gdi32.dll",
                    "DeleteDC",
                    &[dc]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "gdi32.dll",
                    "GetDeviceCaps",
                    &[dc, 88]
                ),
                96
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "ReleaseDC",
                    &[result, dc]
                ),
                1
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "gdi32.dll",
                    "GetDeviceCaps",
                    &[dc, 88]
                ),
                0
            );
        } else {
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "IsWindow",
                    &[wire::x64::runtime::TEB + 0x630000]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "UnregisterClassW",
                    &[DATA, BASE]
                ),
                1
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_directory_searches_report_real_wide_metadata_and_do_not_skip_entries_after_output_faults()
{
    use wire::x64::process::Process64;
    const DATA: u64 = 0x6b0000000;
    let dir = std::env::temp_dir().join(format!("wire-find64-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("items")).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    std::fs::write(dir.join("items/A.txt"), b"abc").unwrap();
    let huge = std::fs::File::create(dir.join("items/B.txt")).unwrap();
    huge.set_len(0x100000007).unwrap();
    std::fs::write(dir.join("items/あ.txt"), b"x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(std::env::temp_dir(), dir.join("items/escape.txt")).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory.map(DATA, 8192, READ | WRITE).unwrap();
    p.memory
        .write(
            DATA,
            &"C:\\items\\*.TXT\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let output = DATA + 0x200;
    let handle = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "FindFirstFileExW",
        &[DATA, 1, output, 0, 0, 2],
    );
    assert!(handle > u32::MAX as u64 && handle != u64::MAX);
    assert_eq!(p.memory.read_int(output + 32, 4).unwrap(), 3);
    assert_eq!(p.memory.read_int(output + 44, 2).unwrap(), b'A' as u64);
    assert_ne!(p.memory.read_int(output + 20, 8).unwrap(), 0);
    let bad = DATA + 8192 - 100;
    p.memory.write(bad, &[0xcc; 100]).unwrap();
    p.cpu.regs[4] = STACK + 0x1f88;
    p.memory.write_int(p.cpu.regs[4], 8, BASE).unwrap();
    p.cpu.regs[1] = handle;
    p.cpu.regs[2] = bad;
    let before = p.cpu.clone();
    let calls = p.runtime.api_calls;
    assert!(p
        .runtime
        .dispatch("kernel32.dll", "FindNextFileW", &mut p.cpu, &mut p.memory)
        .is_err());
    assert_eq!(p.cpu.regs, before.regs);
    assert_eq!(p.cpu.rip, before.rip);
    assert_eq!(p.runtime.api_calls, calls);
    assert_eq!(p.memory.read_int(bad, 8).unwrap(), 0xcccccccccccccccc);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindNextFileW",
            &[handle, output]
        ),
        1
    );
    assert_eq!(p.memory.read_int(output + 44, 2).unwrap(), b'B' as u64);
    assert_eq!(p.memory.read_int(output + 28, 4).unwrap(), 1);
    assert_eq!(p.memory.read_int(output + 32, 4).unwrap(), 7);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindNextFileW",
            &[handle, output]
        ),
        1
    );
    assert_eq!(p.memory.read_int(output + 44, 2).unwrap(), 0x3042);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindNextFileW",
            &[handle, output]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        18
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindClose",
            &[handle]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindClose",
            &[handle]
        ),
        0
    );
    p.memory.write(DATA, b"C:\\items\\missing*\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindFirstFileA",
            &[DATA, output]
        ),
        u64::MAX
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn amd64_icons_load_real_pe32plus_resources_keep_shared_lifetime_and_fail_missing_ids() {
    use wire::x64::process::Process64;
    let path = std::env::temp_dir().join(format!("wire-icon64-{}.exe", std::process::id()));
    let mut bytes = fixture();
    put32(&mut bytes, 0x98 + 112 + 16, 0x1100);
    put32(&mut bytes, 0x98 + 112 + 20, 0x200);
    // Separate GROUP_ICON and ICON branches, each with a real language/data entry.
    for (directory, entries) in [
        (0, vec![(3, 0x80000030), (14, 0x80000060)]),
        (0x30, vec![(5, 0x80000090)]),
        (0x60, vec![(103, 0x800000c0)]),
        (0x90, vec![(1033, 0xf0)]),
        (0xc0, vec![(1033, 0x100)]),
    ] {
        let base = 0x300 + directory;
        put16(&mut bytes, base + 14, entries.len() as u16);
        for (i, (name, value)) in entries.into_iter().enumerate() {
            put32(&mut bytes, base + 16 + i * 8, name);
            put32(&mut bytes, base + 20 + i * 8, value);
        }
    }
    put32(&mut bytes, 0x3f0, 0x1300);
    put32(&mut bytes, 0x3f4, 64);
    put32(&mut bytes, 0x400, 0x1340);
    put32(&mut bytes, 0x404, 20);
    put32(&mut bytes, 0x500, 40);
    put32(&mut bytes, 0x504, 2);
    put32(&mut bytes, 0x508, 4);
    put16(&mut bytes, 0x50c, 1);
    put16(&mut bytes, 0x50e, 32);
    bytes[0x542] = 1;
    bytes[0x544] = 1;
    bytes[0x546] = 2;
    bytes[0x547] = 2;
    put16(&mut bytes, 0x54a, 1);
    put16(&mut bytes, 0x54c, 32);
    put32(&mut bytes, 0x54e, 64);
    put16(&mut bytes, 0x552, 5);
    std::fs::write(&path, bytes).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    let icon = startup_dll_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "user32.dll",
        "LoadIconA",
        &[BASE, 103],
    );
    assert!(icon > u32::MAX as u64);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "LoadIconW",
            &[BASE, 103]
        ),
        icon
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "DestroyIcon",
            &[icon]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "LoadIconA",
            &[BASE, 103]
        ),
        icon
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "LoadIconA",
            &[BASE, 104]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        1814
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_registry_owns_high_handles_roundtrips_cp932_and_keeps_errors_outputs_and_access_real() {
    let (mut mem, mut cpu, mut runtime) = startup();
    const HKCU: u64 = 0xffffffff80000001;
    let path = STACK + 0x200;
    let value = STACK + 0x300;
    let output = STACK + 0x500;
    let data = STACK + 0x600;
    macro_rules! call {
        ($name:expr, $args:expr) => {
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "advapi32.dll",
                $name,
                $args,
            )
        };
    }
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetLastError",
        &[0xabcdef],
    );
    mem.write(
        path,
        &"Software\\Wire\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    mem.write(output, &[0xcc; 32]).unwrap();
    assert_eq!(
        call!(
            "RegCreateKeyExW",
            &[HKCU, path, 0, 0, 0, 0xf003f, 0, output, output + 8]
        ),
        0
    );
    let handle = mem.read_int(output, 8).unwrap();
    assert!(handle > u32::MAX as u64);
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 1);
    assert_eq!(
        call!(
            "RegCreateKeyExW",
            &[HKCU, path, 0, 0, 0, 0x20019, 0, output + 16, output + 8]
        ),
        0
    );
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 2);
    let readonly = mem.read_int(output + 16, 8).unwrap();
    mem.write(value, b"Text\0").unwrap();
    let cp932 = [0x82, 0xa0, 0, 0x41, 0, 0]; // MULTI_SZ preserves embedded NULs.
    mem.write(data, &cp932).unwrap();
    assert_eq!(
        call!(
            "RegSetValueExA",
            &[handle, value, 0, 7, data, cp932.len() as u64]
        ),
        0
    );
    assert_eq!(
        call!(
            "RegSetValueExA",
            &[readonly, value, 0, 7, data, cp932.len() as u64]
        ),
        5
    );
    mem.write(
        value,
        &"tExT\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    mem.write_int(output + 8, 4, 0).unwrap();
    assert_eq!(
        call!(
            "RegQueryValueExW",
            &[handle, value, 0, output + 12, 0, output + 8]
        ),
        0
    );
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 10);
    assert_eq!(mem.read_int(output + 12, 4).unwrap(), 7);
    mem.write(data, &[0xaa; 16]).unwrap();
    mem.write_int(output + 8, 4, 2).unwrap();
    assert_eq!(
        call!(
            "RegQueryValueExW",
            &[readonly, value, 0, output + 12, data, output + 8]
        ),
        234
    );
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 10);
    assert_eq!(mem.read_int(data, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
    assert_eq!(
        call!(
            "RegQueryValueExW",
            &[readonly, value, 0, output + 12, data, output + 8]
        ),
        0
    );
    let mut wide = [0; 10];
    mem.read(data, &mut wide).unwrap();
    assert_eq!(wide, [0x42, 0x30, 0, 0, 0x41, 0, 0, 0, 0, 0]);
    mem.write(value, b"TEXT\0").unwrap();
    mem.write_int(output + 8, 4, 16).unwrap();
    assert_eq!(
        call!("RegQueryValueExA", &[handle, value, 0, 0, data, output + 8]),
        0
    );
    let mut ansi = [0; 6];
    mem.read(data, &mut ansi).unwrap();
    assert_eq!(ansi, cp932);
    assert_eq!(call!("RegCloseKey", &[handle]), 0);
    assert_eq!(call!("RegCloseKey", &[handle]), 6);
    mem.write(path, b"software\\WIRE\0").unwrap();
    assert_eq!(call!("RegOpenKeyExA", &[HKCU, path, 0, 3, output]), 0);
    let reopened = mem.read_int(output, 8).unwrap();
    assert_ne!(reopened, handle);
    assert_eq!(
        call!(
            "RegQueryValueExA",
            &[reopened, value, 0, 0, data, output + 8]
        ),
        0
    );
    // An unknown system installation is absent, not fabricated from a successful open.
    assert_eq!(
        call!(
            "RegOpenKeyExA",
            &[0xffffffff80000002, path, 0, 0x20019, output]
        ),
        2
    );
    assert_eq!(
        call!("RegCreateKeyA", &[0xffffffff80000002, path, output]),
        5
    );
    assert_eq!(
        call!("RegOpenKeyExA", &[HKCU, path, 0, 0x20219, output]),
        50
    );
    assert_eq!(call!("RegDeleteValueA", &[reopened, value]), 0);
    assert_eq!(
        call!(
            "RegQueryValueExA",
            &[reopened, value, 0, 0, data, output + 8]
        ),
        2
    );
    assert_eq!(call!("RegDeleteValueA", &[reopened, value]), 2);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0xabcdef
    );
    // Bad disposition output cannot create the key or write the handle output.
    mem.write(path, b"Software\\NotCreated\0").unwrap();
    mem.write_int(output, 8, 0xdeadbeefcafebabe).unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    let args = [HKCU, path, 0, 0, 0, 3, 0, output, STACK + 8192 - 1];
    for (i, arg) in args.into_iter().enumerate() {
        if i < 4 {
            cpu.regs[[1, 2, 8, 9][i]] = arg;
        } else {
            mem.write_int(cpu.regs[4] + 40 + (i as u64 - 4) * 8, 8, arg)
                .unwrap();
        }
    }
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("advapi32.dll", "RegCreateKeyExA", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xdeadbeefcafebabe);
    assert_eq!(call!("RegOpenKeyExA", &[HKCU, path, 0, 1, output]), 2);
}

#[test]
fn win64_display_queries_agree_on_one_monitor_and_check_ansi_wide_layouts_and_packed_points() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x400;
    let device = STACK + 0x200;
    for (suffix, size, flags, mode_size, size_offset, bits) in [
        ('A', 424u32, 164u64, 156u64, 36u64, 104u64),
        ('W', 840, 324, 220, 68, 168),
    ] {
        mem.write(output, &[0xcc; 848]).unwrap();
        mem.write_int(output, 4, size as u64).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[0, 0, output, 0]
            ),
            1
        );
        assert_eq!(mem.read_int(output + flags, 4).unwrap(), 5);
        assert_eq!(
            mem.read_int(output + size as u64, 8).unwrap(),
            0xcccccccccccccccc
        );
        let mut before = vec![0; size as usize];
        mem.read(output, &mut before).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[0, 1, output, 0]
            ),
            0
        );
        let mut after = vec![0; size as usize];
        mem.read(output, &mut after).unwrap();
        assert_eq!(after, before);
        let text: Vec<u8> = if suffix == 'W' {
            "\\\\.\\DISPLAY1\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            b"\\\\.\\DISPLAY1\0".to_vec()
        };
        mem.write(device, &text).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[device, 0, output, 0]
            ),
            1
        );
        assert_eq!(mem.read_int(output + flags, 4).unwrap(), 3);
        for mode in [0, u32::MAX as u64, u32::MAX as u64 - 1] {
            mem.write(output, &[0xdd; 224]).unwrap();
            mem.write_int(output + size_offset, 2, mode_size).unwrap();
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    "user32.dll",
                    &format!("EnumDisplaySettings{suffix}"),
                    &[device, 0x1234000000000000 | mode, output]
                ),
                1
            );
            for (i, value) in [32, 1024, 768, 0, 60].into_iter().enumerate() {
                assert_eq!(
                    mem.read_int(output + bits + i as u64 * 4, 4).unwrap(),
                    value
                );
            }
            assert_eq!(mem.read_int(output + mode_size, 4).unwrap(), 0xdddddddd);
        }
    }
    let point = (100u64 << 32) | 100;
    let monitor = startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "user32.dll",
        "MonitorFromPoint",
        &[point, 0],
    );
    assert!(monitor > u32::MAX as u64);
    for point in [0x64ffffffff, 0xffffffff00000064, (768u64 << 32) | 1024] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                "MonitorFromPoint",
                &[point, 0]
            ),
            0
        );
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                "MonitorFromPoint",
                &[point, 2]
            ),
            monitor
        );
    }
    for (suffix, size) in [('A', 40), ('A', 72), ('W', 104)] {
        mem.write_int(output, 4, size).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                &format!("GetMonitorInfo{suffix}"),
                &[monitor, output]
            ),
            1
        );
        assert_eq!(mem.read_int(output + 12, 4).unwrap(), 1024);
        assert_eq!(mem.read_int(output + 16, 4).unwrap(), 768);
        assert_eq!(mem.read_int(output + 36, 4).unwrap(), 1);
    }
    // A valid cb field followed by an unmapped tail cannot partially write a device.
    let bad = STACK + 8192 - 8;
    mem.write_int(bad, 8, 0xcccccccc000001a8).unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = 0;
    cpu.regs[2] = 0;
    cpu.regs[8] = bad;
    cpu.regs[9] = 0;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("user32.dll", "EnumDisplayDevicesA", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(bad, 8).unwrap(), 0xcccccccc000001a8);
}

#[test]
fn amd64_monitor_enumeration_calls_real_guest_code_preserves_high_lparam_and_returns_callback_bool()
{
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x690000000;
    const DATA: u64 = CODE + 4096;
    let path = std::env::temp_dir().join(format!("wire-monitor64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for keep_going in [false, true] {
        let mut p = Process64::load(&path, false).unwrap();
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        let gateway = p.runtime.register("user32.dll", "EnumDisplayMonitors");
        let parameter = 0x123456789abcdef0u64;
        let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x31, 0xc9, 0x48, 0xba];
        code.extend(DATA.to_le_bytes());
        code.extend([0x49, 0xb8]);
        code.extend(CODE.to_le_bytes());
        code.extend([0x49, 0xb9]);
        code.extend(parameter.to_le_bytes());
        code.extend([0x48, 0xb8]);
        code.extend(gateway.to_le_bytes());
        code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(BASE + 0x1000, &code).unwrap();
        let mut callback = vec![0x48, 0xb8];
        callback.extend((DATA + 32).to_le_bytes());
        callback.extend([
            0x48, 0x89, 0x08, 0x48, 0x89, 0x50, 8, 0x4c, 0x89, 0x40, 16, 0x4c, 0x89, 0x48, 24,
        ]);
        for i in 0..4u8 {
            callback.extend([0x41, 0x8b, 0x48, 4 * i, 0x89, 0x48, 32 + 4 * i]);
        }
        callback.extend([0xb8, keep_going as u8, 0, 0, 0, 0xc3]);
        p.memory.write(CODE, &callback).unwrap();
        p.memory
            .write(
                DATA,
                &[10i32, 20, 100, 200]
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        loop {
            match p.run(p.cpu.work + 1).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 100),
                result => {
                    assert_eq!(result, Outcome::Returned(keep_going as u64));
                    break;
                }
            }
        }
        assert!(p.memory.read_int(DATA + 32, 8).unwrap() > u32::MAX as u64);
        assert_eq!(p.memory.read_int(DATA + 40, 8).unwrap(), 0);
        assert_eq!(p.memory.read_int(DATA + 56, 8).unwrap(), parameter);
        let block = p.memory.read_int(DATA + 48, 8).unwrap();
        assert!(p.memory.read_int(block, 1).is_err());
        for (i, value) in [10, 20, 100, 200].into_iter().enumerate() {
            assert_eq!(
                p.memory.read_int(DATA + 64 + i as u64 * 4, 4).unwrap(),
                value
            );
        }
        assert_eq!(p.cpu.regs[4], STACK + 0xfff70);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_sspi_tables_are_stable_guest_pointers_and_never_advertise_unavailable_security() {
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.install_gateways(Default::default());
    let text = STACK + 0x200;
    mem.write(text, b"secur32.dll\0").unwrap();
    let module = startup_call(&mut mem, &mut cpu, &mut runtime, "LoadLibraryA", &[text]);
    assert_ne!(module, 0);
    let mut tables = Vec::new();
    for suffix in ['A', 'W'] {
        let name = format!("InitSecurityInterface{suffix}");
        mem.write(text, format!("{name}\0").as_bytes()).unwrap();
        let gateway = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[module, text],
        );
        assert!(gateway > u32::MAX as u64);
        assert_eq!(runtime.gateways[&gateway], format!("secur32.dll!{name}"));
        let table = startup_dll_call(&mut mem, &mut cpu, &mut runtime, "secur32.dll", &name, &[]);
        assert!(table > u32::MAX as u64);
        assert_eq!(mem.read_int(table, 4).unwrap(), 1);
        assert_eq!(mem.read_int(table + 4, 4).unwrap(), 0);
        assert!(mem.write_int(table, 4, 99).is_err());
        for offset in [40, 144, 152, 184] {
            assert_eq!(mem.read_int(table + offset, 8).unwrap(), 0);
        }
        for (offset, function) in [
            (8, format!("EnumerateSecurityPackages{suffix}")),
            (24, format!("AcquireCredentialsHandle{suffix}")),
            (48, format!("InitializeSecurityContext{suffix}")),
            (128, "FreeContextBuffer".into()),
            (136, format!("QuerySecurityPackageInfo{suffix}")),
            (200, "EncryptMessage".into()),
            (208, "DecryptMessage".into()),
            (216, format!("SetContextAttributes{suffix}")),
        ] {
            let pointer = mem.read_int(table + offset, 8).unwrap();
            assert!(pointer > u32::MAX as u64);
            assert_eq!(
                runtime.gateways[&pointer],
                format!("secur32.dll!{function}")
            );
        }
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "secur32.dll",
                "FreeContextBuffer",
                &[table]
            ),
            0x80090301
        );
        let heap = startup_call(&mut mem, &mut cpu, &mut runtime, "GetProcessHeap", &[]);
        cpu.regs[4] = STACK + 0x1f88;
        mem.write_int(cpu.regs[4], 8, BASE).unwrap();
        cpu.regs[1] = heap;
        cpu.regs[2] = 0;
        cpu.regs[8] = table;
        assert!(runtime
            .dispatch("kernel32.dll", "HeapFree", &mut cpu, &mut mem)
            .is_err());
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut runtime, "secur32.dll", &name, &[]),
            table
        );
        tables.push(table);
    }
    assert_ne!(tables[0], tables[1]);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetLastError",
        &[0x87654321],
    );
    let output = STACK + 0x400;
    mem.write(output, &[0xaa; 32]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "secur32.dll",
            "EnumerateSecurityPackagesW",
            &[output, output + 8]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xaaaaaaaa00000000);
    assert_eq!(mem.read_int(output + 8, 8).unwrap(), 0);
    mem.write(text, b"Schannel\0").unwrap();
    mem.write(output, &[0xbb; 32]).unwrap();
    for (name, args) in [
        ("QuerySecurityPackageInfoA", vec![text, output]),
        (
            "AcquireCredentialsHandleA",
            vec![0, text, 2, 0, 0, 0, 0, output, output + 16],
        ),
        (
            "InitializeSecurityContextA",
            vec![
                output,
                0,
                text,
                0,
                0,
                0,
                0,
                0,
                output,
                output,
                output + 16,
                output + 24,
            ],
        ),
        ("EncryptMessage", vec![output, 0, output, 0]),
        ("DecryptMessage", vec![output, output, 0, output + 24]),
        ("FreeContextBuffer", vec![output]),
    ] {
        let expected = if name.starts_with("QuerySecurityPackage") || name.starts_with("Acquire") {
            0x80090305
        } else {
            0x80090301
        };
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut runtime, "secur32.dll", name, &args),
            expected
        );
        let mut bytes = [0; 32];
        mem.read(output, &mut bytes).unwrap();
        assert_eq!(bytes, [0xbb; 32]);
    }
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0x87654321
    );
    // A bad second output must not zero the first output or commit the call.
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = output;
    cpu.regs[2] = STACK + 0x1ffd;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch(
            "secur32.dll",
            "EnumerateSecurityPackagesA",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(output, 4).unwrap(), 0xbbbbbbbb);
}

#[test]
fn amd64_guest_calls_the_sspi_function_table_across_single_instruction_budgets() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x680000000;
    const DATA: u64 = CODE + 4096;
    let dir = std::env::temp_dir().join(format!("wire-sspi64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    p.memory
        .write(DATA, b"C:\\Windows\\System32\\secur32.dll\0")
        .unwrap();
    let module = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "LoadLibraryExA",
        &[DATA, 0, 8],
    );
    assert_ne!(module, 0);
    assert!(!dir.join("Windows/System32/secur32.dll").exists());
    p.memory.write(DATA, b"InitSecurityInterfaceW\0").unwrap();
    let init = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "GetProcAddress",
        &[module, DATA],
    );
    // Restore the actual process return sentinel, replaced by the direct-call helper.
    p.memory
        .write_int(p.cpu.regs[4], 8, 0x7ffe10000000)
        .unwrap();
    let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x48, 0xb8];
    code.extend(init.to_le_bytes());
    code.extend([0xff, 0xd0, 0x48, 0x89, 0xc3, 0x48, 0xb9]); // RBX=table, RCX=count
    code.extend((DATA + 0x100).to_le_bytes());
    code.extend([0x48, 0xba]);
    code.extend((DATA + 0x108).to_le_bytes());
    code.extend([0xff, 0x53, 0x08, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(DATA + 0x100, &[0xcc; 16]).unwrap();
    p.memory.write(CODE, &code).unwrap();
    p.cpu.rip = CODE;
    loop {
        match p.run(p.cpu.work + 1).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 100),
            result => {
                assert_eq!(result, Outcome::Returned(0));
                break;
            }
        }
    }
    assert_eq!(p.memory.read_int(DATA + 0x100, 4).unwrap(), 0);
    assert_eq!(p.memory.read_int(DATA + 0x104, 4).unwrap(), 0xcccccccc);
    assert_eq!(p.memory.read_int(DATA + 0x108, 8).unwrap(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn win64_winsock_negotiates_versions_checks_full_output_and_balances_ordinal_startup_refs() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x200;
    let mut call = |name: &str, args: &[u64]| {
        startup_dll_call(&mut mem, &mut cpu, &mut runtime, "Ws2_32.dll", name, args)
    };
    call("#112", &[0x1234567887654321]);
    assert_eq!(call("WSAGetLastError", &[]), 0x87654321);
    assert_eq!(call("#115", &[0x0202, 0]), 10014);
    assert_eq!(call("WSAStartup", &[0, 0]), 10092);
    assert_eq!(call("#111", &[]), 0x87654321);
    assert_eq!(call("#116", &[]), u32::MAX as u64);
    assert_eq!(call("#111", &[]), 10093);

    // Every successful startup owns a process reference, independent of version.
    for (request, negotiated) in [
        (0x0101u16, 0x0101u16),
        (0x0001, 0x0001),
        (0x0301, 0x0101),
        (0x0202, 0x0202),
        (0x0102, 0x0102),
        (0x0302, 0x0202),
        (0x0003, 0x0202),
    ] {
        mem.write(output, &[0xa5; 416]).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ws2_32.dll",
                "#115",
                &[0x1234000000000000 | request as u64, output]
            ),
            0
        );
        assert_eq!(mem.read_int(output, 2).unwrap(), negotiated as u64);
        assert_eq!(mem.read_int(output + 2, 2).unwrap(), 0x0202);
        assert_eq!(
            mem.read_int(output + 4, 2).unwrap(),
            if request as u8 == 1 { 32767 } else { 0 }
        );
        assert_eq!(
            mem.read_int(output + 6, 2).unwrap(),
            if request as u8 == 1 { 65467 } else { 0 }
        );
        assert_eq!(mem.read_int(output + 8, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
        let mut status = [0; 8];
        mem.read(output + 273, &mut status).unwrap();
        assert_eq!(&status, b"Running\0");
        let mut tail = [0; 14];
        mem.read(output + 402, &mut tail).unwrap();
        assert_eq!(tail, [0xa5; 14]);
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ws2_32.dll",
            "WSAGetLastError",
            &[]
        ),
        10093
    );

    // A checked fault cannot overwrite an initial field or increment references.
    let bad = STACK + 4096 - 200;
    mem.write(bad, &[0xcc; 200]).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.regs[4] = STACK + 0x188;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = 0x0202;
    cpu.regs[2] = bad;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("ws2_32.dll", "#115", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    let mut preserved = [0; 200];
    mem.read(bad, &mut preserved).unwrap();
    assert_eq!(preserved, [0xcc; 200]);
    mem.protect(STACK + 4096, 4096, READ | WRITE).unwrap();
    // Invalid version writes negotiation fields, but owns no reference.
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ws2_32.dll",
            "WSAStartup",
            &[0x0200, output]
        ),
        10092
    );
    assert_eq!(mem.read_int(output, 2).unwrap(), 0x0202);
    for _ in 0..7 {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ws2_32.dll",
                "WSACleanup",
                &[]
            ),
            0
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ws2_32.dll",
            "WSACleanup",
            &[]
        ),
        u32::MAX as u64
    );

    runtime.register_provider("ws2_32.dll");
    mem.write(output, b"ws2_32.dll\0").unwrap();
    let module = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleHandleA",
        &[output],
    );
    mem.write(output, b"WSAStartup\0").unwrap();
    let named = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetProcAddress",
        &[module, output],
    );
    let ordinal = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetProcAddress",
        &[module, 115],
    );
    assert_eq!(named, ordinal);
    assert_eq!(ordinal, runtime.register("ws2_32.dll", "#115"));
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[module, 65535]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        127
    );
}
#[test]
fn win64_address_waits_check_size_values_timeouts_and_faults_before_blocking() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let address = STACK + 0x300;
    let compare = address + 16;
    runtime.enable_scheduler();
    mem.write_int(address, 8, 0xff00000000000000).unwrap();
    for size in [1, 2, 4, 8] {
        let result = startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "api-ms-win-core-synch-l1-2-0.dll",
            "WaitOnAddress",
            &[address, compare, size, 0],
        );
        assert_eq!(result, if size == 8 { 1 } else { 0 });
        if size != 8 {
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
                1460
            );
        }
    }
    for size in [0, 3, 16, 0x100000008] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitOnAddress",
                &[0, 0, size, 0]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            87
        );
    }
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = address;
    cpu.regs[2] = 0x123400000000;
    cpu.regs[8] = 8;
    cpu.regs[9] = 0;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "WaitOnAddress", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    // No readers are required for a wake with no matching waiter.
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "WakeByAddressAll",
        &[0x123400000000],
    );
}
#[test]
fn amd64_address_wait_queues_wake_first_arrival_and_all_across_single_step_budgets() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x580000000;
    const SECOND: u64 = CODE + 4096;
    const FIRST: u64 = CODE + 8192;
    const DATA: u64 = CODE + 12288;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn store(code: &mut Vec<u8>, pointer: u64) {
        mov(code, 7, pointer);
        code.extend([0x48, 0x89, 0x07]);
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[u64]) {
        for (n, &value) in args.iter().enumerate() {
            mov(code, if n < 4 { [1, 2, 8, 9][n] } else { 0 }, value);
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        let dll = if name == "WaitOnAddress" || name.starts_with("WakeByAddress") {
            "api-ms-win-core-synch-l1-2-0.dll"
        } else if name.starts_with("WSA") {
            "ws2_32.dll"
        } else {
            "kernel32.dll"
        };
        mov(code, 0, p.runtime.register(dll, name));
        code.extend([0xff, 0xd0]);
    }
    let path = std::env::temp_dir().join(format!("wire-address64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    p.memory
        .map(CODE, 4 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    // Thread 3 waits before thread 2, so a single wake must use arrival order.
    let mut main = vec![0x48, 0x83, 0xec, 0x38];
    call(&mut p, &mut main, "WakeByAddressSingle", &[DATA]); // No wake credit.
    call(&mut p, &mut main, "CreateThread", &[0, 0, SECOND, 0, 4, 0]);
    store(&mut main, DATA + 64);
    call(&mut p, &mut main, "CreateThread", &[0, 0, FIRST, 0, 4, 0]);
    store(&mut main, DATA + 72);
    mov(&mut main, 0, DATA + 72);
    main.extend([0x48, 0x8b, 0x08]);
    call(&mut p, &mut main, "ResumeThread", &[]);
    call(&mut p, &mut main, "SwitchToThread", &[]);
    mov(&mut main, 0, DATA + 64);
    main.extend([0x48, 0x8b, 0x08]);
    call(&mut p, &mut main, "ResumeThread", &[]);
    call(&mut p, &mut main, "SwitchToThread", &[]);
    call(&mut p, &mut main, "WakeByAddressSingle", &[DATA + 128]); // Wrong address.
    call(&mut p, &mut main, "WakeByAddressSingle", &[DATA]);
    call(&mut p, &mut main, "SwitchToThread", &[]);
    mov(&mut main, 0, DATA + 16);
    main.extend([0x48, 0x8b, 0x00]);
    store(&mut main, DATA + 80);
    mov(&mut main, 0, DATA + 24);
    main.extend([0x48, 0x8b, 0x00]);
    store(&mut main, DATA + 88);
    call(&mut p, &mut main, "WakeByAddressAll", &[DATA]);
    mov(&mut main, 0, DATA + 64);
    main.extend([0x48, 0x8b, 0x08]);
    mov(&mut main, 2, u32::MAX as u64);
    call(&mut p, &mut main, "WaitForSingleObject", &[]);
    // A finite address wait must expire even if the earlier wake had no waiter.
    call(
        &mut p,
        &mut main,
        "WaitOnAddress",
        &[DATA + 128, DATA + 8, 8, 1],
    );
    store(&mut main, DATA + 96);
    call(&mut p, &mut main, "GetLastError", &[]);
    store(&mut main, DATA + 104);
    main.extend([0xb8, 1, 0, 0, 0, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    p.memory.write(CODE, &main).unwrap();
    for (entry, result, last_error, error) in [
        (FIRST, DATA + 16, DATA + 32, 123u64),
        (SECOND, DATA + 24, DATA + 40, 456),
    ] {
        let mut worker = vec![0x48, 0x83, 0xec, 0x28];
        call(&mut p, &mut worker, "WSASetLastError", &[error]);
        call(
            &mut p,
            &mut worker,
            "WaitOnAddress",
            &[DATA, DATA + 8, 8, u32::MAX as u64],
        );
        store(&mut worker, result);
        call(&mut p, &mut worker, "WSAGetLastError", &[]);
        store(&mut worker, last_error);
        worker.extend([0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(entry, &worker).unwrap();
    }
    p.cpu.rip = CODE;
    loop {
        match p.run(p.cpu.work + 1).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 10000),
            result => {
                assert_eq!(result, Outcome::Returned(1));
                break;
            }
        }
    }
    for (offset, expected) in [
        (16, 1),
        (24, 1),
        (32, 123),
        (40, 456),
        (80, 1),
        (88, 0),
        (96, 0),
        (104, 1460),
    ] {
        assert_eq!(
            p.memory.read_int(DATA + offset, 8).unwrap(),
            expected,
            "offset={offset}"
        );
    }
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0); // Wake can return without value change.
    std::fs::remove_file(path).unwrap();
}
#[test]
fn amd64_dll_directory_changes_real_module_search_preserves_faults_and_confines_paths() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x590000000;
    const DATA: u64 = CODE + 4096;
    const CHILD: u64 = 0x1a0000000;
    fn mov(code: &mut Vec<u8>, r: u8, value: u64) {
        code.extend([0x48 | (r >> 3), 0xb8 | (r & 7)]);
        code.extend(value.to_le_bytes());
    }
    let dir = std::env::temp_dir().join(format!("wire-dll-dir64-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("plugins")).unwrap();
    std::fs::create_dir_all(dir.join("cwd")).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    let mut child = fixture();
    put16(&mut child, 0x96, 0x2002);
    put64(&mut child, 0x98 + 24, CHILD);
    child[0x200..0x206].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc3]);
    std::fs::write(dir.join("plugins/choice.dll"), &child).unwrap();
    std::fs::write(dir.join("cwd/cwdonly.dll"), &child).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    let original = p.cpu.clone();
    p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    p.memory.write(DATA, b"C:\\cwd\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetCurrentDirectoryA",
            &[DATA]
        ),
        1
    );
    p.memory.write(DATA, b"C:\\plugins\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetDllDirectoryA",
            &[DATA]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryW",
            &[0, 0]
        ),
        11
    );
    p.memory.write(DATA + 128, &[0xcc; 32]).unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryW",
            &[10, DATA + 128]
        ),
        11
    );
    assert_eq!(
        p.memory.read_int(DATA + 128, 8).unwrap(),
        0xcccccccccccccccc
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryW",
            &[11, DATA + 128]
        ),
        10
    );
    let mut wide = [0; 22];
    p.memory.read(DATA + 128, &mut wide).unwrap();
    let expected: Vec<u8> = "C:\\plugins\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(&wide[..], &expected);
    p.memory.write(DATA, b"..\\..\\outside\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetDllDirectoryA",
            &[DATA]
        ),
        0
    );
    p.cpu.regs[4] = STACK + 0x1f88;
    p.memory.write_int(p.cpu.regs[4], 8, BASE).unwrap();
    p.cpu.regs[1] = CODE + 8192;
    let before = p.cpu.clone();
    let calls = p.runtime.api_calls;
    assert!(p
        .runtime
        .dispatch(
            "kernel32.dll",
            "SetDllDirectoryW",
            &mut p.cpu,
            &mut p.memory
        )
        .is_err());
    assert_eq!(p.cpu, before);
    assert_eq!(p.runtime.api_calls, calls);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryA",
            &[32, DATA + 128]
        ),
        10
    );

    p.memory.write(DATA, b"choice\0").unwrap(); // Actual load adds the .dll extension.
    let load = p.runtime.register("kernel32.dll", "LoadLibraryA");
    let mut code = vec![0x48, 0x83, 0xec, 0x28];
    mov(&mut code, 1, DATA);
    mov(&mut code, 0, load);
    code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(CODE, &code).unwrap();
    p.cpu = original;
    p.cpu.rip = CODE;
    assert_eq!(p.run(1000).unwrap(), Outcome::Returned(CHILD));
    assert!(p
        .modules
        .iter()
        .any(|(name, base)| name == "choice.dll" && *base == CHILD));
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "psapi.dll",
            "GetModuleFileNameExA",
            &[u64::MAX, CHILD, DATA + 128, 64]
        ),
        21
    );
    let mut loaded_path = [0; 22];
    p.memory.read(DATA + 128, &mut loaded_path).unwrap();
    assert_eq!(&loaded_path, b"C:\\plugins\\choice.dll\0");
    // NULL restores CWD search; an empty string removes CWD search.
    p.memory.write(DATA, &[0]).unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetDllDirectoryA",
            &[DATA]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryA",
            &[0, 0]
        ),
        0
    );
    p.memory.write(DATA + 256, b"cwdonly.dll\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "LoadLibraryA",
            &[DATA + 256]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetDllDirectoryW",
            &[0]
        ),
        1
    );
    p.memory.write(DATA, b"cwdonly.dll\0").unwrap();
    p.cpu.regs[4] = STACK + 0x1f88;
    p.memory.write_int(p.cpu.regs[4], 8, BASE).unwrap();
    p.cpu.regs[1] = DATA;
    match p
        .runtime
        .dispatch("kernel32.dll", "LoadLibraryA", &mut p.cpu, &mut p.memory)
        .unwrap()
    {
        wire::x64::runtime::Control::LoadModule { path, .. } => {
            assert_eq!(path, "C:\\cwd\\cwdonly.dll")
        }
        other => panic!("CWD search was not restored: {other:?}"),
    }
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn amd64_altered_and_system_search_load_real_dependency_code_without_changing_process_directory() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x5a0000000;
    const DATA: u64 = CODE + 4096;
    const PARENT: u64 = 0x1a0000000;
    const DEP: u64 = 0x1b0000000;
    fn dependency(value: u32) -> Vec<u8> {
        let mut b = fixture();
        put16(&mut b, 0x96, 0x2002);
        put64(&mut b, 0x98 + 24, DEP);
        b[0x200..0x206].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc3]);
        put32(&mut b, 0x98 + 112, 0x1100);
        put32(&mut b, 0x98 + 116, 64);
        for (offset, word) in [
            (16, 1),
            (20, 1),
            (24, 1),
            (28, 0x1184),
            (32, 0x1188),
            (36, 0x118c),
        ] {
            put32(&mut b, 0x300 + offset, word);
        }
        put32(&mut b, 0x384, 0x1200);
        put32(&mut b, 0x388, 0x11b0);
        b[0x3b0..0x3b6].copy_from_slice(b"Value\0");
        b[0x400] = 0xb8;
        put32(&mut b, 0x401, value);
        b[0x405] = 0xc3;
        b
    }
    let dir = std::env::temp_dir().join(format!("wire-altered64-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("plugins")).unwrap();
    std::fs::create_dir_all(dir.join("Windows/System32")).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    for (path, value) in [
        ("dep.dll", 11),
        ("plugins/dep.dll", 22),
        ("Windows/System32/dep.dll", 33),
    ] {
        std::fs::write(dir.join(path), dependency(value)).unwrap();
    }
    let mut parent = fixture();
    put16(&mut parent, 0x96, 0x2002);
    put64(&mut parent, 0x98 + 24, PARENT);
    put32(&mut parent, 0x98 + 120, 0x1100);
    put32(&mut parent, 0x98 + 124, 40);
    put32(&mut parent, 0x300, 0x1180);
    put32(&mut parent, 0x30c, 0x11c0);
    put32(&mut parent, 0x310, 0x1190);
    put64(&mut parent, 0x380, 0x11d0);
    parent[0x3c0..0x3c8].copy_from_slice(b"dep.dll\0");
    parent[0x3d2..0x3d8].copy_from_slice(b"Value\0");
    // Parent DllMain calls the actual dependency, retaining its result in .text data.
    parent[0x200..0x21b].copy_from_slice(&[
        0x48, 0x83, 0xec, 0x28, 0xff, 0x15, 0x86, 1, 0, 0, 0x48, 0x89, 0x05, 0xef, 2, 0, 0, 0xb8,
        1, 0, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0xc3,
    ]);
    std::fs::write(dir.join("plugins/parent.dll"), parent).unwrap();
    for (flags, expected) in [(0, 11), (8, 22), (0x800, 33)] {
        let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        p.memory.write(DATA, b"C:\\plugins\\parent.dll\0").unwrap();
        let gateway = p.runtime.register("kernel32.dll", "LoadLibraryExA");
        let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x48, 0xb9];
        code.extend(DATA.to_le_bytes());
        code.extend([0x31, 0xd2, 0x41, 0xb8]);
        code.extend((flags as u32).to_le_bytes());
        code.extend([0x48, 0xb8]);
        code.extend(gateway.to_le_bytes());
        code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(CODE, &code).unwrap();
        p.cpu.rip = CODE;
        loop {
            match p.run(p.cpu.work + 1).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 500),
                result => {
                    assert_eq!(result, Outcome::Returned(PARENT));
                    break;
                }
            }
        }
        assert_eq!(p.memory.read_int(PARENT + 0x1300, 8).unwrap(), expected);
        assert_eq!(p.memory.read_int(PARENT + 0x1190, 8).unwrap(), DEP + 0x1200);
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "GetDllDirectoryA",
                &[0, 0]
            ),
            0
        );
        p.memory.write(DATA, b"parent.dll\0").unwrap();
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "LoadLibraryExA",
                &[DATA, 0, 8]
            ),
            0
        );
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "GetLastError",
                &[]
            ),
            87
        );
        p.memory.write(DATA, b"C:\\plugins\\parent.dll\0").unwrap();
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "LoadLibraryExA",
                &[DATA, 0, 0x808]
            ),
            0
        );
    }
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory.map(DATA, 4096, READ | WRITE).unwrap();
    p.memory
        .write(DATA, b"C:\\Windows\\System32\\ntdll.dll\0")
        .unwrap();
    let handle = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "LoadLibraryExA",
        &[DATA, 0, 8],
    );
    assert_ne!(handle, 0);
    assert!(!dir.join("Windows/System32/ntdll.dll").exists());
    p.memory.write(DATA, b"C:\\plugins\\ntdll.dll\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "LoadLibraryExA",
            &[DATA, 0, 8]
        ),
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn win64_version_profile_queries_verify_hierarchically_and_keep_ntstatus_and_output_faults() {
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.install_gateways(Default::default());
    let output = STACK + 0x200;
    for (dll, name, size) in [
        ("ntdll.dll", "RtlGetVersion", 284),
        ("kernel32.dll", "GetVersionExW", 276),
        ("kernel32.dll", "GetVersionExA", 156),
    ] {
        mem.write(output, &[0xcc; 284]).unwrap();
        mem.write_int(output, 4, size).unwrap();
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, name, &[output]),
            (dll == "kernel32.dll") as u64
        );
        assert_eq!(mem.read_int(output + 4, 4).unwrap(), 10);
        assert_eq!(mem.read_int(output + 8, 4).unwrap(), 0);
        assert_eq!(mem.read_int(output + 12, 4).unwrap(), 19045);
        if size != 276 {
            assert_eq!(mem.read_int(output + size - 4, 2).unwrap(), 0x100);
            assert_eq!(mem.read_int(output + size - 2, 1).unwrap(), 1);
        }
    }
    mem.write_int(output, 4, 284).unwrap();
    startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "ntdll.dll",
        "RtlGetVersion",
        &[output],
    );
    let major = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "VerSetConditionMask",
        &[0x8000000000000000, 2, 3],
    );
    let both = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "VerSetConditionMask",
        &[major, 1, 3],
    );
    assert_eq!(both, 0x800000000000001b);
    for (major, minor, expected) in [
        (9, 99, 0),
        (10, 0, 0),
        (10, 1, 0xc0000059),
        (11, 0, 0xc0000059),
    ] {
        mem.write_int(output + 4, 4, major).unwrap();
        mem.write_int(output + 8, 4, minor).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ntdll.dll",
                "RtlVerifyVersionInfo",
                &[output, 3, both]
            ),
            expected
        );
    }
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[777]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "RtlVerifyVersionInfo",
            &[output, 3, both]
        ),
        0xc0000059
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        777
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VerifyVersionInfoW",
            &[output, 3, both]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1150
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "RtlVerifyVersionInfo",
            &[output, 0, both]
        ),
        0xc000000d
    );
    mem.write_int(output + 280, 2, 0x100).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "RtlVerifyVersionInfo",
            &[output, 0x40, 6 << 18]
        ),
        0
    );
    mem.write_int(output + 280, 2, 1).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "RtlVerifyVersionInfo",
            &[output, 0x40, 7 << 18]
        ),
        0xc0000059
    );

    mem.write(output, b"ntdll\0").unwrap();
    let ntdll = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleHandleA",
        &[output],
    );
    assert_ne!(ntdll, 0);
    mem.write(output + 32, b"RtlVerifyVersionInfo\0").unwrap();
    assert_ne!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[ntdll, output + 32]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "FreeLibrary", &[ntdll]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetModuleHandleA",
            &[output]
        ),
        ntdll
    );
    let bad = STACK + 4096 - 100;
    mem.write(bad, &[0xcc; 100]).unwrap();
    mem.write_int(bad, 4, 276).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.regs[4] = STACK + 0x188;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = bad;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("ntdll.dll", "RtlGetVersion", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(bad + 4, 8).unwrap(), 0xcccccccccccccccc);
}
#[test]
fn win64_token_sid_layout_access_and_local_reallocation_preserve_failure_state() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let out = STACK + 0x100;
    let used = STACK + 0x180;
    let info = STACK + 0x200;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "OpenProcessToken",
            &[u64::MAX, 8, out]
        ),
        1
    );
    let handle = mem.read_int(out, 8).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetTokenInformation",
            &[handle, 25, 0, 0, used]
        ),
        0
    );
    assert_eq!(mem.read_int(used, 4).unwrap(), 28);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        122
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetTokenInformation",
            &[handle, 25, info, 28, used]
        ),
        1
    );
    let sid = mem.read_int(info, 8).unwrap();
    assert_eq!(sid, info + 16);
    assert_eq!(mem.read_int(info + 8, 4).unwrap(), 0x60);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetSidSubAuthority",
            &[sid, 0]
        ),
        sid + 8
    );
    assert_eq!(mem.read_int(sid + 8, 4).unwrap(), 8192);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetLengthSid",
            &[sid]
        ),
        12
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "CopySid",
            &[12, info + 64, sid]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "EqualSid",
            &[sid, info + 64]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "IsValidSid",
            &[1 << 48]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1337
    );
    mem.write_int(out, 8, 0xabcdef).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "OpenProcessToken",
            &[u64::MAX, 0x20, out]
        ),
        0
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xabcdef);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "OpenProcessToken",
            &[u64::MAX, 0, out]
        ),
        1
    );
    let inaccessible = mem.read_int(out, 8).unwrap();
    mem.write_int(used, 4, 0x1234).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetTokenInformation",
            &[inaccessible, 20, info, 28, used]
        ),
        0
    );
    assert_eq!(mem.read_int(used, 4).unwrap(), 0x1234);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        5
    );
    // A valid token with a bad output range leaves the return frame and required-size output untouched.
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = handle;
    cpu.regs[2] = 25;
    cpu.regs[8] = STACK + 8192 - 16;
    cpu.regs[9] = 28;
    mem.write_int(cpu.regs[4] + 40, 8, used).unwrap();
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("advapi32.dll", "GetTokenInformation", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(used, 4).unwrap(), 0x1234);
    let allocation = startup_call(&mut mem, &mut cpu, &mut runtime, "LocalAlloc", &[0x40, 8]);
    mem.write_int(allocation, 8, 0x12345678abcdef01).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "LocalReAlloc",
            &[allocation, 5000, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "LocalSize", &[allocation]),
        8
    );
    assert_eq!(mem.read_int(allocation, 8).unwrap(), 0x12345678abcdef01);
    let moved = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "LocalReAlloc",
        &[allocation, 5000, 0x42],
    );
    assert_ne!(moved, allocation);
    assert!(mem.read_int(allocation, 1).is_err());
    assert_eq!(mem.read_int(moved, 8).unwrap(), 0x12345678abcdef01);
    assert_eq!(mem.read_int(moved + 8, 8).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "LocalSize", &[moved]),
        5000
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "LocalFree", &[moved]),
        0
    );
    assert!(mem.read_int(moved, 1).is_err());
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[handle]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CloseHandle",
            &[inaccessible]
        ),
        1
    );
}
#[test]
fn win64_loaded_module_queries_messages_and_disabled_etw_keep_real_lifetimes() {
    use wire::x64::process::Process64;
    let dir = std::env::temp_dir().join(format!("wire-modulequery64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    let out = STACK + 0x100;
    let needed = STACK + 0x180;
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "K32EnumProcessModules",
            &[u64::MAX, out, 8, needed]
        ),
        1
    );
    assert_eq!(p.memory.read_int(out, 8).unwrap(), BASE);
    assert_eq!(p.memory.read_int(needed, 4).unwrap(), 24); // EXE, implicit kernel32 and ntdll.
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "K32EnumProcessModulesEx",
            &[u64::MAX, out, 8, needed, 1]
        ),
        1
    );
    assert_eq!(p.memory.read_int(needed, 4).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "K32GetModuleInformation",
            &[u64::MAX, BASE, out, 24]
        ),
        1
    );
    assert_eq!(p.memory.read_int(out, 8).unwrap(), BASE);
    assert_eq!(p.memory.read_int(out + 8, 4).unwrap(), 0x2000);
    assert_eq!(p.memory.read_int(out + 16, 8).unwrap(), BASE + 0x1000);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "psapi.dll",
            "GetModuleBaseNameA",
            &[u64::MAX, BASE, out, 32]
        ),
        8
    );
    let mut name = [0; 9];
    p.memory.read(out, &mut name).unwrap();
    assert_eq!(&name, b"main.exe\0");
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "K32GetModuleFileNameExA",
            &[u64::MAX, BASE, out, 32]
        ),
        11
    );
    let mut path = [0; 12];
    p.memory.read(out, &mut path).unwrap();
    assert_eq!(&path, b"C:\\main.exe\0");
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetModuleHandleExW",
            &[6, BASE + 0x1200, needed]
        ),
        1
    );
    assert_eq!(p.memory.read_int(needed, 8).unwrap(), BASE);
    assert!(p
        .runtime
        .dispatch(
            "kernel32.dll",
            "FreeLibrary",
            &mut {
                let mut cpu = p.cpu.clone();
                cpu.regs[4] = STACK + 0x1f88;
                p.memory.write_int(cpu.regs[4], 8, BASE).unwrap();
                cpu.regs[1] = BASE;
                cpu
            },
            &mut p.memory
        )
        .is_err()); // UNCHANGED_REFCOUNT does not create a releasable reference.
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetModuleHandleExW",
            &[4, BASE + 0x1200, needed]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FreeLibrary",
            &[BASE]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetModuleHandleExW",
            &[5, BASE + 0x1200, needed]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FreeLibrary",
            &[BASE]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FreeLibrary",
            &[BASE]
        ),
        1
    ); // A pinned image stays loaded.
    let count = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "FormatMessageW",
        &[0x1300, 0, 127, 0x400, out, 128, 0],
    );
    assert_eq!(count, 45);
    let text = p.memory.read_int(out, 8).unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "LocalSize",
            &[text]
        ),
        256
    );
    assert_eq!(p.memory.read_int(text + count * 2, 2).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "LocalFree",
            &[text]
        ),
        0
    );
    p.memory.write_int(out, 8, 0xfeed).unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FormatMessageW",
            &[0x1200, 0, 999999, 0, out, 128, 0]
        ),
        0
    );
    assert_eq!(p.memory.read_int(out, 8).unwrap(), 0xfeed);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        317
    );
    let guid = STACK + 0x200;
    p.memory.write(guid, &[0x42; 16]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventRegister",
            &[guid, 0x510000000, 0xdeadbeef12345678, out]
        ),
        0
    );
    let registration = p.memory.read_int(out, 8).unwrap();
    assert_ne!(registration, 0);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventEnabled",
            &[registration, guid]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventProviderEnabled",
            &[registration, 5, u64::MAX]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventWrite",
            &[registration, guid, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventUnregister",
            &[registration]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventWrite",
            &[registration, guid, 0, 0]
        ),
        6
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        317
    ); // ETW returns its error directly.
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn amd64_vectored_software_exceptions_execute_guest_handlers_and_restore_full_context_across_budgets(
) {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x540000000;
    const DATA: u64 = CODE + 4096;
    const VALUE: u64 = 0x123456789abcdef0;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    let path = std::env::temp_dir().join(format!("wire-veh64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for noncontinuable in [false, true] {
        let mut p = Process64::load(&path, false).unwrap();
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        let mut original = p.cpu.clone();
        original.rip = CODE;
        original.xmm[0] = [0xa5; 16];
        original.fpu.cw = 0x0b7f;
        original.fpu.sw = 3 << 11;
        original.fpu.set_st_bits(0, 0x3fff8000000000000000);
        original.fpu.ip = CODE + 0x123;
        original.fpu.dp = DATA + 0x456;
        original.rflags = 0x602;
        let expected_fpu = original.fpu;
        let raise = p.runtime.register("kernel32.dll", "RaiseException");
        // Register in append order. Both callbacks actually run guest instructions.
        for target in [CODE + 0x200, CODE + 0x300] {
            assert_ne!(
                startup_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "AddVectoredExceptionHandler",
                    &[0, target]
                ),
                0
            );
        }
        let mut main = vec![0x48, 0x83, 0xec, 0x28];
        for (r, v) in [
            (1, 0xe0400042),
            (2, noncontinuable as u64),
            (8, 2),
            (9, DATA + 256),
            (0, raise),
        ] {
            mov(&mut main, r, v);
        }
        main.extend([0xff, 0xd0]);
        mov(&mut main, 7, DATA);
        main.extend([0x48, 0x89, 0x07, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(CODE, &main).unwrap();
        p.memory
            .write_int(DATA + 256, 8, 0x1122334455667788)
            .unwrap();
        p.memory
            .write_int(DATA + 264, 8, 0x8877665544332211)
            .unwrap();
        p.memory.write_int(DATA + 128, 2, 0x077f).unwrap();
        // First callback reads the real EXCEPTION_RECORD and stores the argument,
        // then returns CONTINUE_SEARCH rather than ending dispatch.
        let mut first = vec![0x48, 0x8b, 0x01, 0x48, 0x8b, 0x50, 0x20];
        mov(&mut first, 7, DATA + 16);
        first.extend([0x48, 0x89, 0x17, 0x48, 0x8b, 0x41, 8]);
        mov(&mut first, 7, DATA + 24);
        first.extend([0x48, 0x89, 0x07, 0x31, 0xc0, 0xc3]);
        p.memory.write(CODE + 0x200, &first).unwrap();
        // Second callback edits the context's RAX and deliberately clobbers its
        // own XMM0/control word. Resuming must use the saved exception context.
        let mut second = vec![0x48, 0x8b, 0x41, 8];
        mov(&mut second, 2, VALUE);
        second.extend([0x48, 0x89, 0x50, 120, 0x0f, 0x57, 0xc0]);
        mov(&mut second, 7, DATA + 128);
        second.extend([0xd9, 0x2f, 0xb8, 0xff, 0xff, 0xff, 0xff, 0xc3]);
        p.memory.write(CODE + 0x300, &second).unwrap();
        p.cpu = original;
        loop {
            let budget = p.cpu.work + 1;
            match p.run(budget) {
                Ok(Outcome::Budget) => assert!(p.cpu.work < 200),
                Ok(result) => {
                    assert!(!noncontinuable);
                    assert_eq!(result, Outcome::Returned(VALUE));
                    assert_eq!(p.cpu.xmm[0], [0xa5; 16]);
                    assert_eq!(p.cpu.fpu, expected_fpu);
                    assert_eq!(p.cpu.rflags, 0x602);
                    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), VALUE);
                    break;
                }
                Err(error) => {
                    assert!(noncontinuable, "{error}");
                    assert!(error.contains("noncontinuable"));
                    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0);
                    break;
                }
            }
        }
        assert_eq!(p.memory.read_int(DATA + 16, 8).unwrap(), 0x1122334455667788);
        assert_eq!(
            p.stages
                .iter()
                .filter(|(s, _)| s == "vectored exception handler")
                .count(),
            2
        );
        let context = p.memory.read_int(DATA + 24, 8).unwrap();
        assert!(context > u32::MAX as u64);
        if !noncontinuable {
            assert!(p.memory.read_int(context, 1).is_err());
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amd64_same_frame_exception_unwind_executes_guest_language_and_termination_handlers() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x560000000;
    const DATA: u64 = CODE + 4096;
    const VALUE: u64 = 0xfedcba9876543210;
    fn mov(code: &mut Vec<u8>, r: u8, value: u64) {
        code.extend([0x48 | (r >> 3), 0xb8 | (r & 7)]);
        code.extend(value.to_le_bytes());
    }
    let path = std::env::temp_dir().join(format!("wire-seh64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for termination in [false, true] {
        let mut p = Process64::load(&path, false).unwrap();
        let mut original = p.cpu.clone();
        original.rip = CODE;
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        let raise = p.runtime.register("kernel32.dll", "RaiseException");
        let unwind = p.runtime.register("kernel32.dll", "RtlUnwindEx");
        let mut main = vec![0x48, 0x83, 0xec, 0x28];
        for (r, v) in [(1, 0xe0410042), (2, 0), (8, 0), (9, 0), (0, raise)] {
            mov(&mut main, r, v);
        }
        main.extend([0xff, 0xd0]);
        // A returning RaiseException would write this marker. Target unwind must
        // instead transfer to the catch block with the requested 64-bit RAX.
        mov(&mut main, 7, DATA + 8);
        main.extend([0xc7, 0x07, 0xad, 0xde, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        let catch = main.len() as u64;
        mov(&mut main, 7, DATA);
        main.extend([0x48, 0x89, 0x07, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(CODE, &main).unwrap();
        // Guest handler dispatches using HandlerData, not a host-selected catch PC.
        let mut handler = vec![0x48, 0x83, 0xec, 0x38, 0xf7, 0x41, 4, 2, 0, 0, 0, 0x75, 0];
        let branch = handler.len() - 1;
        handler.extend([
            0x49, 0x89, 0xcc, 0x49, 0x89, 0xd5, 0x4d, 0x89, 0xc6, 0x4d, 0x89, 0xcf,
        ]);
        mov(&mut handler, 7, DATA + 16);
        handler.extend([0x4c, 0x89, 0x2f]);
        mov(&mut handler, 7, DATA + 32);
        handler.extend([
            0x4c, 0x89, 0x37, 0x49, 0x8b, 0x47, 0x38, 0x8b, 0x50, 0x10, 0x49, 0x03, 0x57, 8,
        ]);
        handler.extend([0x4c, 0x89, 0xe9, 0x4d, 0x89, 0xe0]);
        mov(&mut handler, 9, VALUE);
        handler.extend([
            0x4c, 0x89, 0x74, 0x24, 0x20, 0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0,
        ]);
        mov(&mut handler, 0, unwind);
        handler.extend([0xff, 0xd0, 0xcc]); // Unwind never returns to this call site.
        let cleanup = handler.len();
        handler[branch] = u8::try_from(cleanup - branch - 1).unwrap();
        handler.extend([0x8b, 0x41, 4]);
        mov(&mut handler, 7, DATA + 24);
        handler.extend([0x89, 0x07, 0xb8, 1, 0, 0, 0, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
        p.memory.write(CODE + 0x200, &handler).unwrap();
        let table = DATA + 512;
        p.memory.write_int(table, 4, 0).unwrap();
        p.memory.write_int(table + 4, 4, main.len() as u64).unwrap();
        p.memory.write_int(table + 8, 4, 4096 + 544).unwrap();
        let mut metadata = [0; 32];
        metadata[..8].copy_from_slice(&[
            if termination { 0x19 } else { 9 },
            4,
            1,
            0,
            4,
            0x42,
            0,
            0,
        ]);
        put32(&mut metadata, 8, 0x200);
        put32(&mut metadata, 12, 1);
        put32(&mut metadata, 16, 4);
        put32(&mut metadata, 20, catch as u32);
        put32(&mut metadata, 24, 1);
        put32(&mut metadata, 28, catch as u32);
        p.memory.write(DATA + 544, &metadata).unwrap();
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "RtlAddFunctionTable",
                &[table, 1, CODE]
            ),
            1
        );
        p.cpu = original.clone();
        loop {
            match p.run(p.cpu.work + 1).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 500),
                result => {
                    assert_eq!(result, Outcome::Returned(VALUE));
                    break;
                }
            }
        }
        assert_eq!(p.memory.read_int(DATA, 8).unwrap(), VALUE);
        assert_eq!(p.memory.read_int(DATA + 8, 8).unwrap(), 0);
        assert_eq!(
            p.memory.read_int(DATA + 16, 8).unwrap(),
            original.regs[4] - 40
        );
        assert_eq!(
            p.memory.read_int(DATA + 24, 4).unwrap(),
            if termination { 0x22 } else { 0 }
        );
        assert_eq!(p.cpu.regs[4], original.regs[4] + 8);
        let context = p.memory.read_int(DATA + 32, 8).unwrap();
        assert!(p.memory.read_int(context, 1).is_err());
        assert_eq!(
            p.stages
                .iter()
                .filter(|(s, _)| s == "language exception handler")
                .count(),
            1
        );
        assert_eq!(
            p.stages
                .iter()
                .filter(|(s, _)| s == "target unwind handler")
                .count(),
            termination as usize
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amd64_function_tables_lookup_real_metadata_and_execute_guest_lookup_callback_across_budgets() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x520000000;
    const DATA: u64 = CODE + 4096;
    fn mov(code: &mut Vec<u8>, reg: u8, value: u64) {
        code.extend([0x48 | (reg >> 3), 0xb8 | (reg & 7)]);
        code.extend(value.to_le_bytes());
    }
    let dir = std::env::temp_dir().join(format!("wire-unwind64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut b = fixture();
    put32(&mut b, 0x98 + 112 + 3 * 8, 0x1100);
    put32(&mut b, 0x98 + 116 + 3 * 8, 12);
    put32(&mut b, 0x300, 0x1000);
    put32(&mut b, 0x304, 0x100e);
    put32(&mut b, 0x308, 0x11f0);
    b[0x3f0] = 1;
    std::fs::write(dir.join("main.exe"), b).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    let original = p.cpu.clone();
    p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlLookupFunctionEntry",
            &[BASE + 0x1004, DATA, 0]
        ),
        BASE + 0x1100
    );
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), BASE);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlDeleteFunctionTable",
            &[BASE + 0x1100]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlLookupFunctionEntry",
            &[BASE + 0x1500, DATA, 0]
        ),
        0
    );
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0);
    let handler = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "AddVectoredExceptionHandler",
        &[1, CODE + 0x400],
    );
    assert_ne!(handler, 0);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "RemoveVectoredExceptionHandler",
            &[handler]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "RemoveVectoredExceptionHandler",
            &[handler]
        ),
        0
    );
    p.cpu.regs[4] = STACK + 0x1f88;
    p.memory.write_int(p.cpu.regs[4], 8, BASE).unwrap();
    p.cpu.regs[1] = 0xe0000001;
    p.cpu.regs[2] = 1;
    p.cpu.regs[8] = 0;
    p.cpu.regs[9] = 0;
    let before = p.cpu.clone();
    let error = p
        .runtime
        .dispatch("kernel32.dll", "RaiseException", &mut p.cpu, &mut p.memory)
        .unwrap_err();
    assert!(error.contains("code=0xe0000001"));
    assert_eq!(p.cpu.regs, before.regs);
    assert_eq!(p.cpu.rip, before.rip);
    let entry = DATA + 32;
    p.memory.write_int(entry, 4, 0x100).unwrap();
    p.memory.write_int(entry + 4, 4, 0x200).unwrap();
    p.memory.write_int(entry + 8, 4, 0x800).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlAddFunctionTable",
            &[entry, 1, CODE]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlAddFunctionTable",
            &[entry, 1, CODE]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlLookupFunctionEntry",
            &[CODE + 0x180, DATA, 0]
        ),
        entry
    );
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), CODE);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlDeleteFunctionTable",
            &[entry]
        ),
        1
    );
    let context = 0x123456789abcdef0;
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlInstallFunctionTableCallback",
            &[CODE | 3, CODE, 4096, CODE + 0x400, context, 0]
        ),
        1
    );
    let mut callback = Vec::new();
    mov(&mut callback, 0, DATA + 64);
    callback.extend([0x48, 0x89, 0x08, 0x48, 0x89, 0x50, 8]);
    mov(&mut callback, 0, entry);
    callback.push(0xc3);
    p.memory.write(CODE + 0x400, &callback).unwrap();
    let mut code = vec![0x48, 0x83, 0xec, 0x28];
    mov(&mut code, 1, CODE + 0x180);
    mov(&mut code, 2, DATA);
    mov(&mut code, 8, 0);
    mov(
        &mut code,
        0,
        p.runtime.register("ntdll.dll", "RtlLookupFunctionEntry"),
    );
    code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(CODE, &code).unwrap();
    p.cpu = original;
    p.cpu.rip = CODE;
    loop {
        let budget = p.cpu.work + 1;
        match p.run(budget).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 1000),
            value => {
                assert_eq!(value, Outcome::Returned(entry));
                break;
            }
        }
    }
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), CODE);
    assert_eq!(p.memory.read_int(DATA + 64, 8).unwrap(), CODE + 0x180);
    assert_eq!(p.memory.read_int(DATA + 72, 8).unwrap(), context);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlDeleteFunctionTable",
            &[CODE | 3]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlLookupFunctionEntry",
            &[CODE + 0x180, DATA, 0]
        ),
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn win64_file_move_replace_metadata_and_open_handle_failures_are_confined() {
    let dir = std::env::temp_dir().join(format!("wire-move64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("source.txt"), b"source").unwrap();
    std::fs::write(dir.join("target.txt"), b"target").unwrap();
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.set_filesystem(wire::fs::GuestFs::new(&dir).unwrap());
    let source = STACK + 0x100;
    let target = STACK + 0x200;
    mem.write(source, b"source.txt\0").unwrap();
    mem.write(target, b"target.txt\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MoveFileA",
            &[source, target]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        183
    );
    assert_eq!(std::fs::read(dir.join("source.txt")).unwrap(), b"source");
    assert_eq!(std::fs::read(dir.join("target.txt")).unwrap(), b"target");
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetFileAttributesA",
            &[source, 0x2000]
        ),
        1
    );
    let file = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[source, 0x80000000, 7, 0, 3, 0, 0],
    );
    assert_ne!(file, u64::MAX);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MoveFileExA",
            &[source, target, 1]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        32
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[file]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MoveFileExA",
            &[source, target, 1]
        ),
        1
    );
    assert!(!dir.join("source.txt").exists());
    assert_eq!(std::fs::read(dir.join("target.txt")).unwrap(), b"source");
    assert_ne!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesA",
            &[target]
        ) & 0x2000,
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "DeleteFileA", &[target]),
        1
    );
    assert!(!dir.join("target.txt").exists());
    // Unsupported delayed/cross-volume flags cannot silently move files.
    std::fs::write(dir.join("source.txt"), b"source").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MoveFileExA",
            &[source, target, 4]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
    assert!(dir.join("source.txt").is_file());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn win64_thread_handle_duplicates_keep_identity_and_independent_flags_without_closing_thread() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let out = STACK + 0x100;
    let flags = STACK + 0x180;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, u64::MAX - 1, u64::MAX, out, 0, 1, 2]
        ),
        1
    );
    let first = mem.read_int(out, 8).unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadId", &[first]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[first, flags]
        ),
        1
    );
    assert_eq!(mem.read_int(flags, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, first, u64::MAX, out, 0, 0, 2]
        ),
        1
    );
    let second = mem.read_int(out, 8).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[first, 2, 2]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[second, flags]
        ),
        1
    );
    assert_eq!(mem.read_int(flags, 4).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[first]),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[first, 2, 0]
        ),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[first]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadId", &[second]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetExitCodeThread",
            &[second, flags]
        ),
        1
    );
    assert_eq!(mem.read_int(flags, 4).unwrap(), 259);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, second, u64::MAX, out, 0, 1, 3]
        ),
        1
    );
    let third = mem.read_int(out, 8).unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadId", &[second]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadId", &[third]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[third, flags]
        ),
        1
    );
    assert_eq!(mem.read_int(flags, 4).unwrap(), 1);
    mem.write_int(out, 8, 0xfeed).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, third, u64::MAX, out, 0x40, 0, 0]
        ),
        0
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xfeed);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[third]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetCurrentThreadId", &[]),
        1
    );
}
#[test]
fn win64_startup_layout_time_tls_and_recursive_locks_are_real_checked_objects() {
    use wire::x64::runtime::{PEB, TEB};
    let (mut mem, mut cpu, mut runtime) = startup();
    assert_eq!(mem.read_int(TEB + 0x30, 8).unwrap(), TEB);
    assert_eq!(mem.read_int(TEB + 0x60, 8).unwrap(), PEB);
    assert_eq!(mem.read_int(PEB + 0x10, 8).unwrap(), BASE);
    let output = STACK + 0x100;
    let flags = cpu.rflags;
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetSystemTimeAsFileTime",
        &[output],
    );
    assert!(mem.read_int(output, 8).unwrap() > 116444736000000000);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "QueryPerformanceFrequency",
        &[output],
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 10_000_000);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetStartupInfoW",
        &[output],
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 104);
    assert_eq!(mem.read_int(output + 96, 8).unwrap(), 0);
    let slot = startup_call(&mut mem, &mut cpu, &mut runtime, "TlsAlloc", &[]);
    let value = 0x123456789abcdef0;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TlsSetValue",
            &[slot, value]
        ),
        1
    );
    assert_eq!(mem.read_int(TEB + 0x1480 + slot * 8, 8).unwrap(), value);
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[123]);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "TlsGetValue", &[slot]),
        value
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0
    );
    assert_eq!(mem.read_int(PEB + 0x10, 8).unwrap(), BASE); // TEB TLS slots must not overlap PEB.
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "InitializeCriticalSectionAndSpinCount",
        &[output, 4000],
    );
    for _ in 0..2 {
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "EnterCriticalSection",
            &[output],
        );
    }
    assert_eq!(mem.read_int(output + 12, 4).unwrap(), 2);
    assert_eq!(mem.read_int(output + 16, 8).unwrap(), 1);
    for _ in 0..2 {
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "LeaveCriticalSection",
            &[output],
        );
    }
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), u32::MAX as u64);
    assert_eq!(mem.read_int(output + 16, 8).unwrap(), 0);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "InitializeCriticalSection",
        &[output],
    );
    assert_eq!(mem.read_int(output + 32, 8).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetCriticalSectionSpinCount",
            &[output, 777]
        ),
        0
    );
    assert_eq!(mem.read_int(output + 32, 8).unwrap(), 777);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryEnterCriticalSection",
            &[output]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryEnterCriticalSection",
            &[output]
        ),
        1
    );
    assert_eq!(mem.read_int(output + 12, 4).unwrap(), 2);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "LeaveCriticalSection",
        &[output],
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "LeaveCriticalSection",
        &[output],
    );
    // TryEnter never blocks, steals another thread's lock, or changes the LastError value.
    mem.write_int(output + 16, 8, 2).unwrap();
    mem.write_int(output + 12, 4, 1).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryEnterCriticalSection",
            &[output]
        ),
        0
    );
    assert_eq!(mem.read_int(output + 16, 8).unwrap(), 2);
    assert_eq!(cpu.rflags, flags);
}
#[test]
fn win64_gateway_faults_preserve_cpu_and_outputs_and_heap_payloads_survive_realloc() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x100;
    mem.write_int(output, 8, 0x1122334455667788).unwrap();
    cpu.regs[1] = output;
    cpu.regs[4] = 0x123000;
    let before = cpu.clone();
    assert!(runtime
        .dispatch(
            "kernel32.dll",
            "GetSystemTimeAsFileTime",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(mem.read_int(output, 8).unwrap(), 0x1122334455667788);
    assert_eq!(runtime.api_calls, 0);
    cpu.regs[4] = STACK + 0x1fc8;
    cpu.regs[1] = STACK + 8190;
    let before = cpu.clone();
    assert!(runtime
        .dispatch(
            "kernel32.dll",
            "GetSystemTimeAsFileTime",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(runtime.api_calls, 0);
    let heap = startup_call(&mut mem, &mut cpu, &mut runtime, "GetProcessHeap", &[]);
    let ptr = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "HeapAlloc",
        &[heap, 8, 5000],
    );
    assert!(ptr > u32::MAX as u64);
    mem.write_int(ptr + 4094, 8, 0x123456789abcdef0).unwrap();
    let grown = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "HeapReAlloc",
        &[heap, 8, ptr, 10000],
    );
    assert_eq!(mem.read_int(grown + 4094, 8).unwrap(), 0x123456789abcdef0);
    assert_eq!(mem.read_int(grown + 9000, 8).unwrap(), 0);
    assert!(mem.read_int(ptr, 1).is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapSize",
            &[heap, 0, grown]
        ),
        10000
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapFree",
            &[heap, 0, grown]
        ),
        1
    );
    assert!(mem.read_int(grown, 1).is_err());
    // No exception-handler reserve exists: query zero, reject a nonzero guarantee.
    mem.write_int(output, 4, 0).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetThreadStackGuarantee",
            &[output]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0);
    mem.write_int(output, 4, 65536).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetThreadStackGuarantee",
            &[output]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 65536);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
}
#[test]
fn amd64_shifts_and_rotates_match_unicorn_at_mask_and_width_boundaries() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let values = [0, 1, u64::MAX, 0x8000000000000000, 0xdeadbeef87654321];
    let counts = [0, 1, 7, 8, 15, 16, 31, 32, 63, 64, 65, 255];
    for bits in [8, 16, 32, 64] {
        for (opcode, rotate) in [
            (0xe0, false),
            (0xe8, false),
            (0xf8, false),
            (0xc0, true),
            (0xc8, true),
        ] {
            let mut code = Vec::new();
            if bits == 64 {
                code.push(0x48);
            } else if bits == 16 {
                code.push(0x66);
            }
            code.extend_from_slice(&[if bits == 8 { 0xd2 } else { 0xd3 }, opcode]);
            let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
            uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
            uc.mem_write(BASE, &code).unwrap();
            let mut mem = Memory::default();
            mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
            mem.write(BASE, &code).unwrap();
            for value in values {
                for count in counts {
                    let mut cpu = Cpu64::new(BASE, STACK);
                    cpu.regs[0] = value;
                    cpu.regs[1] = count;
                    cpu.rflags = 0xad7;
                    uc.reg_write(U::RAX, value).unwrap();
                    uc.reg_write(U::RCX, count).unwrap();
                    uc.reg_write(U::RFLAGS, cpu.rflags).unwrap();
                    uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                    cpu.step(&mut mem).unwrap();
                    assert_eq!(
                        cpu.regs[0],
                        uc.reg_read(U::RAX).unwrap(),
                        "bits={bits} op={opcode:x} count={count}"
                    );
                    let masked = count & if bits == 64 { 63 } else { 31 };
                    let mut flags = 0x8c5;
                    if masked > 1 {
                        flags &= !0x800;
                    } // OF undefined for multi-bit shifts/rotates.
                    if !rotate && masked >= bits {
                        flags &= !1;
                    } // CF undefined at/above operand width.
                    assert_eq!(
                        cpu.rflags & flags,
                        uc.reg_read(U::RFLAGS).unwrap() & flags,
                        "bits={bits} op={opcode:x} count={count}"
                    );
                }
            }
        }
    }
}

#[test]
fn amd64_cmov_setcc_locked_integer_and_negative_memory_bits_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let code = [
        0x0f, 0x44, 0xc3, // cmove eax,ebx (including false zero-extension)
        0x0f, 0x95, 0xc2, // setne dl
        0xf0, 0x48, 0x0f, 0xb1, 0x1f, // lock cmpxchg [rdi],rbx
        0xf0, 0x48, 0x0f, 0xc1, 0x1f, // lock xadd [rdi],rbx
        0x48, 0x0f, 0xa3, 0x4f, 8, // bt [rdi+8],rcx (negative bit offset)
        0xf0, 0x48, 0x0f, 0xba, 0x37, 1, // lock btr [rdi],1
        0x48, 0x87, 0x1f, // xchg [rdi],rbx
    ];
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 8192, Prot::ALL).unwrap();
    uc.mem_write(BASE, &code).unwrap();
    for flags in [0x202, 0x243, 0xad7] {
        for value in [0, 1, 0x123456789abcdef0, u64::MAX] {
            let mut mem = Memory::default();
            mem.map(BASE, 8192, READ | WRITE | EXECUTE).unwrap();
            mem.write(BASE, &code).unwrap();
            let mut cpu = Cpu64::new(BASE, STACK);
            cpu.regs[0] = value;
            cpu.regs[3] = 0xfffffffffeedbabe;
            cpu.regs[1] = u64::MAX;
            cpu.regs[7] = BASE + 4096;
            cpu.rflags = flags;
            mem.write_int(BASE + 4096, 8, value).unwrap();
            uc.mem_write(BASE + 4096, &value.to_le_bytes()).unwrap();
            for (register, v) in [
                (U::RAX, cpu.regs[0]),
                (U::RBX, cpu.regs[3]),
                (U::RCX, cpu.regs[1]),
                (U::RDX, 0),
                (U::RDI, cpu.regs[7]),
                (U::RFLAGS, flags),
            ] {
                uc.reg_write(register, v).unwrap();
            }
            while cpu.rip < BASE + code.len() as u64 {
                let i = cpu.decode(&mem).unwrap();
                uc.emu_start(cpu.rip, BASE + code.len() as u64, 0, 1)
                    .unwrap();
                cpu.step(&mut mem).unwrap();
                for (register, index) in [(U::RAX, 0), (U::RBX, 3), (U::RCX, 1), (U::RDX, 2)] {
                    assert_eq!(
                        uc.reg_read(register).unwrap(),
                        cpu.regs[index],
                        "{:?}",
                        i.mnemonic()
                    );
                }
                let flags = if matches!(
                    i.mnemonic(),
                    iced_x86::Mnemonic::Bt | iced_x86::Mnemonic::Btr
                ) {
                    1
                } else {
                    0x8d5
                };
                assert_eq!(
                    uc.reg_read(U::RFLAGS).unwrap() & flags,
                    cpu.rflags & flags,
                    "{:?}",
                    i.mnemonic()
                );
                assert_eq!(
                    uc.mem_read_as_vec(BASE + 4096, 8).unwrap(),
                    mem.read_int(BASE + 4096, 8).unwrap().to_le_bytes()
                );
            }
        }
    }
    // A false CMOV still checks its memory source and cannot retire on fault.
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.write(BASE, &[0x48, 0x0f, 0x44, 0x03]).unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.regs[3] = STACK;
    cpu.rflags &= !64;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.retired, 0);
}
#[test]
fn amd64_sse_moves_aliasing_zeroing_and_checked_stores_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let code = [
        0xf3, 0x44, 0x0f, 0x6f, 0x07, // movdqu xmm8,[rdi]
        0x44, 0x0f, 0x28, 0xc8, // movaps xmm9,xmm0
        0xf3, 0x45, 0x0f, 0x10, 0xc8, // movss xmm9,xmm8 (retain high lanes)
        0xf2, 0x44, 0x0f, 0x10, 0x57, 0x10, // movsd xmm10,[rdi+16] (clear high lanes)
        0x66, 0x45, 0x0f, 0xef, 0xc8, // pxor xmm9,xmm8
        0x45, 0x0f, 0x54, 0xc8, // andps xmm9,xmm8
        0x66, 0x45, 0x0f, 0x56, 0xc8, // orpd xmm9,xmm8
        0x66, 0x4c, 0x0f, 0x7e, 0xc8, // movq rax,xmm9
        0x66, 0x4c, 0x0f, 0x6e, 0xd8, // movq xmm11,rax (clear high lanes)
        0x45, 0x0f, 0x57, 0xdb, // xorps xmm11,xmm11
        0x66, 0x45, 0x0f, 0x6c, 0xca, // punpcklqdq xmm9,xmm10
        0x45, 0x0f, 0x14, 0xca, // unpcklps xmm9,xmm10
        0x66, 0x45, 0x0f, 0x15, 0xca, // unpckhpd xmm9,xmm10
        0x45, 0x0f, 0x16, 0xca, // movlhps xmm9,xmm10
        0x45, 0x0f, 0x12, 0xca, // movhlps xmm9,xmm10
        0x44, 0x0f, 0x16, 0x4f, 0x08, // movhps xmm9,[rdi+8]
        0x66, 0x44, 0x0f, 0x12, 0x4f, 0x08, // movlpd xmm9,[rdi+8]
        0x44, 0x0f, 0x17, 0x4f, 0x18, // movhps [rdi+24],xmm9
        0x66, 0x45, 0x0f, 0x70, 0xc9, 0x39, // pshufd xmm9,xmm9,39h
        0x66, 0x45, 0x0f, 0x74, 0xc2, // pcmpeqb xmm8,xmm10
        0x66, 0x45, 0x0f, 0x64, 0xc2, // pcmpgtb xmm8,xmm10
        0x66, 0x45, 0x0f, 0xfc, 0xc8, // paddb xmm9,xmm8
        0x66, 0x45, 0x0f, 0xfa, 0xca, // psubd xmm9,xmm10
        0x66, 0x41, 0x0f, 0x72, 0xf1, 3, // pslld xmm9,3
        0x66, 0x41, 0x0f, 0x72, 0xe1, 31, // psrad xmm9,31
        0x66, 0x41, 0x0f, 0x73, 0xd9, 1, // psrldq xmm9,1
        0x66, 0x45, 0x0f, 0xd7, 0xc1, // pmovmskb r8d,xmm9
        0xf3, 0x44, 0x0f, 0x7f, 0x4f, 0x20, // movdqu [rdi+32],xmm9
    ];
    let initial: Vec<u8> = (0..64).map(|n| n * 3 + 1).collect();
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 8192, Prot::ALL).unwrap();
    uc.mem_write(BASE, &code).unwrap();
    uc.mem_write(BASE + 4096, &initial).unwrap();
    uc.reg_write(U::RDI, BASE + 4096).unwrap();
    uc.reg_write(U::RFLAGS, 0xad7).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 8192, READ | WRITE | EXECUTE).unwrap();
    mem.write(BASE, &code).unwrap();
    mem.write(BASE + 4096, &initial).unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.regs[7] = BASE + 4096;
    cpu.rflags = 0xad7;
    let vectors = [U::XMM0, U::XMM8, U::XMM9, U::XMM10, U::XMM11];
    for (register, index) in vectors.into_iter().zip([0, 8, 9, 10, 11]) {
        cpu.xmm[index] = [index as u8 + 13; 16];
        uc.reg_write_long(register, &cpu.xmm[index]).unwrap();
    }
    while cpu.rip < BASE + code.len() as u64 {
        uc.emu_start(cpu.rip, BASE + code.len() as u64, 0, 1)
            .unwrap();
        cpu.step(&mut mem).unwrap();
        for (register, index) in vectors.into_iter().zip([0, 8, 9, 10, 11]) {
            assert_eq!(&*uc.reg_read_long(register).unwrap(), &cpu.xmm[index]);
        }
        assert_eq!(uc.reg_read(U::RAX).unwrap(), cpu.regs[0]);
        assert_eq!(uc.reg_read(U::R8).unwrap(), cpu.regs[8]);
        assert_eq!(cpu.rflags, 0xad7);
    }
    let mut bytes = [0; 64];
    mem.read(BASE + 4096, &mut bytes).unwrap();
    assert_eq!(uc.mem_read_as_vec(BASE + 4096, 64).unwrap(), bytes);
    // Cross-page vector store cannot commit either bytes or XMM/PC on a checked fault.
    mem.write(BASE, &[0xf3, 0x44, 0x0f, 0x7f, 0x4f, 0x20])
        .unwrap();
    cpu.rip = BASE;
    cpu.regs[7] = BASE + 8192 - 40;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu.xmm, before.xmm);
    assert_eq!(cpu.rip, before.rip);
}

#[test]
fn amd64_rep_has_bounded_partial_progress_and_checked_fault_restart() {
    let mut mem = Memory::default();
    mem.map(BASE, 8192, READ | WRITE | EXECUTE).unwrap();
    mem.write(BASE, &[0xf3, 0x48, 0xab]).unwrap(); // rep stosq
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.regs[0] = 0x1122334455667788;
    cpu.regs[1] = 3;
    cpu.regs[7] = BASE + 8192 - 16;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.rip, BASE);
    assert_eq!(cpu.retired, 0);
    assert_eq!(cpu.work, 1);
    assert_eq!(cpu.regs[1], 2);
    cpu.step(&mut mem).unwrap();
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(cpu.retired, 0);
    assert_eq!(cpu.work, 2);
    assert_eq!(
        mem.read_int(BASE + 8192 - 16, 8).unwrap(),
        0x1122334455667788
    );
    mem.map(BASE + 8192, 4096, READ | WRITE).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.rip, BASE + 3);
    assert_eq!(cpu.retired, 1);
    assert_eq!(cpu.work, 3);
    assert_eq!(cpu.regs[1], 0);
    // Empty REP never accesses either pointer, and still finishes one instruction.
    cpu.rip = BASE;
    cpu.regs[7] = u64::MAX;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[7], u64::MAX);
    assert_eq!(cpu.retired, 2);
    // Backwards overlapping MOVS follows individual architectural copies.
    mem.write(BASE, &[0xf3, 0xa4]).unwrap();
    mem.write(BASE + 0x1000, &[1, 2, 3, 4, 5]).unwrap();
    cpu.rip = BASE;
    cpu.regs[6] = BASE + 0x1003;
    cpu.regs[7] = BASE + 0x1004;
    cpu.regs[1] = 4;
    cpu.rflags |= 1024;
    for _ in 0..4 {
        cpu.step(&mut mem).unwrap();
    }
    let mut bytes = [0; 5];
    mem.read(BASE + 0x1000, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 1, 2, 3, 4]);
}

#[test]
fn amd64_scalar_sse_rounding_nan_denormals_and_sticky_status_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    // Unicorn checks result bits, but does not expose accrued MXCSR exception flags.
    // Sticky status and unmasked faults are checked separately below.
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    for size in [4, 8] {
        let one: u64 = if size == 4 {
            0x3f800000
        } else {
            0x3ff0000000000000
        };
        let max = if size == 4 {
            0x7f7fffff
        } else {
            0x7fefffffffffffff
        };
        let infinity = if size == 4 {
            0x7f800000
        } else {
            0x7ff0000000000000
        };
        let quiet = if size == 4 {
            0x7fc00123
        } else {
            0x7ff8000000000123
        };
        let signal = if size == 4 {
            0x7f800456
        } else {
            0x7ff0000000000456
        };
        for opcode in [0x58, 0x5c, 0x59, 0x5e] {
            let code = [if size == 4 { 0xf3 } else { 0xf2 }, 0x0f, opcode, 0xc1];
            mem.write(BASE, &code).unwrap();
            uc.mem_write(BASE, &code).unwrap();
            for rounding in 0..4 {
                for mode in [0, 0x40] {
                    for (a, b) in [
                        (one, one + 1),
                        (one, 0),
                        (0, 0),
                        (max, max),
                        (1, one),
                        (one, 1),
                        (1, 2),
                        (infinity, infinity),
                        (quiet, signal),
                        (signal, quiet),
                        (quiet, one),
                        (one, signal),
                        (quiet, 1),
                        (1, quiet),
                    ] {
                        let mut cpu = Cpu64::new(BASE, STACK);
                        cpu.mxcsr = 0x1f80 | rounding << 13 | mode;
                        cpu.xmm[0] = [0x5a; 16];
                        cpu.xmm[1] = [0xa5; 16];
                        cpu.xmm[0][..size].copy_from_slice(&a.to_le_bytes()[..size]);
                        cpu.xmm[1][..size].copy_from_slice(&b.to_le_bytes()[..size]);
                        uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                        uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                        uc.reg_write_long(U::XMM1, &cpu.xmm[1]).unwrap();
                        uc.emu_start(BASE, BASE + 4, 0, 1).unwrap();
                        cpu.step(&mut mem).unwrap();
                        assert_eq!(
                            &*uc.reg_read_long(U::XMM0).unwrap(),
                            &cpu.xmm[0],
                            "size={size} opcode={opcode:x} a={a:x} b={b:x} mxcsr={:x}",
                            cpu.mxcsr
                        );
                    }
                }
            }
        }
    }
    // The dev oracle also leaves exact tiny FTZ results unflushed. Check Intel's
    // masked-underflow FTZ result and status independently, including exact tiny output.
    for (opcode, a, b, mode, flags, expected) in [
        (0x5e, 0x3f800000u64, 0, 0, 4, 0x7f800000),
        (0x5e, 0, 0, 0, 1, 0xffc00000),
        (0x58, 0x7f7fffff, 0x7f7fffff, 0, 0x28, 0x7f800000),
        (0x59, 1, 0x3f800000, 0, 2, 1),
        (0x59, 1, 0x3f800000, 0x8000, 0x32, 0),
        (0x59, 1, 0x3f800000, 0x8040, 0, 0),
        (0x58, 0x7fc00123, 1, 0, 0, 0x7fc00123),
    ] {
        mem.write(BASE, &[0xf3, 0x0f, opcode, 0xc1]).unwrap();
        let mut cpu = Cpu64::new(BASE, STACK);
        cpu.mxcsr |= mode;
        cpu.xmm[0][..4].copy_from_slice(&(a as u32).to_le_bytes());
        cpu.xmm[1][..4].copy_from_slice(&(b as u32).to_le_bytes());
        cpu.step(&mut mem).unwrap();
        assert_eq!(cpu.mxcsr & 0x3f, flags);
        assert_eq!(
            u32::from_le_bytes(cpu.xmm[0][..4].try_into().unwrap()),
            expected
        );
    }
    // An unmasked precision exception publishes status, with the destination/PC intact.
    mem.write(BASE, &[0xf3, 0x48, 0x0f, 0x2a, 0xc0]).unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.mxcsr &= !(1 << 12);
    cpu.regs[0] = 16777217;
    cpu.xmm[0] = [0xaa; 16];
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("#XM"));
    assert_eq!(cpu.mxcsr, before.mxcsr | (1 << 5));
    assert_eq!(cpu.xmm, before.xmm);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(cpu.retired, 0);
    assert_eq!(cpu.work, 0);
    // Memory faults happen before arithmetic and cannot publish status.
    mem.write(BASE, &[0xf2, 0x0f, 0x5e, 0x07]).unwrap();
    cpu.regs[7] = BASE + 4096;
    let status = cpu.mxcsr;
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu.mxcsr, status);
}

#[test]
fn amd64_sse_signed_conversion_and_mxcsr_load_store_are_checked() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    for prefix in [0xf2, 0xf3] {
        for width in [32, 64] {
            let mut code = vec![prefix];
            if width == 64 {
                code.push(0x48);
            }
            code.extend_from_slice(&[0x0f, 0x2a, 0xc0]);
            uc.mem_write(BASE, &code).unwrap();
            mem.write(BASE, &code).unwrap();
            for rounding in 0..4 {
                for value in [
                    0,
                    1,
                    u64::MAX,
                    16777217,
                    9007199254740993,
                    i64::MAX as u64,
                    i64::MIN as u64,
                ] {
                    let mut cpu = Cpu64::new(BASE, STACK);
                    cpu.mxcsr |= rounding << 13;
                    cpu.regs[0] = value;
                    cpu.xmm[0] = [0xcc; 16];
                    uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                    uc.reg_write(U::RAX, value).unwrap();
                    uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                    uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                    cpu.step(&mut mem).unwrap();
                    assert_eq!(&*uc.reg_read_long(U::XMM0).unwrap(), &cpu.xmm[0]);
                }
            }
        }
    }
    for prefix in [0xf2, 0xf3] {
        for width in [32, 64] {
            for opcode in [0x2c, 0x2d] {
                let mut code = vec![prefix];
                if width == 64 {
                    code.push(0x48);
                }
                code.extend([0x0f, opcode, 0xc0]);
                mem.write(BASE, &code).unwrap();
                uc.mem_write(BASE, &code).unwrap();
                // (bits, invalid for I32, invalid for I64, inexact). Unicorn does not
                // update MXCSR for these conversions, so status uses architectural vectors.
                let samples: &[(u64, bool, bool, bool)] = if prefix == 0xf3 {
                    &[
                        (0, false, false, false),
                        (0x80000000, false, false, false),
                        (0x3fc00000, false, false, true),
                        (0xbfc00000, false, false, true),
                        (0x4f000000, true, false, false),
                        (0x5f000000, true, true, false),
                        (0xdf000000, true, false, false),
                        (1, false, false, true),
                        (0x80000001, false, false, true),
                        (0x7fc12345, true, true, false),
                        (0x7f800001, true, true, false),
                        (0x7f800000, true, true, false),
                        (0xff800000, true, true, false),
                    ]
                } else {
                    &[
                        (0, false, false, false),
                        (0x8000000000000000, false, false, false),
                        (0x3ff8000000000000, false, false, true),
                        (0xbff8000000000000, false, false, true),
                        (0x41e0000000000000, true, false, false),
                        (0x43e0000000000000, true, true, false),
                        (0xc3e0000000000000, true, false, false),
                        (0x43dfffffffffffff, true, false, false),
                        (1, false, false, true),
                        (0x8000000000000001, false, false, true),
                        (0x7ff8123456789abc, true, true, false),
                        (0x7ff0000000000001, true, true, false),
                        (0x7ff0000000000000, true, true, false),
                        (0xfff0000000000000, true, true, false),
                    ]
                };
                for round in 0..4 {
                    for daz in [0, 1 << 6] {
                        for &(bits, invalid32, invalid64, inexact) in samples {
                            let mut cpu = Cpu64::new(BASE, STACK);
                            cpu.mxcsr |= round << 13 | daz;
                            let control = cpu.mxcsr;
                            cpu.regs[0] = u64::MAX;
                            cpu.xmm[0] = [0xa5; 16];
                            cpu.xmm[0][..8].copy_from_slice(&bits.to_le_bytes());
                            uc.reg_write(U::RAX, cpu.regs[0]).unwrap();
                            uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                            uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                            uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                            cpu.step(&mut mem).unwrap();
                            assert_eq!(
                                cpu.regs[0],
                                uc.reg_read(U::RAX).unwrap(),
                                "float to int {code:x?} bits={bits:x} round={round} daz={daz}"
                            );
                            let denormal = matches!(bits, 1 | 0x80000001 | 0x8000000000000001);
                            let invalid = if width == 32 { invalid32 } else { invalid64 };
                            let status = if invalid {
                                1
                            } else if inexact && !(daz != 0 && denormal) {
                                1 << 5
                            } else {
                                0
                            };
                            assert_eq!(
                                cpu.mxcsr,
                                control | status,
                                "float to int status {code:x?} bits={bits:x}"
                            );
                            assert_eq!(&cpu.xmm[0], &*uc.reg_read_long(U::XMM0).unwrap());
                        }
                    }
                }
            }
        }
    }
    mem.write(BASE, &[0xf2, 0x48, 0x0f, 0x2c, 0xc0]).unwrap();
    for (bits, flag, mask) in [
        (0x3ff8000000000000u64, 1 << 5, 1 << 12),
        (0x7ff8000000000000, 1, 1 << 7),
    ] {
        let mut cpu = Cpu64::new(BASE, STACK);
        cpu.mxcsr &= !mask;
        cpu.regs[0] = 0x123456789abcdef0;
        cpu.xmm[0][..8].copy_from_slice(&bits.to_le_bytes());
        let before = cpu.clone();
        assert!(cpu.step(&mut mem).unwrap_err().contains("#XM"));
        assert_eq!(cpu.regs, before.regs);
        assert_eq!(cpu.xmm, before.xmm);
        assert_eq!(cpu.rip, before.rip);
        assert_eq!(cpu.retired, before.retired);
        assert_eq!(cpu.mxcsr, before.mxcsr | flag);
    }
    mem.write(BASE, &[0x0f, 0xae, 0x17, 0x0f, 0xae, 0x1f])
        .unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.regs[7] = BASE + 0x100;
    mem.write_int(cpu.regs[7], 4, 0x7f80).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.mxcsr, 0x7f80);
    cpu.step(&mut mem).unwrap();
    assert_eq!(mem.read_int(cpu.regs[7], 4).unwrap(), 0x7f80);
    cpu.rip = BASE;
    mem.write_int(cpu.regs[7], 4, 0x10000).unwrap();
    assert!(cpu.step(&mut mem).unwrap_err().contains("#GP"));
    assert_eq!(cpu.mxcsr, 0x7f80);
    assert_eq!(cpu.rip, BASE);
}

#[test]
fn amd64_packed_integer_conversions_match_lanes_and_preserve_fault_atomicity() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    // Legacy SSE encodings: I32->F64/F32 and rounded/truncated F32/F64->I32.
    let instructions: &[(&[u8], bool, bool)] = &[
        (&[0xf3, 0x0f, 0xe6, 0xc1], true, true),
        (&[0x0f, 0x5b, 0xc1], true, false),
        (&[0x66, 0x0f, 0x5b, 0xc1], false, false),
        (&[0xf3, 0x0f, 0x5b, 0xc1], false, false),
        (&[0xf2, 0x0f, 0xe6, 0xc1], false, true),
        (&[0x66, 0x0f, 0xe6, 0xc1], false, true),
    ];
    for &(code, integer, double) in instructions {
        let samples: &[u64] = if integer {
            &[
                0, 1, 0xffffffff, 16777217, 0xfeffffff, 0x7fffffff, 0x80000000,
            ]
        } else if double {
            &[
                0,
                0x8000000000000000,
                0x3ff8000000000000,
                0xbff8000000000000,
                0x41dfffffffc00000,
                0x41e0000000000000,
                0xc1e0000000000000,
                1,
                0x8000000000000001,
                0x7ff8123456789abc,
                0x7ff0000000000001,
                0x7ff0000000000000,
                0xfff0000000000000,
            ]
        } else {
            &[
                0, 0x80000000, 0x3fc00000, 0xbfc00000, 0x4effffff, 0x4f000000, 0xcf000000, 1,
                0x80000001, 0x7fc12345, 0x7f800001, 0x7f800000, 0xff800000,
            ]
        };
        mem.write(BASE, code).unwrap();
        uc.mem_write(BASE, code).unwrap();
        for round in 0..4 {
            for daz in [0, 1 << 6] {
                for start in 0..samples.len() {
                    let mut cpu = Cpu64::new(BASE, STACK);
                    cpu.mxcsr |= round << 13 | daz;
                    cpu.xmm[0] = [0xa5; 16];
                    let width = if !integer && double { 8 } else { 4 };
                    for lane in 0..16 / width {
                        let bits = samples[(start + lane) % samples.len()].to_le_bytes();
                        cpu.xmm[1][lane * width..(lane + 1) * width]
                            .copy_from_slice(&bits[..width]);
                    }
                    let source = cpu.xmm[1];
                    uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                    uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                    uc.reg_write_long(U::XMM1, &source).unwrap();
                    uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                    cpu.step(&mut mem).unwrap();
                    assert_eq!(
                        &cpu.xmm[0],
                        &*uc.reg_read_long(U::XMM0).unwrap(),
                        "packed conversion {code:x?}, round={round}, daz={daz}, source={source:x?}"
                    );
                    assert_eq!(cpu.xmm[1], source);
                    assert_eq!(cpu.rflags, 0x202);
                }
            }
        }
        let mut memory_code = code.to_vec();
        *memory_code.last_mut().unwrap() = 0x07; // [RDI]
        mem.write(BASE, &memory_code).unwrap();
        let mut cpu = Cpu64::new(BASE, STACK);
        cpu.xmm[0] = [0xa5; 16];
        // CVTDQ2PD reads exactly 8 bytes and permits unaligned m64. The other
        // legacy m128 operands require 16-byte alignment independently of MXCSR.
        cpu.regs[7] = BASE + 4087;
        mem.write_int(cpu.regs[7], 8, 0x8000000000000001).unwrap();
        if integer && double {
            cpu.step(&mut mem).unwrap();
            assert_eq!(cpu.xmm[0][..8], 1f64.to_le_bytes());
            assert_eq!(cpu.xmm[0][8..], (-2147483648f64).to_le_bytes());
        } else {
            let before = cpu.clone();
            assert!(cpu.step(&mut mem).unwrap_err().contains("unaligned"));
            assert_eq!(cpu.xmm, before.xmm);
            assert_eq!(cpu.mxcsr, before.mxcsr);
            assert_eq!(cpu.rip, before.rip);
        }
        cpu.rip = BASE;
        cpu.regs[7] = BASE + 4096;
        let before = cpu.clone();
        assert!(cpu.step(&mut mem).is_err());
        assert_eq!(cpu.xmm, before.xmm);
        assert_eq!(cpu.mxcsr, before.mxcsr);
        assert_eq!(cpu.retired, before.retired);
        assert_eq!(cpu.rip, before.rip);
    }
    // The oracle does not reliably update conversion MXCSR. Check architectural
    // invalid/precision aggregation and unmasked #XM separately across all lanes.
    for code in [[0x66, 0x0f, 0x5b, 0xc1], [0xf3, 0x0f, 0x5b, 0xc1]] {
        mem.write(BASE, &code).unwrap();
        for control in [0x1f80, 0x1f00, 0x0f80] {
            let mut cpu = Cpu64::new(BASE, STACK);
            cpu.mxcsr = control;
            cpu.xmm[0] = [0xa5; 16];
            for (lane, bits) in [0x3fc00000u32, 0x7f800000, 0x3f800000, 1]
                .into_iter()
                .enumerate()
            {
                cpu.xmm[1][lane * 4..lane * 4 + 4].copy_from_slice(&bits.to_le_bytes());
            }
            let before = cpu.clone();
            let result = cpu.step(&mut mem);
            assert_eq!(cpu.mxcsr, control | 0x21);
            if control == 0x1f80 {
                result.unwrap();
                assert_eq!(cpu.xmm[0][4..8], 0x80000000u32.to_le_bytes());
            } else {
                assert!(result.unwrap_err().contains("#XM"));
                assert_eq!(cpu.xmm, before.xmm);
                assert_eq!(cpu.regs, before.regs);
                assert_eq!(cpu.rip, before.rip);
                assert_eq!(cpu.retired, before.retired);
            }
        }
    }
}

#[test]
fn amd64_x87_control_words_status_and_pending_exceptions_use_checked_high_addresses() {
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.regs[7] = BASE + 0x100;
    mem.write(BASE, &[0xd9, 0x3f, 0xd9, 0x2f, 0xdf, 0xe0])
        .unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(mem.read_int(BASE + 0x100, 2).unwrap(), 0x037f);
    mem.write_int(BASE + 0x100, 2, 0x0b7f).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.fpu.cw, 0x0b7f);
    cpu.fpu.sw = 0x4500;
    cpu.regs[0] = 0xabcdef0123456789;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0xabcdef0123454500);
    mem.write(BASE, &[0x9b, 0xdb, 0xe2, 0x9b, 0xdb, 0xe3])
        .unwrap();
    cpu.rip = BASE;
    cpu.fpu.cw = 0x037e;
    cpu.fpu.sw = 0xc581;
    cpu.fpu.regs[0] = 0x3fff8000000000000001;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("#MF"));
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(cpu.fpu.sw, before.fpu.sw);
    assert_eq!(cpu.retired, before.retired);
    cpu.rip = BASE + 1;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.fpu.sw, 0x4500);
    cpu.step(&mut mem).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.fpu.cw, 0x037f);
    assert_eq!(cpu.fpu.sw, 0);
    assert_eq!(cpu.fpu.tags, 0xffff);
    assert_eq!(cpu.fpu.regs[0], 0x3fff8000000000000001);
    mem.write(BASE, &[0xd9, 0x3f]).unwrap();
    cpu.rip = BASE;
    cpu.regs[7] = BASE + 4095;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(cpu.fpu.cw, before.fpu.cw);
    assert_eq!(cpu.retired, before.retired);
}
#[test]
fn amd64_x87_keeps_extended_precision_high_pointers_rounding_and_fault_atomicity() {
    const ONE: u128 = 0x3fff_8000_0000_0000_0000;
    let mut mem = Memory::default();
    mem.map(BASE, 8192, READ | WRITE | EXECUTE).unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.regs[7] = BASE + 0x100;
    cpu.regs[6] = BASE + 0x120;
    cpu.regs[2] = BASE + 0x140;
    // Extended 1 + 2^-64 is halfway at FP80 precision, invisible in binary64.
    mem.write(BASE, &[0xdb, 0x2f, 0xdb, 0x2e, 0xde, 0xc1, 0xdb, 0x3a])
        .unwrap();
    mem.write(BASE + 0x100, &ONE.to_le_bytes()[..10]).unwrap();
    mem.write(
        BASE + 0x120,
        &(((16383u128 - 64) << 64) | (1 << 63)).to_le_bytes()[..10],
    )
    .unwrap();
    for (control, expected, status) in [(0x037f, ONE, 0x20), (0x0b7f, ONE + 1, 0x220)] {
        cpu.rip = BASE;
        cpu.fpu = wire::cpu::Fpu::default();
        cpu.fpu.cw = control;
        for _ in 0..3 {
            cpu.step(&mut mem).unwrap();
        }
        assert_eq!(cpu.fpu.st_bits(0), expected);
        assert_eq!(cpu.fpu.sw & 0x23f, status);
        assert_eq!(cpu.fpu.ip, BASE + 4);
        assert_eq!(cpu.fpu.dp, BASE + 0x120);
        cpu.step(&mut mem).unwrap();
        let mut bytes = [0; 16];
        mem.read(BASE + 0x140, &mut bytes[..10]).unwrap();
        assert_eq!(u128::from_le_bytes(bytes), expected);
        assert_eq!(cpu.fpu.tags, 0xffff);
        assert_eq!(cpu.fpu.ip, BASE + 6);
        assert_eq!(cpu.fpu.dp, BASE + 0x140);
    }
    // Signed 64-bit loads are exact; integer stores use CW or truncate explicitly.
    mem.write(BASE, &[0xdf, 0x2f, 0xdf, 0x3a]).unwrap();
    for value in [i64::MIN, i64::MAX, -(1i64 << 54) - 1] {
        cpu.rip = BASE;
        cpu.fpu = wire::cpu::Fpu::default();
        mem.write_int(BASE + 0x100, 8, value as u64).unwrap();
        cpu.step(&mut mem).unwrap();
        cpu.step(&mut mem).unwrap();
        assert_eq!(mem.read_int(BASE + 0x140, 8).unwrap(), value as u64);
    }
    mem.write_int(BASE + 0x100, 8, 2.75f64.to_bits()).unwrap();
    for (control, expected) in [(0x037f, 3), (0x077f, 2), (0x0b7f, 3), (0x0f7f, 2)] {
        mem.write(BASE, &[0xdd, 0x07, 0xdb, 0x1a]).unwrap();
        cpu.rip = BASE;
        cpu.fpu = wire::cpu::Fpu::default();
        cpu.fpu.cw = control;
        cpu.step(&mut mem).unwrap();
        cpu.step(&mut mem).unwrap();
        assert_eq!(mem.read_int(BASE + 0x140, 4).unwrap(), expected);
        assert_eq!(cpu.fpu.sw & 0x20, 0x20);
    }
    mem.write(BASE, &[0xdd, 0x07, 0xdb, 0x0a]).unwrap();
    cpu.rip = BASE;
    cpu.fpu = wire::cpu::Fpu::default();
    cpu.step(&mut mem).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(mem.read_int(BASE + 0x140, 4).unwrap(), 2);
    // Read/write failures spanning pages preserve every CPU field and byte.
    for code in [[0xdb, 0x2f], [0xdb, 0x3f]] {
        mem.write(BASE, &code).unwrap();
        cpu.rip = BASE;
        cpu.regs[7] = BASE + 8192 - 5;
        mem.write(cpu.regs[7], &[0xa5; 5]).unwrap();
        let before = cpu.clone();
        assert!(cpu.step(&mut mem).is_err());
        assert_eq!(cpu, before);
        let mut bytes = [0; 5];
        mem.read(cpu.regs[7], &mut bytes).unwrap();
        assert_eq!(bytes, [0xa5; 5]);
    }
    // An unsupported new unmasked exception must not emit a store or pop.
    mem.write(BASE, &[0xdb, 0x1a]).unwrap();
    cpu.rip = BASE;
    cpu.fpu = wire::cpu::Fpu::default();
    cpu.fpu.cw &= !1;
    cpu.fpu.set_st_bits(0, 0x7fff_c000_0000_0000_0000);
    let before = cpu.clone();
    let previous = mem.read_int(BASE + 0x140, 4).unwrap();
    assert!(cpu
        .step(&mut mem)
        .unwrap_err()
        .contains("deferred delivery"));
    assert_eq!(cpu, before);
    assert_eq!(mem.read_int(BASE + 0x140, 4).unwrap(), previous);
}
#[test]
fn amd64_x87_arithmetic_register_directions_and_pops_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    let mut seed = 0xa71c_3319_8455_1281u64;
    for opcode in [0xd8, 0xdc, 0xde] {
        for byte in [0xc1, 0xc9, 0xe1, 0xe9, 0xf1, 0xf9] {
            for rounding in 0..4u16 {
                let mut code = vec![
                    0xdb, 0xe3, 0xd9, 0x6f, 0x20, 0xdd, 0x07, 0xdd, 0x06, opcode, byte,
                ];
                // DC writes ST1 and leaves ST0 intact; discard ST0 to observe it.
                if opcode == 0xdc {
                    code.extend([0xdd, 0xd8]);
                }
                code.extend([0xdd, 0x1a, 0xdf, 0xe0]);
                mem.write(BASE, &code).unwrap();
                uc.mem_write(BASE, &code).unwrap();
                mem.write_int(BASE + 0x120, 2, (0x037f | (rounding << 10)) as u64)
                    .unwrap();
                uc.mem_write(BASE + 0x120, &(0x037f | (rounding << 10)).to_le_bytes())
                    .unwrap();
                for _ in 0..32 {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let a = f64::from_bits((seed & 0x800f_ffff_ffff_ffff) | (0x3ffu64 << 52));
                    seed = seed.rotate_left(19).wrapping_add(0x772f_1591_5328);
                    let b = f64::from_bits((seed & 0x800f_ffff_ffff_ffff) | (0x400u64 << 52));
                    let mut cpu = Cpu64::new(BASE, STACK);
                    for (r, at, reg) in [
                        (7, BASE + 0x100, U::RDI),
                        (6, BASE + 0x140, U::RSI),
                        (2, BASE + 0x180, U::RDX),
                    ] {
                        cpu.regs[r] = at;
                        uc.reg_write(reg, at).unwrap();
                    }
                    for (at, value) in [(BASE + 0x100, a.to_bits()), (BASE + 0x140, b.to_bits())] {
                        mem.write_int(at, 8, value).unwrap();
                        uc.mem_write(at, &value.to_le_bytes()).unwrap();
                    }
                    uc.emu_start(BASE, BASE + code.len() as u64, 0, 0).unwrap();
                    while cpu.rip < BASE + code.len() as u64 {
                        cpu.step(&mut mem).unwrap();
                    }
                    let output = uc.mem_read_as_vec(BASE + 0x180, 8).unwrap();
                    assert_eq!(
                        mem.read_int(BASE + 0x180, 8).unwrap(),
                        u64::from_le_bytes(output.try_into().unwrap()),
                        "{opcode:x} {byte:x} round={rounding} a={a} b={b}"
                    );
                    // Unicorn does not model all sticky x87 precision flags.
                    assert_eq!(cpu.regs[0] & 0x3800, uc.reg_read(U::RAX).unwrap() & 0x3800);
                }
            }
        }
    }
}
#[test]
fn amd64_sse_compare_nan_daz_flags_and_unmasked_faults_are_checked() {
    for prefix in [None, Some(0x66)] {
        let size = if prefix.is_some() { 8 } else { 4 };
        let one: u64 = if size == 4 {
            0x3f800000
        } else {
            0x3ff0000000000000
        };
        let quiet = if size == 4 {
            0x7fc00123
        } else {
            0x7ff8000000000123
        };
        let signal = if size == 4 {
            0x7f800456
        } else {
            0x7ff0000000000456
        };
        for opcode in [0x2e, 0x2f] {
            let mut code = Vec::new();
            code.extend(prefix);
            code.extend([0x0f, opcode, 0xc1]);
            let mut mem = Memory::default();
            mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
            mem.write(BASE, &code).unwrap();
            for (a, b, mode, flags, status) in [
                (one, one, 0, 0x40, 0),
                (one, 0, 0, 0, 0),
                (0, one, 0, 1, 0),
                (quiet, one, 0, 0x45, (opcode == 0x2f) as u32),
                (one, signal, 0, 0x45, 1),
                (1, 0, 0, 0, 2),
                (1, 0, 0x40, 0x40, 0),
            ] {
                let mut cpu = Cpu64::new(BASE, STACK);
                cpu.mxcsr |= mode;
                cpu.rflags = 0xed7;
                cpu.xmm[0][..size].copy_from_slice(&a.to_le_bytes()[..size]);
                cpu.xmm[1][..size].copy_from_slice(&b.to_le_bytes()[..size]);
                cpu.step(&mut mem).unwrap();
                assert_eq!(cpu.rflags & 0x8d5, flags);
                assert_eq!(cpu.mxcsr & 0x3f, status);
                if status != 0 {
                    cpu.rip = BASE;
                    cpu.mxcsr = (0x1f80 | mode) & !(status << 7);
                    cpu.rflags = 0xed7;
                    let before = cpu.clone();
                    assert!(cpu.step(&mut mem).unwrap_err().contains("#XM"));
                    assert_eq!(cpu.rflags, before.rflags);
                    assert_eq!(cpu.xmm, before.xmm);
                    assert_eq!(cpu.rip, BASE);
                    assert_eq!(cpu.retired, before.retired);
                }
            }
        }
    }
}

#[test]
fn amd64_sse_sqrt_uses_exact_integer_rounding_and_preserves_upper_lanes() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    let mut seed = 0x922145327f013456u64;
    for size in [4, 8] {
        let code = [if size == 4 { 0xf3 } else { 0xf2 }, 0x0f, 0x51, 0xc1];
        uc.mem_write(BASE, &code).unwrap();
        mem.write(BASE, &code).unwrap();
        for rounding in 0..4 {
            for mode in [0, 0x40] {
                for n in 0..128 {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let bits = match n {
                        0 => 0,
                        1 => 1,
                        2 => 2,
                        3 => {
                            if size == 4 {
                                0x80000000
                            } else {
                                0x8000000000000000
                            }
                        }
                        4 => {
                            if size == 4 {
                                0xbf800000
                            } else {
                                0xbff0000000000000
                            }
                        }
                        5 => {
                            if size == 4 {
                                0x7f800123
                            } else {
                                0x7ff0000000000123
                            }
                        }
                        _ => seed,
                    };
                    let mut cpu = Cpu64::new(BASE, STACK);
                    cpu.mxcsr |= rounding << 13 | mode;
                    cpu.xmm[0] = [0xbb; 16];
                    cpu.xmm[1][..size].copy_from_slice(&bits.to_le_bytes()[..size]);
                    uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                    uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                    uc.reg_write_long(U::XMM1, &cpu.xmm[1]).unwrap();
                    uc.emu_start(BASE, BASE + 4, 0, 1).unwrap();
                    cpu.step(&mut mem).unwrap();
                    assert_eq!(
                        &*uc.reg_read_long(U::XMM0).unwrap(),
                        &cpu.xmm[0],
                        "size={size} bits={bits:x} round={rounding}"
                    );
                }
            }
        }
    }
}

#[test]
fn amd64_reciprocal_estimates_obey_error_bound_and_ignore_mxcsr_controls() {
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    let mut seed = 0x772a53ddu32;
    for opcode in [0x52, 0x53] {
        for scalar in [false, true] {
            let mut code = Vec::new();
            if scalar {
                code.push(0xf3);
            }
            code.extend([0x0f, opcode, 0xc1]);
            mem.write(BASE, &code).unwrap();
            for mode in [0, 0x6000, 0x8040] {
                for _ in 0..256 {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    // Positive normal inputs across every representable exponent.
                    let bits = ((seed % 254 + 1) << 23) | (seed & 0x7fffff);
                    let mut cpu = Cpu64::new(BASE, STACK);
                    cpu.mxcsr = mode; // All exceptions unmasked: these instructions never trap.
                    cpu.xmm[0] = [0x5a; 16];
                    for lane in 0..4 {
                        cpu.xmm[1][lane * 4..lane * 4 + 4].copy_from_slice(&bits.to_le_bytes());
                    }
                    cpu.step(&mut mem).unwrap();
                    assert_eq!(cpu.mxcsr, mode);
                    let output = u32::from_le_bytes(cpu.xmm[0][..4].try_into().unwrap());
                    let input = f32::from_bits(bits) as f64;
                    let reference = if opcode == 0x52 {
                        1.0 / input.sqrt()
                    } else {
                        1.0 / input
                    };
                    if reference >= f32::MIN_POSITIVE as f64 {
                        assert!(
                            ((f32::from_bits(output) as f64 - reference) / reference).abs()
                                <= 1.5 / 4096.0
                        );
                    } else {
                        assert_eq!(output, 0);
                    }
                    if scalar {
                        assert_eq!(&cpu.xmm[0][4..], &[0x5a; 12]);
                    }
                }
            }
            for (input, expected) in [
                (0, 0x7f800000),
                (0x80000001, 0xff800000),
                (0x7f800000, 0),
                (0x7f800123, 0x7fc00123),
                (
                    0xbf800000,
                    if opcode == 0x52 {
                        0xffc00000
                    } else {
                        0xbf800000
                    },
                ),
            ] {
                let mut cpu = Cpu64::new(BASE, STACK);
                cpu.xmm[1][..4].copy_from_slice(&u32::to_le_bytes(input));
                cpu.step(&mut mem).unwrap();
                assert_eq!(
                    u32::from_le_bytes(cpu.xmm[0][..4].try_into().unwrap()),
                    expected
                );
                assert_eq!(cpu.mxcsr, 0x1f80);
            }
        }
    }
}

#[test]
fn amd64_packed_floating_point_checks_full_operand_and_aggregates_lane_exceptions() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    for size in [4, 8] {
        for opcode in [0x58, 0x5c, 0x59, 0x5e] {
            let mut code = Vec::new();
            if size == 8 {
                code.push(0x66);
            }
            code.extend([0x0f, opcode, 0xc1]);
            mem.write(BASE, &code).unwrap();
            uc.mem_write(BASE, &code).unwrap();
            for rounding in 0..4 {
                let mut cpu = Cpu64::new(BASE, STACK);
                cpu.mxcsr |= rounding << 13;
                for n in 0..16 / size {
                    let a = if size == 4 {
                        (n as f32 + 2.5).to_bits() as u64
                    } else {
                        (n as f64 + 2.5).to_bits()
                    };
                    let b = if size == 4 {
                        (n as f32 + 1.125).to_bits() as u64
                    } else {
                        (n as f64 + 1.125).to_bits()
                    };
                    cpu.xmm[0][n * size..n * size + size].copy_from_slice(&a.to_le_bytes()[..size]);
                    cpu.xmm[1][n * size..n * size + size].copy_from_slice(&b.to_le_bytes()[..size]);
                }
                uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                uc.reg_write_long(U::XMM1, &cpu.xmm[1]).unwrap();
                uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                cpu.step(&mut mem).unwrap();
                assert_eq!(&*uc.reg_read_long(U::XMM0).unwrap(), &cpu.xmm[0]);
            }
        }
    }
    mem.write(BASE, &[0x0f, 0x5e, 0xc1]).unwrap(); // divps xmm0,xmm1
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.mxcsr = 0; // unmasked invalid and divide-by-zero in different lanes
    cpu.xmm[0][4..8].copy_from_slice(&1.0f32.to_le_bytes());
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("#XM"));
    assert_eq!(cpu.mxcsr & 5, 5);
    assert_eq!(cpu.xmm, before.xmm);
    assert_eq!(cpu.retired, 0);
    mem.write(BASE, &[0x0f, 0x5e, 0x07]).unwrap();
    cpu.regs[7] = BASE + 4096 - 8;
    let status = cpu.mxcsr;
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu.mxcsr, status);
}

#[test]
fn amd64_sse_float_conversions_preserve_or_clear_lanes_and_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    for (prefix, from) in [(None, 4), (Some(0x66), 8), (Some(0xf3), 4), (Some(0xf2), 8)] {
        let mut code = Vec::new();
        code.extend(prefix);
        code.extend([0x0f, 0x5a, 0xc1]);
        mem.write(BASE, &code).unwrap();
        uc.mem_write(BASE, &code).unwrap();
        for rounding in 0..4 {
            for n in 0..7 {
                let bits = match n {
                    0 => 0u64,
                    1 => 1,
                    2 => {
                        if from == 4 {
                            0x3fc00001
                        } else {
                            0x3ff0000000000001
                        }
                    }
                    3 => {
                        if from == 4 {
                            0x7f800001
                        } else {
                            0x7ff0000000000001
                        }
                    }
                    4 => {
                        if from == 4 {
                            0x7fc12345
                        } else {
                            0x7ff81234567890ab
                        }
                    }
                    5 => {
                        if from == 4 {
                            0x7f7fffff
                        } else {
                            0x7fefffffffffffff
                        }
                    }
                    _ => {
                        if from == 4 {
                            0xff800000
                        } else {
                            0xfff0000000000000
                        }
                    }
                };
                let mut cpu = Cpu64::new(BASE, STACK);
                cpu.mxcsr |= rounding << 13;
                cpu.xmm[0] = [0xb7; 16];
                for lane in 0..16 / from {
                    cpu.xmm[1][lane * from..lane * from + from]
                        .copy_from_slice(&bits.to_le_bytes()[..from]);
                }
                uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                uc.reg_write_long(U::XMM1, &cpu.xmm[1]).unwrap();
                uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                cpu.step(&mut mem).unwrap();
                assert_eq!(
                    &*uc.reg_read_long(U::XMM0).unwrap(),
                    &cpu.xmm[0],
                    "prefix={prefix:?} bits={bits:x} round={rounding}"
                );
            }
        }
    }
    // CVTPS2PD reads m64 (not m128), including at a page end.
    mem.write(BASE, &[0x0f, 0x5a, 0x07]).unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.regs[7] = BASE + 4096 - 8;
    mem.write(cpu.regs[7], &[0; 8]).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.xmm[0], [0; 16]);
}

#[test]
fn win64_system_layout_and_host_calendar_have_complete_checked_outputs() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let pointer = STACK + 0x100;
    for name in ["GetSystemInfo", "GetNativeSystemInfo"] {
        startup_call(&mut mem, &mut cpu, &mut runtime, name, &[pointer]);
        assert_eq!(mem.read_int(pointer, 2).unwrap(), 9);
        assert_eq!(mem.read_int(pointer + 4, 4).unwrap(), 4096);
        assert_eq!(mem.read_int(pointer + 8, 8).unwrap(), 0x10000);
        assert_eq!(mem.read_int(pointer + 24, 8).unwrap(), 1);
        assert_eq!(mem.read_int(pointer + 32, 4).unwrap(), 1);
        assert_eq!(mem.read_int(pointer + 40, 4).unwrap(), 65536);
    }
    for name in ["GetSystemTime", "GetLocalTime"] {
        startup_call(&mut mem, &mut cpu, &mut runtime, name, &[pointer]);
        assert!((2020..2100).contains(&mem.read_int(pointer, 2).unwrap()));
        assert!((1..=12).contains(&mem.read_int(pointer + 2, 2).unwrap()));
        assert!(mem.read_int(pointer + 4, 2).unwrap() <= 6);
        assert!((1..=31).contains(&mem.read_int(pointer + 6, 2).unwrap()));
        assert!(mem.read_int(pointer + 8, 2).unwrap() <= 23);
        assert!(mem.read_int(pointer + 10, 2).unwrap() <= 59);
    }
}

#[test]
fn win64_calendar_roundtrips_epoch_leap_centuries_and_rejects_invalid_dates() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let source = STACK + 0x100;
    let ticks = STACK + 0x200;
    let result = STACK + 0x300;
    for (fields, expected) in [
        ([1601u16, 1, 0, 1, 0, 0, 0, 0], Some(0)),
        ([1970, 1, 0, 1, 0, 0, 0, 0], Some(116444736000000000)),
        ([2000, 1, 0, 1, 0, 0, 0, 0], Some(125911584000000000)),
        ([2000, 2, 0, 29, 23, 59, 59, 999], None),
        ([30827, 12, 0, 31, 23, 59, 59, 999], None),
    ] {
        let bytes: Vec<u8> = fields.into_iter().flat_map(u16::to_le_bytes).collect();
        mem.write(source, &bytes).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SystemTimeToFileTime",
                &[source, ticks]
            ),
            1
        );
        if let Some(expected) = expected {
            assert_eq!(mem.read_int(ticks, 8).unwrap(), expected);
        }
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "FileTimeToSystemTime",
                &[ticks, result]
            ),
            1
        );
        for n in [0, 1, 3, 4, 5, 6, 7] {
            assert_eq!(
                mem.read_int(result + n * 2, 2).unwrap(),
                fields[n as usize] as u64
            );
        }
        assert!(mem.read_int(result + 4, 2).unwrap() <= 6);
    }
    for fields in [
        [1900u16, 2, 0, 29, 0, 0, 0, 0],
        [2000, 2, 0, 30, 0, 0, 0, 0],
        [1600, 1, 0, 1, 0, 0, 0, 0],
        [2000, 1, 0, 1, 24, 0, 0, 0],
    ] {
        mem.write(
            source,
            &fields
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        mem.write_int(ticks, 8, 0x11223344).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SystemTimeToFileTime",
                &[source, ticks]
            ),
            0
        );
        assert_eq!(mem.read_int(ticks, 8).unwrap(), 0x11223344);
    }
    mem.write_int(ticks, 8, 1 << 63).unwrap();
    mem.write(result, &[0x77; 16]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "FileTimeToSystemTime",
            &[ticks, result]
        ),
        0
    );
    let mut bytes = [0; 16];
    mem.read(result, &mut bytes).unwrap();
    assert_eq!(bytes, [0x77; 16]);
}

#[test]
fn win64_timezone_conversion_obeys_dst_boundaries_gaps_and_checked_aliasing() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let zone = STACK + 0x100;
    let input = STACK + 0x300;
    let output = STACK + 0x400;
    fn systemtime(fields: [u16; 8]) -> Vec<u8> {
        fields.into_iter().flat_map(u16::to_le_bytes).collect()
    }
    fn read_fields(mem: &Memory, address: u64) -> [u16; 8] {
        std::array::from_fn(|n| mem.read_int(address + n as u64 * 2, 2).unwrap() as u16)
    }
    // Query the actual host zone, then check that the supplied Windows structure
    // reproduces default-zone conversion at the current instant.
    let mode = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetTimeZoneInformation",
        &[zone],
    );
    assert!(mode <= 2);
    startup_call(&mut mem, &mut cpu, &mut runtime, "GetSystemTime", &[input]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SystemTimeToTzSpecificLocalTime",
            &[0, input, output]
        ),
        1
    );
    let host = read_fields(&mem, output);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SystemTimeToTzSpecificLocalTime",
            &[zone, input, output]
        ),
        1
    );
    assert_eq!(read_fields(&mem, output), host);
    // US Eastern: second Sunday in March at 02:00, first Sunday in November at 02:00.
    let mut tz = [0; 172];
    put32(&mut tz, 0, 300);
    put32(&mut tz, 168, (-60i32) as u32);
    tz[68..84].copy_from_slice(&systemtime([0, 11, 0, 1, 2, 0, 0, 0]));
    tz[152..168].copy_from_slice(&systemtime([0, 3, 0, 2, 2, 0, 0, 0]));
    mem.write(zone, &tz).unwrap();
    for (utc, local) in [
        (
            [2024, 3, 0, 10, 6, 59, 59, 999],
            [2024, 3, 0, 10, 1, 59, 59, 999],
        ),
        ([2024, 3, 0, 10, 7, 0, 0, 0], [2024, 3, 0, 10, 3, 0, 0, 0]),
        (
            [2024, 11, 0, 3, 5, 59, 59, 0],
            [2024, 11, 0, 3, 1, 59, 59, 0],
        ),
        ([2024, 11, 0, 3, 6, 0, 0, 0], [2024, 11, 0, 3, 1, 0, 0, 0]),
    ] {
        mem.write(input, &systemtime(utc)).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SystemTimeToTzSpecificLocalTime",
                &[zone, input, output]
            ),
            1
        );
        let result = read_fields(&mem, output);
        for n in [0, 1, 3, 4, 5, 6, 7] {
            assert_eq!(result[n], local[n]);
        }
    }
    // Resolve overlap to standard time and reject nonexistent spring-forward time.
    for (local, expected) in [
        (
            [2024, 11, 0, 3, 1, 30, 0, 123],
            Some([2024, 11, 0, 3, 6, 30, 0, 123]),
        ),
        ([2024, 3, 0, 10, 2, 30, 0, 0], None),
    ] {
        mem.write(input, &systemtime(local)).unwrap();
        mem.write(output, &[0xa5; 16]).unwrap();
        let result = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TzSpecificLocalTimeToSystemTime",
            &[zone, input, output],
        );
        if let Some(expected) = expected {
            assert_eq!(result, 1);
            let fields = read_fields(&mem, output);
            for n in [0, 1, 3, 4, 5, 6, 7] {
                assert_eq!(fields[n], expected[n]);
            }
        } else {
            assert_eq!(result, 0);
            assert_eq!(read_fields(&mem, output), [0xa5a5; 8]);
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
                87
            );
        }
    }
    // Southern hemisphere crosses January and uses the last Sunday of a short month.
    put32(&mut tz, 0, (-600i32) as u32);
    tz[68..84].copy_from_slice(&systemtime([0, 2, 0, 5, 3, 0, 0, 0]));
    tz[152..168].copy_from_slice(&systemtime([0, 10, 0, 1, 2, 0, 0, 0]));
    mem.write(zone, &tz).unwrap();
    for (utc, local) in [
        ([2024, 1, 0, 1, 0, 0, 0, 0], [2024, 1, 0, 1, 11, 0, 0, 0]),
        (
            [2024, 2, 0, 24, 15, 59, 59, 0],
            [2024, 2, 0, 25, 2, 59, 59, 0],
        ),
        ([2024, 2, 0, 24, 16, 0, 0, 0], [2024, 2, 0, 25, 2, 0, 0, 0]),
        ([2024, 7, 0, 1, 0, 0, 0, 0], [2024, 7, 0, 1, 10, 0, 0, 0]),
    ] {
        mem.write(input, &systemtime(utc)).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SystemTimeToTzSpecificLocalTime",
                &[zone, input, input]
            ),
            1
        ); // aliases input
        let result = read_fields(&mem, input);
        for n in [0, 1, 3, 4, 5, 6, 7] {
            assert_eq!(result[n], local[n]);
        }
    }
    // A fixed zone ignores standard/daylight extra biases and preserves a year crossing.
    tz[68..84].fill(0);
    tz[152..168].fill(0);
    put32(&mut tz, 0, (-480i32) as u32);
    put32(&mut tz, 84, 1234);
    mem.write(zone, &tz).unwrap();
    mem.write(input, &systemtime([2023, 12, 0, 31, 20, 0, 0, 456]))
        .unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SystemTimeToTzSpecificLocalTime",
            &[zone, input, output]
        ),
        1
    );
    assert_eq!(read_fields(&mem, output), [2024, 1, 1, 1, 4, 0, 0, 456]);
    // A null timezone consults host tzdata; an ordinary midday value roundtrips.
    let fields = [2024, 7, 0, 1, 12, 13, 14, 789];
    mem.write(input, &systemtime(fields)).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SystemTimeToTzSpecificLocalTime",
            &[0, input, output]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TzSpecificLocalTimeToSystemTime",
            &[0, output, input]
        ),
        1
    );
    let result = read_fields(&mem, input);
    for n in [0, 1, 3, 4, 5, 6, 7] {
        assert_eq!(result[n], fields[n]);
    }
    cpu.regs[1] = zone;
    cpu.regs[2] = input;
    cpu.regs[8] = STACK + 8192 - 8;
    mem.write_int(cpu.regs[8], 8, 0xa5a5a5a5a5a5a5a5).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch(
            "kernel32.dll",
            "SystemTimeToTzSpecificLocalTime",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(mem.read_int(cpu.regs[8], 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
}

#[test]
fn amd64_vmm_tracks_reserve_commit_image_guard_and_allocation_ownership() {
    use wire::mem::{MEM_COMMIT, MEM_FREE, MEM_IMAGE, MEM_PRIVATE, MEM_RESERVE};
    let mut mem = Memory::default();
    mem.map_image(BASE, 8192, READ | EXECUTE).unwrap();
    let info = mem.virtual_query(BASE + 3).unwrap();
    assert_eq!(
        u32::from_le_bytes(info[40..44].try_into().unwrap()),
        MEM_IMAGE
    );
    assert!(mem.virtual_free(BASE, 0, 0x8000).is_err());
    let base = mem
        .virtual_alloc(0x400000003, 8192, MEM_RESERVE, 4)
        .unwrap();
    assert_eq!(base, 0x400000000);
    assert!(mem.read_int(base, 1).is_err());
    let info = mem.virtual_query(base).unwrap();
    assert_eq!(
        u32::from_le_bytes(info[32..36].try_into().unwrap()),
        MEM_RESERVE
    );
    assert_eq!(
        u32::from_le_bytes(info[40..44].try_into().unwrap()),
        MEM_PRIVATE
    );
    assert_eq!(u64::from_le_bytes(info[24..32].try_into().unwrap()), 12288);
    assert!(mem.map(base, 4096, READ).is_err());
    assert_eq!(
        mem.virtual_alloc(base + 4099, 1, MEM_COMMIT, 4).unwrap(),
        base + 4096
    );
    mem.write_int(base + 4096, 8, 0x12345678).unwrap();
    // Recommit preserves both payload and previous protection.
    mem.virtual_alloc(base + 4096, 4096, MEM_COMMIT, 2).unwrap();
    mem.write_int(base + 4096 + 8, 8, 99).unwrap();
    assert_eq!(mem.read_int(base + 4096, 8).unwrap(), 0x12345678);
    assert!(mem.virtual_protect(base, 8192, 2).is_err());
    assert_eq!(mem.virtual_protect(base + 4096, 4096, 0x104).unwrap(), 4);
    let info = mem.virtual_query(base + 4096).unwrap(); // Query does not consume guard.
    assert_eq!(u32::from_le_bytes(info[36..40].try_into().unwrap()), 0x104);
    assert!(mem.read_int(base + 4096, 1).unwrap_err().contains("guard"));
    assert_eq!(mem.read_int(base + 4096, 8).unwrap(), 0x12345678);
    assert_eq!(mem.virtual_protect(base + 4096, 4096, 2).unwrap(), 4);
    assert!(mem.write_int(base + 4096, 8, 0).is_err());
    mem.virtual_free(base + 4096, 4096, 0x4000).unwrap();
    mem.virtual_alloc(base + 4096, 4096, MEM_COMMIT, 4).unwrap();
    assert_eq!(mem.read_int(base + 4096, 8).unwrap(), 0);
    assert!(mem.virtual_free(base + 4096, 0, 0x8000).is_err());
    mem.virtual_free(base, 0, 0x8000).unwrap();
    let info = mem.virtual_query(base).unwrap();
    assert_eq!(
        u32::from_le_bytes(info[32..36].try_into().unwrap()),
        MEM_FREE
    );
    let top = mem.virtual_alloc(0, 4096, 0x103000, 0x20).unwrap();
    assert!(top > STACK);
    assert!(mem.read_int(top, 1).is_ok());
    assert!(mem.fetch(top, &mut [0]).is_ok());
    assert!(mem.write_int(top, 1, 0).is_err());
    mem.virtual_free(top, 0, 0x8000).unwrap();
}

#[test]
fn win64_vmm_gateway_obeys_size_t_and_prechecks_outputs_before_protection() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let base = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "VirtualAlloc",
        &[0, 8192, 0x3000, 4],
    );
    assert_ne!(base, 0);
    let pointer = STACK + 0x100;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualQuery",
            &[base, pointer, 48]
        ),
        48
    );
    assert_eq!(mem.read_int(pointer, 8).unwrap(), base);
    assert_eq!(mem.read_int(pointer + 24, 8).unwrap(), 8192);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualQueryEx",
            &[u64::MAX, BASE, pointer, 47]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualAllocEx",
            &[0x1234, 0, 4096, 0x3000, 4]
        ),
        0
    );
    cpu.regs[1] = base;
    cpu.regs[2] = 4096;
    cpu.regs[8] = 2;
    cpu.regs[9] = 0;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "VirtualProtect", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(cpu.regs, before.regs);
    mem.write_int(base, 8, 123).unwrap(); // Protection did not change on bad output.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualProtect",
            &[base, 4096, 2, base]
        ),
        1
    );
    assert_eq!(mem.read_int(base, 4).unwrap(), 4); // Output may alias a page becoming read-only.
    assert!(mem.write_int(base, 4, 0).is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualFree",
            &[base, 0, 0x8000]
        ),
        1
    );
    assert!(mem.read_int(base, 1).is_err());
}

#[test]
fn win64_events_share_named_state_consume_auto_reset_and_validate_wait_access() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let name = STACK + 0x100;
    mem.write(
        name,
        &"Local\\wire-event\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let auto = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 0, 1, name],
    );
    let alias = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 1, 0, name],
    );
    assert_ne!(auto, alias);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        183
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[alias, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[auto, 0]
        ),
        258
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[auto]),
        1
    );
    let manual = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 1, 0, 0],
    );
    let array = STACK + 0x200;
    mem.write_int(array, 8, auto).unwrap();
    mem.write_int(array + 8, 8, manual).unwrap();
    // A failed wait-all must not consume the already signaled auto-reset event.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, array, 1, 0]
        ),
        258
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[manual]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, array, 1, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[auto, 0]
        ),
        258
    );
    for _ in 0..2 {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[manual, 0]
            ),
            0
        );
    }
    let read = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "OpenEventW",
        &[0x100000, 0, name],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[read]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[auto]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[alias]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[read, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[read]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "OpenEventW",
            &[0x100000, 0, name]
        ),
        0
    );
    cpu.regs[1] = manual;
    cpu.regs[2] = u32::MAX as u64;
    startup_call(&mut mem, &mut cpu, &mut runtime, "ResetEvent", &[manual]);
    cpu.regs[1] = manual;
    cpu.regs[2] = u32::MAX as u64;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "WaitForSingleObject", &mut cpu, &mut mem)
        .unwrap_err()
        .contains("scheduler"));
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
}

#[test]
fn win64_semaphores_count_wait_atomically_and_share_event_namespace() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let name = STACK + 0x100;
    let output = STACK + 0x200;
    let array = STACK + 0x300;
    mem.write(
        name,
        &"wire-semaphore\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let sem = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateSemaphoreExW",
        &[0, 1, 2, name, 0, 0x1f0003],
    );
    assert_ne!(sem, 0);
    let alias = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateSemaphoreW",
        &[0, 0, 1, name],
    );
    assert_ne!(alias, 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        183
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateEventW",
            &[0, 0, 0, name]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
    let event = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 0, 0, 0],
    );
    mem.write_int(array, 8, sem).unwrap();
    mem.write_int(array + 8, 8, event).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, array, 1, 0]
        ),
        258
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[event]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, array, 1, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[alias, 0]
        ),
        258
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[alias, 2, output]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0);
    mem.write_int(output, 4, 0xfeed).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[alias, 1, output]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        298
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0xfeed);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[alias, u32::MAX as u64, output]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        87
    );
    let read = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "OpenSemaphoreW",
        &[0x100000, 0, name],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[read, 1, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[read, 0]
        ),
        0
    );
    // Bad previous-count pointer must not increment the live object.
    cpu.regs[1] = sem;
    cpu.regs[2] = 1;
    cpu.regs[8] = 2;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "ReleaseSemaphore", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[sem, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[sem, 0]
        ),
        258
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[sem]);
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[read, 1, 0]
        ),
        0
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[read]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "OpenSemaphoreW",
            &[0x100000, 0, name]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateSemaphoreW",
            &[0, 3, 2, 0]
        ),
        0
    );
}

#[test]
fn win64_anonymous_pipes_transfer_bytes_preserve_data_on_fault_and_report_broken_endpoint() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let pointers = STACK + 0x100;
    let input = STACK + 0x200;
    let output = STACK + 0x300;
    let count = STACK + 0x400;
    let attributes = STACK + 0x500;
    mem.write(attributes, &[0; 24]).unwrap();
    mem.write_int(attributes, 4, 24).unwrap();
    mem.write_int(attributes + 16, 4, 1).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreatePipe",
            &[pointers, pointers + 8, attributes, 0]
        ),
        1
    );
    let reader = mem.read_int(pointers, 8).unwrap();
    let writer = mem.read_int(pointers + 8, 8).unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[reader]),
        3
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[reader, count]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[reader, 1, 0]
        ),
        1
    );
    mem.write(input, b"real-stream").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[writer, input, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 11);
    // A bad result pointer must not consume any queued bytes.
    cpu.regs[1] = reader;
    cpu.regs[2] = output;
    cpu.regs[8] = 11;
    cpu.regs[9] = 0;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    mem.write_int(cpu.regs[4] + 40, 8, 0).unwrap();
    assert!(runtime
        .dispatch("kernel32.dll", "ReadFile", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, output, 4, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 4);
    let mut bytes = [0; 4];
    mem.read(output, &mut bytes).unwrap();
    assert_eq!(&bytes, b"real");
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[writer]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, output, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 7);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, output, 11, count, 0]
        ),
        0
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        109
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[reader]),
        1
    );
}

#[test]
fn win64_child_startup_attributes_query_size_and_missing_executable_do_not_fake_process() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let size = STACK + 0x100;
    let list = STACK + 0x200;
    let pointers = STACK + 0x300;
    let attributes = STACK + 0x400;
    let exe = STACK + 0x500;
    let startup_info = STACK + 0x600;
    let process_info = STACK + 0x700;
    mem.write_int(size, 8, 0).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitializeProcThreadAttributeList",
            &[0, 1, 0, size]
        ),
        0
    );
    assert_eq!(mem.read_int(size, 8).unwrap(), 64);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitializeProcThreadAttributeList",
            &[list, 1, 0, size]
        ),
        1
    );
    mem.write(attributes, &[0; 24]).unwrap();
    mem.write_int(attributes, 4, 24).unwrap();
    mem.write_int(attributes + 16, 4, 1).unwrap();
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreatePipe",
        &[pointers, pointers + 8, attributes, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "UpdateProcThreadAttribute",
            &[list, 0, 0x20002, pointers, 8, 0, 0]
        ),
        1
    );
    let root = std::env::temp_dir().join(format!("wire64-missing-child-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    mem.write(startup_info, &[0; 104]).unwrap();
    mem.write_int(startup_info, 4, 104).unwrap();
    mem.write(process_info, &[0x55; 24]).unwrap();
    mem.write(
        exe,
        &"C:\\missing.exe\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateProcessW",
            &[exe, 0, 0, 0, 1, 0, 0, 0, startup_info, process_info]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        2
    );
    let mut unchanged = [0; 24];
    mem.read(process_info, &mut unchanged).unwrap();
    assert_eq!(unchanged, [0x55; 24]);
    mem.write(
        exe,
        &"\"C:\\missing.exe\" --attach\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateProcessW",
            &[0, exe, 0, 0, 1, 0, 0, 0, startup_info, process_info]
        ),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "DeleteProcThreadAttributeList",
        &[list],
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_winrt_initialization_balances_apartment_refs_and_rejects_mode_changes() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let call = |name: &str,
                arguments: &[u64],
                mem: &mut Memory,
                cpu: &mut Cpu64,
                runtime: &mut wire::x64::runtime::Runtime64| {
        for (n, value) in arguments.iter().enumerate() {
            cpu.regs[[1, 2, 8, 9][n]] = *value;
        }
        mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
        runtime
            .dispatch("api-ms-win-core-winrt-l1-1-0.dll", name, cpu, mem)
            .unwrap();
        cpu.regs[0]
    };
    assert_eq!(
        call("RoInitialize", &[0], &mut mem, &mut cpu, &mut runtime),
        0
    );
    assert_eq!(
        call("RoInitialize", &[0], &mut mem, &mut cpu, &mut runtime),
        1
    );
    assert_eq!(
        call("RoInitialize", &[1], &mut mem, &mut cpu, &mut runtime),
        0x80010106
    );
    call("RoUninitialize", &[], &mut mem, &mut cpu, &mut runtime);
    assert_eq!(
        call("CoInitializeEx", &[0, 2], &mut mem, &mut cpu, &mut runtime),
        1
    );
    call("RoUninitialize", &[], &mut mem, &mut cpu, &mut runtime);
    assert_eq!(
        call("CoInitializeEx", &[0, 0], &mut mem, &mut cpu, &mut runtime),
        0x80010106
    );
    call("CoUninitialize", &[], &mut mem, &mut cpu, &mut runtime);
    assert_eq!(
        call("RoInitialize", &[1], &mut mem, &mut cpu, &mut runtime),
        0
    );
    assert_eq!(
        call("RoInitialize", &[2], &mut mem, &mut cpu, &mut runtime),
        0x80070057
    );
}

#[test]
fn win64_shell_argv_uses_high_pointers_shell_quoting_and_one_local_allocation() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let input = STACK + 0x100;
    let count = STACK + 0x500;
    for (command, expected) in [
        (
            "\"C:\\日本語 game.exe\" \"a b\" empty\"\" \\\"x",
            vec!["C:\\日本語 game.exe", "a b", "empty", "\"x"],
        ),
        ("  game.exe arg", vec!["", "game.exe", "arg"]),
        ("game.exe \"\" tail\\", vec!["game.exe", "", "tail\\"]),
    ] {
        mem.write(
            input,
            &command
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cpu.regs[1] = input;
        cpu.regs[2] = count;
        mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
        runtime
            .dispatch("shell32.dll", "CommandLineToArgvW", &mut cpu, &mut mem)
            .unwrap();
        let argv = cpu.regs[0];
        assert!(argv > u32::MAX as u64);
        assert_eq!(mem.read_int(count, 4).unwrap(), expected.len() as u64);
        for (n, expected) in expected.iter().enumerate() {
            let pointer = mem.read_int(argv + n as u64 * 8, 8).unwrap();
            let units: Vec<u16> = expected.encode_utf16().chain(Some(0)).collect();
            for (i, unit) in units.iter().enumerate() {
                assert_eq!(
                    mem.read_int(pointer + i as u64 * 2, 2).unwrap(),
                    *unit as u64
                );
            }
        }
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "LocalFree", &[argv]),
            0
        );
        assert!(mem.read_int(argv, 8).is_err());
    }
}

#[test]
fn win64_filesystem_queries_read_real_metadata_and_keep_cwd_confined() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-files-{}", std::process::id()));
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/test.dat"), b"real-data").unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let path = STACK + 0x100;
    let output = STACK + 0x200;
    mem.write(path, b"C:\\sub\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesA",
            &[path]
        ) & 0x10,
        0x10
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetCurrentDirectoryA",
            &[path]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetCurrentDirectoryA",
            &[0, 0]
        ),
        7
    );
    mem.write(path, b"test.dat\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesExA",
            &[path, 0, output]
        ),
        1
    );
    assert_eq!(mem.read_int(output + 32, 4).unwrap(), 9);
    mem.write(path, b"..\\..\\outside\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetCurrentDirectoryA",
            &[path]
        ),
        0
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn win64_readonly_file_sections_keep_views_alive_and_coherent_with_guest_file_writes() {
    use std::io::{Seek, SeekFrom, Write as _};
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-section-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut file = std::fs::File::create(root.join("section.bin")).unwrap();
    let data: Vec<_> = (0..73729).map(|n| (n % 251) as u8).collect();
    file.write_all(&data).unwrap();
    file.seek(SeekFrom::Start(0x100010000)).unwrap();
    file.write_all(b"high-file-offset").unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let name = STACK + 0x100;
    let out = STACK + 0x300;
    mem.write(name, b"C:\\section.bin\0").unwrap();
    let source = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0xc0000000, 7, 0, 3, 0x80, 0],
    );
    assert_ne!(source, u64::MAX);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetFilePointerEx",
        &[source, 1234, 0, 0],
    );
    let section = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileMappingW",
        &[source, 0, 2, 0, 0, 0],
    );
    assert_ne!(section, 0);
    const ADDRESS: u64 = 0x550000000;
    let view = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MapViewOfFileEx",
        &[section, 4, 0, 0, data.len() as u64, ADDRESS],
    );
    assert_eq!(view, ADDRESS);
    let tail = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MapViewOfFile",
        &[section, 4, 0, 65536, 16],
    );
    assert_ne!(tail, 0);
    let high = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MapViewOfFile",
        &[section, 4, 1, 65536, 16],
    );
    let mut high_data = [0; 16];
    mem.read(high, &mut high_data).unwrap();
    assert_eq!(&high_data, b"high-file-offset");
    let mut mapped = vec![0; data.len()];
    mem.read(view, &mut mapped).unwrap();
    assert_eq!(mapped, data);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "ReadFile",
        &[source, out, 1, out + 16, 0],
    );
    assert_eq!(mem.read_int(out, 1).unwrap(), data[1234] as u64); // Mapping never seeks the file handle.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualQuery",
            &[view, out, 48]
        ),
        48
    );
    assert_eq!(mem.read_int(out + 8, 8).unwrap(), view);
    assert_eq!(mem.read_int(out + 36, 4).unwrap(), 2);
    assert_eq!(
        mem.read_int(out + 40, 4).unwrap(),
        wire::mem::MEM_MAPPED as u64
    );
    assert!(mem.write_int(view + 4094, 8, 0).is_err());
    mem.write_int(out, 4, 0xa5a5a5a5).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualProtect",
            &[view, 4096, 4, out]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        5
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 0xa5a5a5a5);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualFree",
            &[view, 0, 0x8000]
        ),
        0
    );
    for (access, offset_hi, offset_lo, size, expected) in [
        (2, 0, 0, 16, 50), // writable view is an explicit unsupported boundary
        (4, 0, 1, 16, 1132),
        (4, 1, 65536, 17, 87),
        (4, 0, 0, 0, 8), // whole >4GiB view exceeds checked memory capacity
    ] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "MapViewOfFile",
                &[section, access, offset_hi, offset_lo, size]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            expected
        );
    }
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MapViewOfFileEx",
            &[section, 4, 0, 0, 16, ADDRESS]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        487
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, section, u64::MAX, out, 0, 1, 2]
        ),
        1
    );
    let alias = mem.read_int(out, 8).unwrap();
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetHandleInformation",
        &[alias, out],
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 1);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetHandleInformation",
        &[alias, 2, 2],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetHandleInformation",
        &[alias, 2, 0],
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[section]);
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[source]);
    let retained = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MapViewOfFile",
        &[alias, 4, 0, 0, 16],
    );
    assert_ne!(retained, 0); // descriptor and section survive source closure
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]);
    let writer = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0x40000000, 7, 0, 3, 0x80, 0],
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetFilePointerEx",
        &[writer, 65532, 0, 0],
    );
    mem.write(out, b"abcdefghijkl").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[writer, out, 12, out + 16, 0]
        ),
        1
    );
    let mut changed = [0; 12];
    mem.read(view + 65532, &mut changed).unwrap();
    assert_eq!(&changed, b"abcdefghijkl");
    assert_eq!(
        mem.read_int(tail, 8).unwrap(),
        u64::from_le_bytes(*b"efghijkl")
    );
    // Backing updates must also preserve PAGE_NOACCESS, until the guest restores READ.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualProtect",
            &[tail, 4096, 1, out]
        ),
        1
    );
    mem.write(out, b"ABCDEFGH").unwrap();
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetFilePointerEx",
        &[writer, 65536, 0, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[writer, out, 8, out + 16, 0]
        ),
        1
    );
    assert!(mem.read_int(tail, 8).is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualProtect",
            &[tail, 4096, 2, out]
        ),
        1
    );
    assert_eq!(
        mem.read_int(tail, 8).unwrap(),
        u64::from_le_bytes(*b"ABCDEFGH")
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateFileA",
            &[name, 0x40000000, 7, 0, 2, 0x80, 0]
        ),
        u64::MAX
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1224
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "FlushViewOfFile",
            &[view + 65536, 16]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "UnmapViewOfFile",
            &[view + 4096]
        ),
        0
    );
    for base in [view, tail, high, retained] {
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "UnmapViewOfFile", &[base]),
            1
        );
        assert!(mem.read_int(base, 1).is_err());
    }
    let truncated = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0x40000000, 7, 0, 2, 0x80, 0],
    );
    assert_ne!(truncated, u64::MAX);
    assert_eq!(file.metadata().unwrap().len(), 0);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CloseHandle",
        &[truncated],
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[writer]);
    drop(file);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn win64_handle_metadata_preserves_large_sizes_identity_times_and_checked_outputs() {
    use std::os::unix::fs::MetadataExt;
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-handle-info-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("large.bin");
    let file = std::fs::File::create(&path).unwrap();
    let length = 0x100000009;
    file.set_len(length).unwrap();
    std::fs::hard_link(&path, root.join("alias.bin")).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let name = STACK + 0x100;
    let out = STACK + 0x200;
    mem.write(name, b"C:\\large.bin\0").unwrap();
    let handle = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0x80000000, 7, 0, 3, 0x80, 0],
    );
    assert_ne!(handle, u64::MAX);
    // Rename after open: metadata must come from the live descriptor.
    std::fs::rename(&path, root.join("renamed.bin")).unwrap();
    let meta = file.metadata().unwrap();
    mem.write(out, &[0xa5; 60]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandle",
            &[handle, out]
        ),
        1
    );
    assert_eq!(mem.read_int(out + 28, 4).unwrap(), meta.dev() as u32 as u64);
    assert_eq!(mem.read_int(out + 32, 4).unwrap(), 1);
    assert_eq!(mem.read_int(out + 36, 4).unwrap(), 9);
    assert_eq!(mem.read_int(out + 40, 4).unwrap(), 2);
    let index = mem.read_int(out + 44, 4).unwrap() << 32 | mem.read_int(out + 48, 4).unwrap();
    assert_eq!(index, meta.ino());
    assert_eq!(mem.read_int(out + 52, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
    let times = [4, 12, 20].map(|offset| mem.read_int(out + offset, 8).unwrap());
    assert!(times.iter().all(|&t| t > 116444736000000000));
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileTime",
            &[handle, out, 0, out + 16]
        ),
        1
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), times[0]);
    assert_eq!(mem.read_int(out + 16, 8).unwrap(), times[2]);
    mem.write(out, &[0xa5; 60]).unwrap();
    cpu.regs[1] = handle;
    cpu.regs[2] = out;
    cpu.regs[8] = BASE + 4096; // invalid second output, valid first and third
    cpu.regs[9] = out + 16;
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "GetFileTime", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
    for (class, size) in [(0, 40), (1, 24), (9, 8), (18, 24)] {
        mem.write(out, &[0xa5; 60]).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetFileInformationByHandleEx",
                &[handle, class, out, size - 1]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            24
        );
        assert_eq!(mem.read_int(out, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetFileInformationByHandleEx",
                &[handle, class, out, size]
            ),
            1
        );
        assert_eq!(mem.read_int(out + size, 4).unwrap(), 0xa5a5a5a5);
        match class {
            0 => {
                assert_eq!(mem.read_int(out, 8).unwrap(), times[0]);
                assert_eq!(mem.read_int(out + 16, 8).unwrap(), times[2]);
                assert_ne!(mem.read_int(out + 24, 8).unwrap(), 0);
                assert_eq!(mem.read_int(out + 32, 4).unwrap(), 0x80);
            }
            1 => {
                assert_eq!(mem.read_int(out, 8).unwrap(), meta.blocks() * 512);
                assert_eq!(mem.read_int(out + 8, 8).unwrap(), length);
                assert_eq!(mem.read_int(out + 16, 4).unwrap(), 2);
                assert_eq!(mem.read_int(out + 20, 4).unwrap(), 0);
            }
            9 => assert_eq!(mem.read_int(out, 8).unwrap(), 0x80),
            18 => {
                assert_eq!(mem.read_int(out, 8).unwrap(), meta.dev());
                assert_eq!(mem.read_int(out + 8, 8).unwrap(), meta.ino());
                assert_eq!(mem.read_int(out + 16, 8).unwrap(), 0);
            }
            _ => unreachable!(),
        }
    }
    mem.write(name, b"C:\\alias.bin\0").unwrap();
    let alias = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0x80000000, 7, 0, 3, 0x80, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandle",
            &[alias, out]
        ),
        1
    );
    assert_eq!(
        mem.read_int(out + 44, 4).unwrap() << 32 | mem.read_int(out + 48, 4).unwrap(),
        index
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandleEx",
            &[handle, 2, out, 60]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[handle]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandle",
            &[handle, out]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]);
    drop(file);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_file_io_is_real_checks_sharing_and_preflights_before_advancing_offsets() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-io-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let path = STACK + 0x100;
    let bytes = STACK + 0x200;
    let count = STACK + 0x300;
    mem.write(path, b"C:\\data.bin\0").unwrap();
    mem.write(bytes, b"actual-file").unwrap();
    let writer = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[path, 0x40000000, 0, 0, 2, 0x80, 0],
    );
    assert_ne!(writer, u64::MAX);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[writer, bytes, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 11);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateFileA",
            &[path, 0x80000000, 3, 0, 3, 0x80, 0]
        ),
        u64::MAX
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        32
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[writer]),
        1
    );
    assert_eq!(
        std::fs::read(root.join("data.bin")).unwrap(),
        b"actual-file"
    );
    let reader = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[path, 0x120089, 3, 0, 3, 0x80, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileSizeEx",
            &[reader, count]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 8).unwrap(), 11);
    cpu.regs[1] = reader;
    cpu.regs[2] = bytes;
    cpu.regs[8] = 11;
    cpu.regs[9] = 0;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    mem.write_int(cpu.regs[4] + 40, 8, 0).unwrap();
    assert!(runtime
        .dispatch("kernel32.dll", "ReadFile", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, bytes, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 11);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, bytes, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetFilePointerEx",
            &[reader, (-4i64) as u64, count, 2]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 8).unwrap(), 7);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, reader, u64::MAX, count, 0, 1, 2]
        ),
        1
    );
    let duplicate = mem.read_int(count, 8).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[duplicate, count]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[duplicate, bytes, 4, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 4);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, bytes, 4, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 0); // Shared underlying file offset.
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[reader]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileSizeEx",
            &[duplicate, count]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 8).unwrap(), 11);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CloseHandle",
        &[duplicate],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, 0x101, u64::MAX, count, 0, 0, 2]
        ),
        1
    );
    let duplicate = mem.read_int(count, 8).unwrap();
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[0x101]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileType",
            &[duplicate]
        ),
        3
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[duplicate, bytes, 0, count, 0]
        ),
        1
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CloseHandle",
        &[duplicate],
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_memory_status_reports_actual_guest_commit_limit_and_reservations() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let pointer = STACK + 0x100;
    mem.write_int(pointer, 4, 64).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GlobalMemoryStatusEx",
            &[pointer]
        ),
        1
    );
    let before = mem.read_int(pointer + 16, 8).unwrap();
    assert_eq!(mem.read_int(pointer + 8, 8).unwrap(), 512 * 1024 * 1024);
    let allocation = mem.virtual_alloc(0, 8192, 0x3000, 4).unwrap();
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GlobalMemoryStatusEx",
        &[pointer],
    );
    assert_eq!(mem.read_int(pointer + 16, 8).unwrap(), before - 8192);
    mem.virtual_free(allocation, 0, 0x8000).unwrap();
    mem.write_int(pointer, 4, 63).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GlobalMemoryStatusEx",
            &[pointer]
        ),
        0
    );
}

#[test]
fn win64_environment_is_guest_owned_case_insensitive_and_length_queries_do_not_write() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let name = STACK + 0x100;
    let value = STACK + 0x200;
    let output = STACK + 0x300;
    mem.write(name, b"Wire_Test\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetEnvironmentVariableA",
            &[name, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        203
    );
    mem.write(value, b"guest-value\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetEnvironmentVariableA",
            &[name, value]
        ),
        1
    );
    mem.write(name, b"wire_test\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetEnvironmentVariableA",
            &[name, 0, 0]
        ),
        12
    );
    mem.write(output, &[0x55; 12]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetEnvironmentVariableA",
            &[name, output, 11]
        ),
        12
    );
    assert_eq!(mem.read_int(output, 1).unwrap(), 0x55);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetEnvironmentVariableA",
            &[name, output, 12]
        ),
        11
    );
    let block = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetEnvironmentStringsW",
        &[],
    );
    assert_eq!(mem.read_int(block, 2).unwrap(), b'W' as u64);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "FreeEnvironmentStringsW",
            &[block]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetEnvironmentVariableA",
            &[name, 0]
        ),
        1
    );
}

#[test]
fn win64_bcrypt_uses_os_entropy_checks_output_and_tracks_rng_provider_lifetime() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let name = STACK + 0x100;
    let handle = STACK + 0x200;
    let output = STACK + 0x300;
    mem.write(
        name,
        &"RNG\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let call = |function: &str,
                args: &[u64],
                mem: &mut Memory,
                cpu: &mut Cpu64,
                runtime: &mut wire::x64::runtime::Runtime64| {
        for (n, value) in args.iter().enumerate() {
            cpu.regs[[1, 2, 8, 9][n]] = *value;
        }
        mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
        runtime.dispatch("bcrypt.dll", function, cpu, mem).unwrap();
        cpu.regs[0]
    };
    assert_eq!(
        call(
            "BCryptOpenAlgorithmProvider",
            &[handle, name, 0, 0],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0
    );
    let rng = mem.read_int(handle, 8).unwrap();
    mem.write(output - 1, &[0x55; 514]).unwrap();
    assert_eq!(
        call(
            "BCryptGenRandom",
            &[rng, output, 512, 0],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0
    );
    assert_eq!(mem.read_int(output - 1, 1).unwrap(), 0x55);
    assert_eq!(mem.read_int(output + 512, 1).unwrap(), 0x55);
    assert_eq!(
        call(
            "BCryptCloseAlgorithmProvider",
            &[rng, 0],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0
    );
    assert_eq!(
        call(
            "BCryptGenRandom",
            &[rng, output, 1, 0],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0xc0000008
    );
    assert_eq!(
        call(
            "BCryptGenRandom",
            &[0, output, 256, 2],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0
    );
    cpu.regs[1] = 0;
    cpu.regs[2] = STACK + 8192 - 16;
    cpu.regs[8] = 32;
    cpu.regs[9] = 2;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch("bcrypt.dll", "BCryptGenRandom", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
}

#[test]
fn win64_processor_topology_is_consistent_with_single_guest_cpu_and_x64_record_layouts() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x100;
    let size = STACK + 0x300;
    mem.write_int(size, 4, 0).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLogicalProcessorInformationEx",
            &[0xffff, 0, size]
        ),
        0
    );
    assert_eq!(mem.read_int(size, 4).unwrap(), 224);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLogicalProcessorInformationEx",
            &[0xffff, output, size]
        ),
        1
    );
    let mut at = output;
    for (kind, length) in [(0, 48), (1, 48), (3, 48), (4, 80)] {
        assert_eq!(mem.read_int(at, 4).unwrap(), kind);
        assert_eq!(mem.read_int(at + 4, 4).unwrap(), length);
        assert_eq!(
            mem.read_int(at + if kind == 4 { 72 } else { 32 }, 8)
                .unwrap(),
            1
        );
        at += length;
    }
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetActiveProcessorCount",
            &[0xffff]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetActiveProcessorCount",
            &[1]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLogicalProcessorInformationEx",
            &[2, output, size]
        ),
        1
    );
    assert_eq!(mem.read_int(size, 4).unwrap(), 0);
}

#[test]
fn amd64_cmpxchg_pairs_match_unicorn_and_fault_before_committing_any_state() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    for wide in [false, true] {
        let mut code = vec![0xf0];
        if wide {
            code.push(0x48);
        }
        code.extend([0x0f, 0xc7, 0x0f]);
        let size = if wide { 16 } else { 8 };
        let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        uc.mem_map(BASE, 8192, Prot::ALL).unwrap();
        uc.mem_write(BASE, &code).unwrap();
        let mut mem = Memory::default();
        mem.map(BASE, 8192, READ | WRITE | EXECUTE).unwrap();
        mem.write(BASE, &code).unwrap();
        for matched in [false, true] {
            let mut cpu = Cpu64::new(BASE, STACK);
            cpu.regs[0] = 0xaabbccdd12345678;
            cpu.regs[2] = 0x5566778899aabbcc;
            cpu.regs[3] = 0x1122334455667788;
            cpu.regs[1] = 0xffeeccbbaa998877;
            cpu.regs[7] = BASE + 4096;
            let bits = if matched {
                if wide {
                    (cpu.regs[2] as u128) << 64 | cpu.regs[0] as u128
                } else {
                    (cpu.regs[2] as u32 as u128) << 32 | cpu.regs[0] as u32 as u128
                }
            } else {
                0
            };
            mem.write(cpu.regs[7], &bits.to_le_bytes()[..size]).unwrap();
            uc.mem_write(cpu.regs[7], &bits.to_le_bytes()[..size])
                .unwrap();
            for (register, index) in [
                (U::RAX, 0),
                (U::RCX, 1),
                (U::RDX, 2),
                (U::RBX, 3),
                (U::RDI, 7),
            ] {
                uc.reg_write(register, cpu.regs[index]).unwrap();
            }
            uc.reg_write(U::RFLAGS, cpu.rflags).unwrap();
            uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
            cpu.step(&mut mem).unwrap();
            assert_eq!(uc.reg_read(U::RAX).unwrap(), cpu.regs[0]);
            assert_eq!(uc.reg_read(U::RDX).unwrap(), cpu.regs[2]);
            assert_eq!(uc.reg_read(U::RFLAGS).unwrap() & 0x40, cpu.rflags & 0x40);
            let mut bytes = vec![0; size];
            mem.read(BASE + 4096, &mut bytes).unwrap();
            assert_eq!(uc.mem_read_as_vec(BASE + 4096, size).unwrap(), bytes);
        }
        mem.protect(BASE + 4096, 4096, READ).unwrap();
        let mut cpu = Cpu64::new(BASE, STACK);
        cpu.regs[7] = BASE + 4096;
        let before = cpu.clone();
        assert!(cpu.step(&mut mem).is_err());
        assert_eq!(cpu.regs, before.regs);
        assert_eq!(cpu.rflags, before.rflags);
        assert_eq!(cpu.retired, 0);
        if wide {
            cpu.regs[7] += 1;
            assert!(cpu.step(&mut mem).unwrap_err().contains("#GP"));
        }
    }
}
#[test]
fn amd64_multiply_divide_widths_overflow_and_cpuid_profile_are_checked() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    for bits in [16, 32, 64] {
        for opcode in [
            vec![0x0f, 0xaf, 0xc3],
            vec![0xf7, 0xe3],
            vec![0xf7, 0xf3],
            vec![0xf7, 0xfb],
        ] {
            let mut code = Vec::new();
            if bits == 16 {
                code.push(0x66);
            } else if bits == 64 {
                code.push(0x48);
            }
            code.extend_from_slice(&opcode);
            let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
            uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
            uc.mem_write(BASE, &code).unwrap();
            for (a, b, d) in [
                (900, 21, 0),
                (u64::MAX, 3, 0),
                (0, 0, 0),
                (u64::MAX, u64::MAX, u64::MAX),
            ] {
                let mut mem = Memory::default();
                mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
                mem.write(BASE, &code).unwrap();
                let mut cpu = Cpu64::new(BASE, STACK);
                cpu.regs[0] = a;
                cpu.regs[3] = b;
                cpu.regs[2] = d;
                cpu.rflags = 0x202;
                for (register, value) in [
                    (U::RAX, a),
                    (U::RBX, b),
                    (U::RDX, d),
                    (U::RFLAGS, cpu.rflags),
                ] {
                    uc.reg_write(register, value).unwrap();
                }
                let expected = uc.emu_start(BASE, BASE + code.len() as u64, 0, 1);
                let before = cpu.clone();
                let result = cpu.step(&mut mem);
                assert_eq!(
                    result.is_ok(),
                    expected.is_ok(),
                    "bits={bits} opcode={opcode:x?} a={a:x} b={b:x}"
                );
                if result.is_ok() {
                    for (register, index) in [(U::RAX, 0), (U::RBX, 3), (U::RDX, 2)] {
                        assert_eq!(uc.reg_read(register).unwrap(), cpu.regs[index]);
                    }
                    if opcode[1] != 0xf3 && opcode[1] != 0xfb {
                        assert_eq!(uc.reg_read(U::RFLAGS).unwrap() & 0x801, cpu.rflags & 0x801);
                    }
                } else {
                    assert_eq!(cpu.regs, before.regs);
                    assert_eq!(cpu.rflags, before.rflags);
                    assert_eq!(cpu.retired, 0);
                }
            }
        }
    }
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.write(BASE, &[0x0f, 0xa2]).unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.regs[0] = 1;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[1], 0);
    assert_eq!(cpu.regs[2], 0); // No AVX, OSXSAVE or SSE advertisement.
}
#[test]
fn win64_codepage_stack_arguments_and_explicit_stdio_profile() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let text = "日本語\0";
    let encoded = wire::codepage::encode(text, 932).unwrap().0;
    let source = STACK + 0x100;
    let dest = STACK + 0x200;
    mem.write(source, &encoded).unwrap();
    let count = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MultiByteToWideChar",
        &[932, 0, source, u32::MAX as u64, 0, 0xff7fff7e00000000],
    );
    assert_eq!(count, 4);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MultiByteToWideChar",
            &[932, 0, source, u32::MAX as u64, dest, count]
        ),
        count
    );
    let mut bytes = [0; 8];
    mem.read(dest, &mut bytes).unwrap();
    assert_eq!(
        bytes.to_vec(),
        text.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    let converted = STACK + 0x300;
    let used = STACK + 0x400;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WideCharToMultiByte",
            &[932, 0, dest, count, converted, 32, 0, used]
        ),
        encoded.len() as u64
    );
    assert_eq!(mem.read_int(used, 4).unwrap(), 0);
    let mut output = vec![0; encoded.len()];
    mem.read(converted, &mut output).unwrap();
    assert_eq!(output, encoded);
    let handle = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetStdHandle",
        &[0xfffffff5],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[handle]),
        3
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetConsoleMode",
            &[handle, used]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[handle]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[handle]),
        0
    );
}
#[test]
fn amd64_static_tls_and_delay_iat_are_high_address_runtime_data() {
    let mut b = fixture();
    let opt = 0x98;
    put32(&mut b, opt + 112 + 9 * 8, 0x1100);
    put32(&mut b, opt + 116 + 9 * 8, 40);
    put64(&mut b, 0x300, BASE + 0x11c0);
    put64(&mut b, 0x308, BASE + 0x11c4);
    put64(&mut b, 0x310, BASE + 0x1180);
    put64(&mut b, 0x318, BASE + 0x1190);
    put32(&mut b, 0x320, 12);
    put32(&mut b, 0x324, 5 << 20);
    put64(&mut b, 0x390, BASE + 0x1220);
    put32(&mut b, 0x3c0, 0x12345678);
    b[0x420] = 0xc3;
    let pe = Pe64::parse(&b).unwrap();
    assert!(pe.load(&b, BASE, |_, _| Ok(0)).is_err());
    let mut mem = pe.load_with_runtime(&b, BASE, |_, _| Ok(0)).unwrap();
    mem.map(STACK, 8192, READ | WRITE).unwrap();
    let mut runtime =
        wire::x64::runtime::Runtime64::new(&mut mem, BASE, STACK, 8192, "probe").unwrap();
    let tls = pe.tls(&mem, BASE).unwrap().unwrap();
    assert_eq!(tls.callbacks, vec![BASE + 0x1220]);
    runtime.initialize_tls(&mut mem, &tls).unwrap();
    let vector = mem.read_int(wire::x64::runtime::TEB + 0x58, 8).unwrap();
    let ptr = mem.read_int(vector, 8).unwrap();
    assert!(ptr > u32::MAX as u64);
    assert_eq!(mem.read_int(ptr, 4).unwrap(), 0x12345678);
    assert_eq!(mem.read_int(ptr + 4, 8).unwrap(), 0);
    assert_eq!(mem.read_int(BASE + 0x1180, 4).unwrap(), 0);
    let mut b = fixture();
    put32(&mut b, opt + 112 + 13 * 8, 0x1100);
    put32(&mut b, opt + 116 + 13 * 8, 64);
    for (offset, value) in [(0, 1), (4, 0x1180), (8, 0x11a0), (12, 0x11b0), (16, 0x11d0)] {
        put32(&mut b, 0x300 + offset, value);
    }
    b[0x380..0x38d].copy_from_slice(b"kernel32.dll\0");
    put64(&mut b, 0x3d0, 0x11e0);
    b[0x3e2..0x3f6].copy_from_slice(b"GetCurrentProcessId\0");
    let pe = Pe64::parse(&b).unwrap();
    let delays = pe.delay_imports(&b).unwrap();
    assert_eq!(delays.len(), 1);
    let mut mem = pe.load_with_runtime(&b, BASE, |_, _| Ok(0)).unwrap();
    mem.map(STACK, 8192, READ | WRITE).unwrap();
    let rsp = STACK + 0x1fc8;
    mem.write_int(rsp, 8, BASE).unwrap();
    let mut cpu = Cpu64::new(BASE, rsp);
    let mut runtime =
        wire::x64::runtime::Runtime64::new(&mut mem, BASE, STACK, 8192, "probe").unwrap();
    runtime.install_gateways(Default::default());
    runtime.install_delays(&mut mem, BASE, &delays).unwrap();
    let gateway = mem.read_int(BASE + 0x11b0, 8).unwrap();
    assert!(gateway > u32::MAX as u64);
    assert_eq!(mem.read_int(BASE + 0x11a0, 8).unwrap(), 0);
    cpu.rip = gateway;
    runtime
        .dispatch("kernel32.dll", "GetCurrentProcessId", &mut cpu, &mut mem)
        .unwrap();
    assert_eq!(cpu.regs[0], 1);
    assert_ne!(mem.read_int(BASE + 0x11a0, 8).unwrap(), 0);
    put32(&mut b, 0x300, 0);
    assert!(pe.delay_imports(&b).is_err());
}
#[test]
fn amd64_dll_cli_runs_static_tls_callback_before_real_attach_entry() {
    use std::process::Command;
    let mut b = fixture();
    put16(&mut b, 0x96, 0x2002);
    put32(&mut b, 0x98 + 112 + 9 * 8, 0x1100);
    put32(&mut b, 0x98 + 116 + 9 * 8, 40);
    put64(&mut b, 0x300, BASE + 0x11c0);
    put64(&mut b, 0x308, BASE + 0x11c4);
    put64(&mut b, 0x310, BASE + 0x1180);
    put64(&mut b, 0x318, BASE + 0x1190);
    put64(&mut b, 0x390, BASE + 0x1220);
    put32(&mut b, 0x3c0, 0x12345678);
    let load = [
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00,
    ];
    b[0x200..0x20c].copy_from_slice(&load);
    b[0x20c..0x20f].copy_from_slice(&[0x8b, 0x00, 0xc3]);
    b[0x420..0x42c].copy_from_slice(&load);
    b[0x42c..0x42f].copy_from_slice(&[0xff, 0x00, 0xc3]);
    let path = std::env::temp_dir().join(format!("wire-dll64-cli-{}.dll", std::process::id()));
    std::fs::write(&path, b).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wire"))
        .args(["probe64", path.to_str().unwrap(), "--dll"])
        .output()
        .unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("TLS process attach: 0x140001220"));
    assert!(report.contains("entry returned RAX=0x12345679; retired=8"));
}

#[test]
fn amd64_process_binds_guest_dll_export_and_attaches_before_readonly_exe_iat_call() {
    use wire::x64::process::{Outcome, Process64};
    let mut child = fixture();
    put16(&mut child, 0x96, 0x2002);
    put32(&mut child, 0x98 + 112, 0x1100);
    put32(&mut child, 0x98 + 116, 80);
    for (offset, value) in [
        (16, 1),
        (20, 1),
        (24, 1),
        (28, 0x1180),
        (32, 0x1190),
        (36, 0x11a0),
    ] {
        put32(&mut child, 0x300 + offset, value);
    }
    put32(&mut child, 0x380, 0x1200);
    put32(&mut child, 0x390, 0x11b0);
    child[0x3b0..0x3b6].copy_from_slice(b"Value\0");
    // Both files prefer BASE; this valid ABS-only relocation table allows moving the RIP-relative child.
    put32(&mut child, 0x98 + 112 + 5 * 8, 0x11e0);
    put32(&mut child, 0x98 + 116 + 5 * 8, 12);
    put32(&mut child, 0x3e0, 0x1000);
    put32(&mut child, 0x3e4, 12);
    child[0x200..0x20d].copy_from_slice(&[0xc6, 0x05, 0xf9, 0x02, 0, 0, 1, 0xb8, 1, 0, 0, 0, 0xc3]);
    child[0x400..0x408].copy_from_slice(&[0x0f, 0xb6, 0x05, 0xf9, 0, 0, 0, 0xc3]);
    let mut main = fixture();
    put32(&mut main, 0x98 + 120, 0x1140);
    put32(&mut main, 0x98 + 124, 40);
    put32(&mut main, 0x340, 0x1180);
    put32(&mut main, 0x34c, 0x11c0);
    put32(&mut main, 0x350, 0x1190);
    put64(&mut main, 0x380, 0x11d0);
    main[0x3c0..0x3ca].copy_from_slice(b"child.dll\0");
    main[0x3d2..0x3d8].copy_from_slice(b"Value\0");
    main[0x200..0x20f].copy_from_slice(&[
        0x48, 0x83, 0xec, 0x28, 0xff, 0x15, 0x86, 1, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0xc3,
    ]);
    put32(&mut main, 0x98 + 0xf0 + 36, 0x60000020);
    let dir = std::env::temp_dir().join(format!("wire-linked64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.exe"), main).unwrap();
    std::fs::write(dir.join("child.dll"), child).unwrap();
    let mut process = Process64::load(&dir.join("main.exe"), false).unwrap();
    let child_base = process
        .modules
        .iter()
        .find(|(name, _)| name == "child.dll")
        .unwrap()
        .1;
    assert_ne!(child_base, BASE);
    assert!(child_base > u32::MAX as u64);
    assert_eq!(
        process.memory.read_int(BASE + 0x1190, 8).unwrap(),
        child_base + 0x1200
    );
    assert!(process.memory.check(BASE + 0x1190, 8, WRITE).is_err());
    assert_eq!(process.run(100).unwrap(), Outcome::Returned(1));
    assert_eq!(
        process
            .stages
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["child.dll entry", "main.exe entry"]
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn amd64_dynamic_load_executes_tls_and_entry_once_and_initializes_existing_thread_tls() {
    use wire::x64::process::{Outcome, Process64};
    const CHILD: u64 = BASE + 0x1000000;
    const CODE: u64 = 0x510000000;
    const WORKER: u64 = CODE + 4096;
    const DATA: u64 = CODE + 8192;
    fn mov(code: &mut Vec<u8>, reg: u8, value: u64) {
        code.extend([0x48 | (reg >> 3), 0xb8 | (reg & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[u64]) {
        for (n, &value) in args.iter().enumerate() {
            mov(code, if n < 4 { [1, 2, 8, 9][n] } else { 0 }, value);
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        mov(code, 0, p.runtime.register("kernel32.dll", name));
        code.extend([0xff, 0xd0]);
    }
    fn store(code: &mut Vec<u8>, address: u64) {
        mov(code, 7, address);
        code.extend([0x48, 0x89, 0x07]);
    }
    let dir = std::env::temp_dir().join(format!("wire-dynamic64-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("engine")).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    let mut child = fixture();
    put16(&mut child, 0x96, 0x2002);
    put64(&mut child, 0x98 + 24, CHILD);
    put32(&mut child, 0x98 + 112, 0x1100);
    put32(&mut child, 0x98 + 116, 64);
    for (offset, value) in [
        (16, 1),
        (20, 1),
        (24, 1),
        (28, 0x1184),
        (32, 0x1188),
        (36, 0x118c),
    ] {
        put32(&mut child, 0x300 + offset, value);
    }
    put32(&mut child, 0x384, 0x1200);
    put32(&mut child, 0x388, 0x11b0);
    child[0x3b0..0x3b6].copy_from_slice(b"Value\0");
    put32(&mut child, 0x98 + 112 + 9 * 8, 0x1140);
    put32(&mut child, 0x98 + 116 + 9 * 8, 40);
    put64(&mut child, 0x340, CHILD + 0x11c0);
    put64(&mut child, 0x348, CHILD + 0x11c4);
    put64(&mut child, 0x350, CHILD + 0x1180);
    put64(&mut child, 0x358, CHILD + 0x1190);
    put32(&mut child, 0x360, 4);
    put64(&mut child, 0x390, CHILD + 0x1220);
    put32(&mut child, 0x3c0, 0x1234);
    let tls_load = [
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00,
    ];
    let mut entry = vec![0x83, 0xfa, 1, 0x75, 15]; // Only process attach mutates the loading thread.
    entry.extend(tls_load);
    entry.extend([0x83, 0x00, 0x10, 0xb8, 1, 0, 0, 0, 0xc3]);
    child[0x200..0x200 + entry.len()].copy_from_slice(&entry);
    let mut value = tls_load.to_vec();
    value.extend([0x8b, 0x00, 0xc3]);
    child[0x400..0x400 + value.len()].copy_from_slice(&value);
    let mut callback = tls_load.to_vec();
    callback.extend([0xff, 0x00, 0xc3]);
    child[0x420..0x420 + callback.len()].copy_from_slice(&callback);
    std::fs::write(dir.join("engine/child.dll"), &child).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory
        .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    let path: Vec<_> = "C:\\engine\\child.dll"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    p.memory.write(DATA + 0x100, &path).unwrap();
    p.memory.write(DATA + 0x180, b"Value\0").unwrap();
    let mut code = vec![0x48, 0x83, 0xec, 0x38];
    call(
        &mut p,
        &mut code,
        "CreateThread",
        &[0, 0, WORKER, DATA, 4, 0],
    );
    code.extend([0x49, 0x89, 0xc4]); // r12 = suspended thread
    call(&mut p, &mut code, "SwitchToThread", &[]);
    store(&mut code, DATA + 32);
    call(&mut p, &mut code, "LoadLibraryW", &[DATA + 0x100]);
    store(&mut code, DATA + 8);
    call(&mut p, &mut code, "GetProcAddress", &[CHILD, DATA + 0x180]);
    code.extend([0xff, 0xd0]);
    store(&mut code, DATA + 16);
    call(&mut p, &mut code, "LoadLibraryW", &[DATA + 0x100]);
    store(&mut code, DATA + 24);
    code.extend([0x4c, 0x89, 0xe1]); // rcx = r12
    call(&mut p, &mut code, "ResumeThread", &[]);
    call(&mut p, &mut code, "SwitchToThread", &[]);
    store(&mut code, DATA + 40);
    code.extend([0x4c, 0x89, 0xe1]);
    mov(&mut code, 2, u32::MAX as u64);
    call(&mut p, &mut code, "WaitForSingleObject", &[]);
    code.extend([0xb8, 1, 0, 0, 0, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    p.memory.write(CODE, &code).unwrap();
    let mut worker = tls_load.to_vec();
    worker.extend([0x8b, 0x00]);
    store(&mut worker, DATA);
    worker.push(0xc3);
    p.memory.write(WORKER, &worker).unwrap();
    p.cpu.rip = CODE;
    let initial_rsp = p.cpu.regs[4];
    loop {
        let budget = p.cpu.work + 1;
        match p.run(budget).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 10000),
            result => {
                assert_eq!(result, Outcome::Returned(1));
                break;
            }
        }
    }
    assert_eq!(p.cpu.regs[4], initial_rsp + 8);
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0x1235);
    assert_eq!(p.memory.read_int(DATA + 8, 8).unwrap(), CHILD);
    assert_eq!(p.memory.read_int(DATA + 16, 8).unwrap(), 0x1245);
    assert_eq!(p.memory.read_int(DATA + 24, 8).unwrap(), CHILD);
    assert_eq!(p.memory.read_int(DATA + 32, 8).unwrap(), 0);
    assert_eq!(p.memory.read_int(DATA + 40, 8).unwrap(), 1);
    assert_eq!(
        p.stages
            .iter()
            .filter(|(s, _)| s == "child.dll dynamic entry")
            .count(),
        1
    );
    assert_eq!(
        p.stages
            .iter()
            .filter(|(s, _)| s == "child.dll TLS thread 2")
            .count(),
        1
    );
    assert_eq!(
        p.modules.iter().filter(|(s, _)| s == "child.dll").count(),
        1
    );
    // A FALSE DllMain is an explicit unsupported rollback boundary, never successful loading.
    child[0x200..0x203].copy_from_slice(&[0x31, 0xc0, 0xc3]);
    std::fs::write(dir.join("engine/child.dll"), &child).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory
        .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    p.memory.write(DATA + 0x100, &path).unwrap();
    let mut code = vec![0x48, 0x83, 0xec, 0x28];
    call(&mut p, &mut code, "LoadLibraryW", &[DATA + 0x100]);
    code.extend([0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(CODE, &code).unwrap();
    p.cpu.rip = CODE;
    assert!(p
        .run(1000)
        .unwrap_err()
        .contains("DLL_PROCESS_ATTACH returned FALSE"));
    assert!(!p
        .runtime
        .trace
        .iter()
        .any(|line| line.contains("LoadLibraryW")));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn amd64_guest_threads_run_callbacks_and_isolate_tls_fls_teb_and_mxcsr() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x500000000;
    const WORKER: u64 = CODE + 4096;
    const DATA: u64 = CODE + 8192;
    const DESTRUCTOR: u64 = CODE + 12288;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn store(code: &mut Vec<u8>, address: u64) {
        mov(code, 7, address);
        code.extend([0x48, 0x89, 0x07]);
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[(u64, bool)]) {
        for (n, &(value, indirect)) in args.iter().enumerate() {
            let reg = if n < 4 { [1, 2, 8, 9][n] } else { 0 };
            mov(code, reg, value);
            if indirect {
                // Load through RAX without disturbing the other argument registers.
                mov(code, 0, value);
                code.extend([0x48, 0x8b, 0x00]);
                if reg != 0 {
                    code.extend([0x48 | (reg >> 3), 0x89, 0xc0 | (reg & 7)]);
                }
            }
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        mov(code, 0, p.runtime.register("kernel32.dll", name));
        code.extend([0xff, 0xd0]);
    }
    let mut b = fixture();
    put32(&mut b, 0x98 + 112 + 9 * 8, 0x1100);
    put32(&mut b, 0x98 + 116 + 9 * 8, 40);
    put64(&mut b, 0x300, BASE + 0x11c0);
    put64(&mut b, 0x308, BASE + 0x11c4);
    put64(&mut b, 0x310, BASE + 0x1180);
    put64(&mut b, 0x318, BASE + 0x1190);
    put64(&mut b, 0x390, BASE + 0x1220);
    put32(&mut b, 0x3c0, 0x1234);
    let mut callback = vec![
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00, 0xff, 0x00,
    ];
    mov(&mut callback, 0, DATA + 112);
    callback.extend([0xff, 0x00, 0xc3]);
    b[0x420..0x420 + callback.len()].copy_from_slice(&callback);
    let mut jump = Vec::new();
    mov(&mut jump, 0, CODE);
    jump.extend([0xff, 0xe0]);
    b[0x200..0x200 + jump.len()].copy_from_slice(&jump);
    let path = std::env::temp_dir().join(format!("wire-thread64-{}.exe", std::process::id()));
    std::fs::write(&path, b).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    p.memory
        .map(CODE, 4 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    let mut main = vec![0x48, 0x83, 0xec, 0x38];
    call(
        &mut p,
        &mut main,
        "CreateSemaphoreW",
        &[(0, false), (0, false), (1, false), (0, false)],
    );
    store(&mut main, DATA);
    call(&mut p, &mut main, "TlsAlloc", &[]);
    assert_eq!(p.memory.read_int(BASE + 0x1180, 4).unwrap(), 0);
    call(
        &mut p,
        &mut main,
        "TlsSetValue",
        &[(0, false), (0x99, false)],
    );
    call(&mut p, &mut main, "FlsAlloc", &[(DESTRUCTOR, false)]);
    call(
        &mut p,
        &mut main,
        "FlsSetValue",
        &[(0, false), (0x77, false)],
    );
    main.extend([
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00, 0xc7, 0x00, 0x11, 0x11, 0, 0,
    ]);
    mov(&mut main, 7, DATA + 120);
    main.extend([0x0f, 0xae, 0x17]); // LDMXCSR [RDI]
    mov(&mut main, 7, DATA + 160);
    main.extend([0xd9, 0x2f]);
    call(&mut p, &mut main, "SetLastError", &[(123, false)]);
    call(
        &mut p,
        &mut main,
        "CreateThread",
        &[
            (0, false),
            (0, false),
            (WORKER, false),
            (DATA, false),
            (4, false),
            (DATA + 24, false),
        ],
    );
    store(&mut main, DATA + 16);
    call(
        &mut p,
        &mut main,
        "GetExitCodeThread",
        &[(DATA + 16, true), (DATA + 32, false)],
    );
    call(
        &mut p,
        &mut main,
        "WaitForSingleObject",
        &[(DATA + 16, true), (0, false)],
    );
    store(&mut main, DATA + 40);
    call(&mut p, &mut main, "ResumeThread", &[(DATA + 16, true)]);
    store(&mut main, DATA + 48);
    call(
        &mut p,
        &mut main,
        "WaitForSingleObject",
        &[(DATA, true), (u32::MAX as u64, false)],
    );
    call(
        &mut p,
        &mut main,
        "WaitForSingleObject",
        &[(DATA + 16, true), (u32::MAX as u64, false)],
    );
    call(
        &mut p,
        &mut main,
        "GetExitCodeThread",
        &[(DATA + 16, true), (DATA + 56, false)],
    );
    call(&mut p, &mut main, "GetLastError", &[]);
    store(&mut main, DATA + 64);
    call(&mut p, &mut main, "TlsGetValue", &[(0, false)]);
    store(&mut main, DATA + 72);
    call(&mut p, &mut main, "FlsGetValue", &[(0, false)]);
    store(&mut main, DATA + 80);
    main.extend([0x48, 0x83, 0xc4, 0x38, 0xb8, 1, 0, 0, 0, 0xc3]);
    let mut worker = vec![0x48, 0x83, 0xec, 0x38, 0x48, 0x89, 0xc8];
    store(&mut worker, DATA + 88); // Exact lpParameter.
    mov(&mut worker, 7, DATA + 164);
    worker.extend([0xd9, 0x3f]);
    mov(&mut worker, 7, DATA + 168);
    worker.extend([0xd9, 0x2f]);
    worker.extend([0x65, 0x48, 0x8b, 0x04, 0x25, 0x30, 0, 0, 0]);
    store(&mut worker, DATA + 96);
    worker.extend([
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00, 0x8b, 0x00,
    ]);
    store(&mut worker, DATA + 104);
    call(&mut p, &mut worker, "TlsGetValue", &[(0, false)]);
    store(&mut worker, DATA + 128);
    call(&mut p, &mut worker, "FlsGetValue", &[(0, false)]);
    store(&mut worker, DATA + 136);
    call(&mut p, &mut worker, "SetLastError", &[(42, false)]);
    call(
        &mut p,
        &mut worker,
        "TlsSetValue",
        &[(0, false), (0x11, false)],
    );
    call(
        &mut p,
        &mut worker,
        "FlsSetValue",
        &[(0, false), (0x88, false)],
    );
    mov(&mut worker, 7, DATA + 124);
    worker.extend([0x0f, 0xae, 0x17]);
    call(
        &mut p,
        &mut worker,
        "ReleaseSemaphore",
        &[(DATA, true), (1, false), (0, false)],
    );
    worker.extend([0x48, 0x83, 0xc4, 0x38, 0xb8, 77, 0, 0, 0, 0xc3]);
    let mut destructor = vec![0x48, 0x89, 0xc8];
    store(&mut destructor, DATA + 144);
    mov(&mut destructor, 7, DATA + 172);
    destructor.extend([0xd9, 0x3f]);
    destructor.push(0xc3);
    p.memory.write(CODE, &main).unwrap();
    p.memory.write(WORKER, &worker).unwrap();
    p.memory.write(DESTRUCTOR, &destructor).unwrap();
    p.memory.write_int(DATA + 120, 4, 0x3f80).unwrap();
    p.memory.write_int(DATA + 124, 4, 0x5f80).unwrap();
    p.memory.write_int(DATA + 160, 2, 0x0b7f).unwrap();
    p.memory.write_int(DATA + 168, 2, 0x077f).unwrap();
    assert_eq!(p.run(10000).unwrap(), Outcome::Returned(1));
    assert_eq!(p.memory.read_int(DATA + 24, 4).unwrap(), 2);
    assert_eq!(p.memory.read_int(DATA + 32, 4).unwrap(), 259);
    assert_eq!(p.memory.read_int(DATA + 40, 8).unwrap(), 258);
    assert_eq!(p.memory.read_int(DATA + 48, 8).unwrap(), 1);
    assert_eq!(p.memory.read_int(DATA + 56, 4).unwrap(), 77);
    assert_eq!(p.memory.read_int(DATA + 64, 8).unwrap(), 123);
    assert_eq!(p.memory.read_int(DATA + 72, 8).unwrap(), 0x99);
    assert_eq!(p.memory.read_int(DATA + 80, 8).unwrap(), 0x77);
    assert_eq!(p.memory.read_int(DATA + 88, 8).unwrap(), DATA);
    assert_ne!(
        p.memory.read_int(DATA + 96, 8).unwrap(),
        wire::x64::runtime::TEB
    );
    assert!(p
        .memory
        .read_int(p.memory.read_int(DATA + 96, 8).unwrap(), 1)
        .is_err()); // Exited TEB released.
    assert_eq!(p.memory.read_int(DATA + 104, 8).unwrap(), 0x1235); // Fresh template + thread attach.
    assert_eq!(p.memory.read_int(DATA + 112, 4).unwrap(), 3); // Process attach, thread attach/detach.
    assert_eq!(p.memory.read_int(DATA + 128, 8).unwrap(), 0);
    assert_eq!(p.memory.read_int(DATA + 136, 8).unwrap(), 0);
    assert_eq!(p.memory.read_int(DATA + 144, 8).unwrap(), 0x88);
    assert_eq!(p.cpu.mxcsr, 0x3f80);
    assert_eq!(p.cpu.fpu.cw, 0x0b7f);
    assert_eq!(p.memory.read_int(DATA + 164, 2).unwrap(), 0x037f);
    assert_eq!(p.memory.read_int(DATA + 172, 2).unwrap(), 0x077f);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amd64_timed_sleep_survives_budget_pause_and_wait_timeout_uses_elapsed_time() {
    use std::time::{Duration, Instant};
    use wire::x64::process::{Outcome, Process64};
    let path = std::env::temp_dir().join(format!("wire-sleep64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    let sleep = p.runtime.register("kernel32.dll", "Sleep");
    let mut code = vec![0x48, 0x83, 0xec, 0x28, 0xb9, 40, 0, 0, 0, 0x48, 0xb8];
    code.extend(sleep.to_le_bytes());
    code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xb8, 1, 0, 0, 0, 0xc3]);
    p.memory.write(BASE + 0x1000, &code).unwrap();
    let start = Instant::now();
    assert_eq!(p.run(5).unwrap(), Outcome::Budget);
    assert_eq!(p.cpu.retired, 4);
    assert_eq!(p.cpu.work, 5);
    assert_eq!(p.run(1000).unwrap(), Outcome::Returned(1));
    assert!(start.elapsed() >= Duration::from_millis(40));
    assert_eq!(p.cpu.retired, 7);
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.enable_scheduler();
    let event = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 0, 0, 0],
    );
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = event;
    cpu.regs[2] = 10;
    let before = cpu.clone();
    assert_eq!(
        runtime
            .dispatch("kernel32.dll", "WaitForSingleObject", &mut cpu, &mut mem)
            .unwrap(),
        wire::x64::runtime::Control::Blocked
    );
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    std::thread::sleep(Duration::from_millis(15));
    assert_eq!(
        runtime
            .dispatch("kernel32.dll", "WaitForSingleObject", &mut cpu, &mut mem)
            .unwrap(),
        wire::x64::runtime::Control::Returned
    );
    assert_eq!(cpu.regs[0], 258);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_toolhelp_snapshots_have_correct_x64_layout_and_checked_cursor_progress() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let out = STACK + 0x100;
    let snap = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateToolhelp32Snapshot",
        &[2, 0],
    );
    assert_ne!(snap, u64::MAX);
    mem.write_int(out, 4, 568).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Process32FirstW",
            &[snap, out]
        ),
        1
    );
    assert_eq!(mem.read_int(out + 8, 4).unwrap(), 1);
    assert_eq!(mem.read_int(out + 16, 8).unwrap(), 0);
    assert_eq!(mem.read_int(out + 28, 4).unwrap(), 1);
    assert_eq!(mem.read_int(out + 36, 4).unwrap(), 8);
    let mut bytes = [0; 20];
    mem.read(out + 44, &mut bytes).unwrap();
    assert_eq!(
        bytes.to_vec(),
        "probe.exe\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Process32NextW",
            &[snap, out]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        18
    );
    // A failed First copy cannot advance or reset a snapshot's iteration cursor.
    let bad = STACK + 8192 - 4;
    mem.write_int(bad, 4, 568).unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = snap;
    cpu.regs[2] = bad;
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "Process32FirstW", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Process32NextW",
            &[snap, out]
        ),
        0
    );
    mem.write_int(out, 4, 28).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Thread32First",
            &[snap, out]
        ),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetHandleInformation",
        &[snap, 2, 2],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[snap]),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetHandleInformation",
        &[snap, 2, 0],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[snap]),
        1
    );
    let snap = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateToolhelp32Snapshot",
        &[4, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Thread32First",
            &[snap, out]
        ),
        1
    );
    assert_eq!(mem.read_int(out + 8, 4).unwrap(), 1);
    assert_eq!(mem.read_int(out + 12, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Thread32Next",
            &[snap, out]
        ),
        0
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[snap]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateToolhelp32Snapshot",
            &[8, 0]
        ),
        u64::MAX
    );
}

#[test]
fn amd64_native_locale_callbacks_execute_stop_and_preserve_frames_across_budgets() {
    use wire::x64::process::{Outcome, Process64};
    let path = std::env::temp_dir().join(format!("wire-callback64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for keep_going in [false, true] {
        let mut p = Process64::load(&path, false).unwrap();
        let base = 0x500000000u64;
        p.memory.map(base, 8192, READ | WRITE | EXECUTE).unwrap();
        let gateway = p.runtime.register("kernel32.dll", "EnumSystemLocalesEx");
        let parameter = 0xaabbccdd12345678u64;
        let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x48, 0xb9];
        code.extend(base.to_le_bytes());
        code.extend([0x31, 0xd2, 0x49, 0xb8]);
        code.extend(parameter.to_le_bytes());
        code.extend([0x45, 0x31, 0xc9, 0x48, 0xb8]);
        code.extend(gateway.to_le_bytes());
        code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(BASE + 0x1000, &code).unwrap();
        let mut callback = vec![0x48, 0xb8];
        callback.extend((base + 4096).to_le_bytes());
        callback.extend([
            0x48,
            0xff,
            0x00,
            0x48,
            0x89,
            0x48,
            8,
            0x4c,
            0x89,
            0x40,
            16,
            0xb8,
            keep_going as u8,
            0,
            0,
            0,
            0xc3,
        ]);
        p.memory.write(base, &callback).unwrap();
        let mut finished = false;
        for budget in 1..100 {
            if p.run(budget).unwrap() == Outcome::Returned(1) {
                finished = true;
                break;
            }
        }
        assert!(finished);
        assert_eq!(
            p.memory.read_int(base + 4096, 8).unwrap(),
            if keep_going { 4 } else { 1 }
        );
        assert_eq!(p.memory.read_int(base + 4112, 8).unwrap(), parameter);
        let temporary = p.memory.read_int(base + 4104, 8).unwrap();
        assert!(p.memory.read_int(temporary, 1).is_err());
        assert_eq!(p.cpu.regs[4], 0x7ff0000fff70); // Original EXE return frame, no callback stack leak.
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_locale_queries_and_calendar_formats_preflight_outputs_and_keep_literals() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let locale = STACK + 0x100;
    let pattern = STACK + 0x200;
    let time = STACK + 0x300;
    let out = STACK + 0x400;
    fn wide(mem: &mut Memory, ptr: u64, text: &str) {
        mem.write(
            ptr,
            &text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    wide(&mut mem, locale, "en-US");
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "LocaleNameToLCID",
            &[locale, 0]
        ),
        0x409
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLocaleInfoEx",
            &[locale, 0x20001004, out, 2]
        ),
        2
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 1252);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLocaleInfoEx",
            &[0, 0x20001004, out, 2]
        ),
        2
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 932);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLocaleInfoEx",
            &[locale, 0x1004, 0, 0]
        ),
        5
    );
    wide(&mut mem, pattern, "yyyy-MM-dd 'year' '' dddd");
    for (n, v) in [2024u16, 2, 0, 29, 99, 99, 99, 99].iter().enumerate() {
        mem.write_int(time + n as u64 * 2, 2, *v as u64).unwrap();
    }
    let count = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetDateFormatEx",
        &[locale, 0, time, pattern, out, 128, 0],
    );
    let mut bytes = vec![0; count as usize * 2];
    mem.read(out, &mut bytes).unwrap();
    assert_eq!(
        bytes,
        "2024-02-29 year ' Thursday\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    // Date ignores input time and the supplied weekday; Time ignores the date.
    for (n, v) in [0u16, 0, 0, 0, 13, 4, 5, 999].iter().enumerate() {
        mem.write_int(time + n as u64 * 2, 2, *v as u64).unwrap();
    }
    wide(&mut mem, pattern, "HH:mm:ss 'M yyyy' tt");
    let count = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetTimeFormatEx",
        &[locale, 0, time, pattern, out, 128],
    );
    let mut bytes = vec![0; count as usize * 2];
    mem.read(out, &mut bytes).unwrap();
    assert_eq!(
        bytes,
        "13:04:05 M yyyy PM\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    mem.write_int(out, 8, 0xfeed).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetTimeFormatEx",
            &[locale, 0, time, pattern, out, 2]
        ),
        0
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xfeed);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        122
    );
    wide(&mut mem, locale, "unknown-XX");
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "IsValidLocaleName",
            &[locale]
        ),
        0
    );
    wide(&mut mem, locale, "FunctionWireDoesNotProvide");
    let kernel = startup_call(&mut mem, &mut cpu, &mut runtime, "GetModuleHandleW", &[0]);
    assert_ne!(kernel, 0); // Main image is a real guest module, not a provider handle.
    mem.write(pattern, b"kernel32.dll\0").unwrap();
    runtime.install_gateways(Default::default());
    let kernel = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleHandleA",
        &[pattern],
    );
    mem.write(pattern, b"FunctionWireDoesNotProvide\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[kernel, pattern]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        127
    );
}

#[test]
fn win64_known_folders_use_confined_real_storage_and_com_allocation_lifetime() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire-known64-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let guid = STACK + 0x100;
    let out = STACK + 0x200;
    let path = STACK + 0x300;
    mem.write(
        guid,
        &[
            0xa4, 0xa1, 0x20, 0xa5, 0x80, 0x17, 0xf6, 0x4f, 0xbd, 0x18, 0x16, 0x73, 0x43, 0xc5,
            0xaf, 0x16,
        ],
    )
    .unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SHGetKnownFolderPath",
            &[guid, 0x8000, 0, out]
        ),
        0
    );
    let pointer = mem.read_int(out, 8).unwrap();
    assert!(root.join("Users/wire/AppData/LocalLow").is_dir());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesW",
            &[pointer]
        ),
        0x10
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetFileAttributesW",
            &[pointer, 0x2010]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesW",
            &[pointer]
        ),
        0x2010
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CoTaskMemFree",
        &[pointer],
    );
    assert!(mem.read_int(pointer, 1).is_err());
    mem.write(guid, &[0; 16]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SHGetKnownFolderPath",
            &[guid, 0, 0, out]
        ),
        0x80070032
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0);
    mem.write(path, b"C:\\..\\escape\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateDirectoryA",
            &[path, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        5
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_srw_ownership_and_init_once_publish_context_without_fake_contention() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let lock = STACK + 0x100;
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "AcquireSRWLockShared",
        &[lock],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryAcquireSRWLockExclusive",
            &[lock]
        ),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "ReleaseSRWLockShared",
        &[lock],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryAcquireSRWLockExclusive",
            &[lock]
        ),
        1
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "ReleaseSRWLockExclusive",
        &[lock],
    );
    assert_eq!(mem.read_int(lock, 8).unwrap(), 0);
    let pending = STACK + 0x200;
    let context = STACK + 0x300;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitOnceBeginInitialize",
            &[lock, 0, pending, context]
        ),
        1
    );
    assert_eq!(mem.read_int(pending, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitOnceComplete",
            &[lock, 0, 0x123456780000]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitOnceBeginInitialize",
            &[lock, 1, pending, context]
        ),
        1
    );
    assert_eq!(mem.read_int(pending, 4).unwrap(), 0);
    assert_eq!(mem.read_int(context, 8).unwrap(), 0x123456780000);
}

#[test]
fn amd64_sse_min_max_preserve_source_nan_signed_zero_and_fault_atomicity() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    for size in [4, 8] {
        let values: Vec<u64> = if size == 4 {
            vec![
                0,
                0x80000000,
                1,
                0x80000001,
                1.0f32.to_bits() as u64,
                (-2.5f32).to_bits() as u64,
                0x7f800000,
                0x7fc12345,
                0x7f812345,
            ]
        } else {
            vec![
                0,
                0x8000000000000000,
                1,
                0x8000000000000001,
                1.0f64.to_bits(),
                (-2.5f64).to_bits(),
                0x7ff0000000000000,
                0x7ff8123456789012,
                0x7ff0123456789012,
            ]
        };
        for packed in [false, true] {
            for opcode in [0x5d, 0x5f] {
                let mut code = Vec::new();
                if !packed {
                    code.push(if size == 4 { 0xf3 } else { 0xf2 });
                } else if size == 8 {
                    code.push(0x66);
                }
                code.extend([0x0f, opcode, 0xc1]);
                mem.write(BASE, &code).unwrap();
                uc.mem_write(BASE, &code).unwrap();
                for &a in &values {
                    for &b in &values {
                        for mode in [0, 0x40, 0x8000, 0x8040] {
                            let mut cpu = Cpu64::new(BASE, STACK);
                            cpu.mxcsr |= mode;
                            cpu.xmm[0] = [0xa5; 16];
                            cpu.xmm[1] = [0x5a; 16];
                            let lanes = if packed { 16 / size } else { 1 };
                            for n in 0..lanes {
                                cpu.xmm[0][n * size..n * size + size]
                                    .copy_from_slice(&a.to_le_bytes()[..size]);
                                cpu.xmm[1][n * size..n * size + size]
                                    .copy_from_slice(&b.to_le_bytes()[..size]);
                            }
                            uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                            uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                            uc.reg_write_long(U::XMM1, &cpu.xmm[1]).unwrap();
                            uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                            cpu.step(&mut mem).unwrap();
                            // Unicorn's MIN/MAX does not model DAZ on subnormals.
                            let denormal = |v: u64| {
                                if size == 4 {
                                    v & 0x7f800000 == 0 && v & 0x7fffff != 0
                                } else {
                                    v & 0x7ff0000000000000 == 0 && v & 0xfffffffffffff != 0
                                }
                            };
                            if mode & 0x40 == 0 || (!denormal(a) && !denormal(b)) {
                                assert_eq!(&*uc.reg_read_long(U::XMM0).unwrap(), &cpu.xmm[0], "size={size} packed={packed} op={opcode:x} a={a:x} b={b:x} mode={mode:x}");
                            }
                        }
                    }
                }
            }
        }
    }
    mem.write(BASE, &[0xf3, 0x0f, 0x5f, 0xc1]).unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.mxcsr &= !(1 << 7);
    cpu.xmm[1][..4].copy_from_slice(&0x7fc12345u32.to_le_bytes());
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("#XM"));
    assert_eq!(cpu.xmm, before.xmm);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(cpu.retired, 0);
    assert_eq!(cpu.mxcsr, before.mxcsr | 1);
    mem.write(BASE, &[0x0f, 0x5f, 0x07]).unwrap();
    cpu.regs[7] = BASE + 4096 - 8;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu, before);
}

#[test]
fn amd64_sse_predicate_masks_and_sign_masks_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    for size in [4, 8] {
        for packed in [false, true] {
            for predicate in 0..8 {
                let mut code = Vec::new();
                if !packed {
                    code.push(if size == 4 { 0xf3 } else { 0xf2 });
                } else if size == 8 {
                    code.push(0x66);
                }
                code.extend([0x0f, 0xc2, 0xc1, predicate]);
                mem.write(BASE, &code).unwrap();
                uc.mem_write(BASE, &code).unwrap();
                for &a in &[-2.5f64, -0.0, 0.0, 1.0, f64::INFINITY, f64::NAN] {
                    for &b in &[-3.0f64, -0.0, 0.0, 1.0, f64::INFINITY, f64::NAN] {
                        let mut cpu = Cpu64::new(BASE, STACK);
                        cpu.xmm[0] = [0xa5; 16];
                        cpu.xmm[1] = [0x5a; 16];
                        for n in 0..if packed { 16 / size } else { 1 } {
                            let av = if size == 4 {
                                (a as f32).to_bits() as u64
                            } else {
                                a.to_bits()
                            };
                            let bv = if size == 4 {
                                (b as f32).to_bits() as u64
                            } else {
                                b.to_bits()
                            };
                            cpu.xmm[0][n * size..n * size + size]
                                .copy_from_slice(&av.to_le_bytes()[..size]);
                            cpu.xmm[1][n * size..n * size + size]
                                .copy_from_slice(&bv.to_le_bytes()[..size]);
                        }
                        uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                        uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                        uc.reg_write_long(U::XMM1, &cpu.xmm[1]).unwrap();
                        uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                        cpu.step(&mut mem).unwrap();
                        assert_eq!(&*uc.reg_read_long(U::XMM0).unwrap(), &cpu.xmm[0]);
                        assert_eq!(cpu.rflags, 0x202);
                        let expected = if (a.is_nan() || b.is_nan())
                            && matches!(predicate & 7, 1 | 2 | 5 | 6)
                        {
                            1
                        } else {
                            0
                        };
                        assert_eq!(cpu.mxcsr & 0x3f, expected);
                    }
                }
            }
        }
    }
    for (code, lane) in [
        (vec![0x0f, 0x50, 0xc1], 4),
        (vec![0x66, 0x0f, 0x50, 0xc1], 8),
        (vec![0x66, 0x0f, 0xd7, 0xc1], 1),
    ] {
        mem.write(BASE, &code).unwrap();
        uc.mem_write(BASE, &code).unwrap();
        for pattern in 0..256u32 {
            let mut cpu = Cpu64::new(BASE, STACK);
            for n in 0..16 {
                cpu.xmm[1][n] = pattern.rotate_left(n as u32) as u8;
            }
            cpu.regs[0] = u64::MAX;
            uc.reg_write(U::RAX, u64::MAX).unwrap();
            uc.reg_write_long(U::XMM1, &cpu.xmm[1]).unwrap();
            uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
            cpu.step(&mut mem).unwrap();
            assert_eq!(cpu.regs[0], uc.reg_read(U::RAX).unwrap());
            assert!(cpu.regs[0] < (1 << (16 / lane)));
        }
    }
    mem.write(BASE, &[0x0f, 0xc2, 0xc1, 1]).unwrap();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.mxcsr &= !(1 << 7);
    cpu.xmm[1][4..8].copy_from_slice(&f32::NAN.to_le_bytes());
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("#XM"));
    assert_eq!(cpu.xmm, before.xmm);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(cpu.retired, 0);
    assert_eq!(cpu.mxcsr, before.mxcsr | 1);
}

#[test]
fn amd64_rotate_through_carry_matches_unicorn_across_widths_and_counts() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    for bits in [8, 16, 32, 64] {
        for opcode in [0xd0, 0xd8] {
            for count in 0..=65u8 {
                let mut code = Vec::new();
                if bits == 16 {
                    code.push(0x66);
                } else if bits == 64 {
                    code.push(0x48);
                }
                code.extend([if bits == 8 { 0xc0 } else { 0xc1 }, opcode, count]);
                mem.write(BASE, &code).unwrap();
                uc.mem_write(BASE, &code).unwrap();
                for a in [0, 1, u64::MAX, 0xfedcba9876543210, 1u64 << (bits - 1)] {
                    for cf in [0, 1] {
                        let mut cpu = Cpu64::new(BASE, STACK);
                        cpu.regs[0] = a;
                        cpu.rflags = 0xa96 | cf;
                        uc.reg_write(U::RAX, a).unwrap();
                        uc.reg_write(U::RFLAGS, cpu.rflags).unwrap();
                        uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                        cpu.step(&mut mem).unwrap();
                        assert_eq!(
                            cpu.regs[0],
                            uc.reg_read(U::RAX).unwrap(),
                            "bits={bits} op={opcode:x} count={count} a={a:x} cf={cf}"
                        );
                        let masked = count & if bits == 64 { 63 } else { 31 };
                        let mask = if masked == 1 || (masked as u32).is_multiple_of(bits + 1) {
                            0xffff
                        } else {
                            0xffff & !0x800
                        };
                        assert_eq!(
                            cpu.rflags & mask,
                            uc.reg_read(U::RFLAGS).unwrap() & mask,
                            "bits={bits} count={count}"
                        );
                    }
                }
            }
        }
    }
}
