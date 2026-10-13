//! Shared original PE64 builder and checked Win64 call harness.
use wire::x64::{
    cpu::Cpu64,
    memory::{Memory, READ, WRITE},
};
pub(crate) const BASE: u64 = 0x140000000;
pub(crate) const STACK: u64 = 0x7ff000000000;

pub(crate) fn put16(b: &mut [u8], at: usize, n: u16) {
    b[at..at + 2].copy_from_slice(&n.to_le_bytes());
}

pub(crate) fn put32(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
}

pub(crate) fn put64(b: &mut [u8], at: usize, n: u64) {
    b[at..at + 8].copy_from_slice(&n.to_le_bytes());
}

pub(crate) fn fixture() -> Vec<u8> {
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

pub(crate) fn startup() -> (Memory, Cpu64, wire::x64::runtime::Runtime64) {
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

pub(crate) fn startup_call(
    mem: &mut Memory,
    cpu: &mut Cpu64,
    runtime: &mut wire::x64::runtime::Runtime64,
    name: &str,
    args: &[u64],
) -> u64 {
    startup_dll_call(mem, cpu, runtime, "KERNEL32.dll", name, args)
}

pub(crate) fn startup_dll_call(
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
