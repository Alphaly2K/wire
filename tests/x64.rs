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
