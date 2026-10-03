//! Differential tests: wire interpreter vs Unicorn (QEMU-derived x86 core).
//!
//! Method: generate an instruction sequence and an initial machine state,
//! execute on both sides, compare final GPRs / EFLAGS / memory. Templates
//! guarantee memory safety by construction (pointer registers always point
//! into the DATA region); when a case faults, both sides must fault at the
//! same EIP to be considered equivalent.
//!
//! Flags the ISA leaves undefined for a given instruction are excluded from
//! comparison per family (e.g. AF after logic ops is kept defined here since
//! our implementation is deterministic — the conservative masks are only for
//! shifts/mul/div/bit-ops).

use unicorn_engine::{Arch, Mode, Prot, RegisterX86, Unicorn};
use wire::cpu::Cpu;
use wire::mem::GuestMem;

const CODE: u32 = 0x1_0000;
const CODE_SIZE: usize = 0x1_0000;
const DATA: u32 = 0x2_0000;
const DATA_SIZE: usize = 0x1_0000;
const STACK: u32 = 0x3_0000;
const STACK_SIZE: usize = 0x1_0000;
const STACK_TOP: u32 = STACK + STACK_SIZE as u32 - 16;

const REGNAME: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
const UREGS: [RegisterX86; 8] = [
    RegisterX86::EAX,
    RegisterX86::ECX,
    RegisterX86::EDX,
    RegisterX86::EBX,
    RegisterX86::ESP,
    RegisterX86::EBP,
    RegisterX86::ESI,
    RegisterX86::EDI,
];

const FLAG_MASK: u32 = 0x0cd5; // CF PF AF ZF SF OF DF
const SZP_CF: u32 = 0x00c5; // SF ZF PF CF (for shifts: AF/OF unreliable)

// ---------------- RNG ----------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn u32(&mut self) -> u32 {
        (self.next() >> 16) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        (self.next() % n.max(1) as u64) as u32
    }
}

// ---------------- initial state ----------------

#[derive(Clone, Debug)]
struct Init {
    regs: [u32; 8],
    eflags: u32,
    data: Vec<u8>,
}

impl Init {
    fn random(rng: &mut Rng) -> Init {
        let mut regs = [0u32; 8];
        regs[0] = rng.u32();
        regs[1] = rng.u32();
        regs[2] = rng.u32();
        regs[3] = DATA + (rng.u32() & 0x3ff0);
        regs[4] = STACK_TOP;
        regs[5] = DATA + (rng.u32() & 0x3ff0);
        regs[6] = DATA + (rng.u32() & 0x3ff0);
        regs[7] = DATA + (rng.u32() & 0x3ff0);
        let eflags = (rng.u32() & FLAG_MASK) | 0x202;
        let mut data = vec![0u8; DATA_SIZE];
        for b in data.iter_mut().take(0x4000) {
            *b = rng.next() as u8;
        }
        Init { regs, eflags, data }
    }
}

// ---------------- execution ----------------

enum RunResult {
    Completed {
        regs: [u32; 8],
        eflags: u32,
        data: Vec<u8>,
        stack: Vec<u8>,
    },
    Fault {
        eip: u32,
        desc: String,
    },
}

fn run_wire(code: &[u8], init: &Init, max_steps: u64) -> RunResult {
    let mut mem = GuestMem::new().expect("mmap");
    mem.commit(CODE, CODE_SIZE as u32).unwrap();
    mem.commit(DATA, DATA_SIZE as u32).unwrap();
    mem.commit(STACK, STACK_SIZE as u32).unwrap();
    mem.write_bytes(CODE, code).unwrap();
    mem.write_bytes(DATA, &init.data).unwrap();
    let mut cpu = Cpu::new(CODE, init.regs[4], 0);
    cpu.regs = init.regs;
    cpu.eflags = init.eflags;
    let code_end = CODE + code.len() as u32;
    let mut steps = 0u64;
    while cpu.eip != code_end {
        let eip0 = cpu.eip;
        if let Err(e) = cpu.step(&mut mem) {
            return RunResult::Fault {
                eip: eip0,
                desc: format!("wire: {e}"),
            };
        }
        steps += 1;
        if steps > max_steps {
            return RunResult::Fault {
                eip: eip0,
                desc: "wire: step limit".into(),
            };
        }
    }
    let mut data = vec![0u8; DATA_SIZE];
    mem.read_bytes(DATA, &mut data).unwrap();
    let mut stack = vec![0u8; STACK_SIZE];
    mem.read_bytes(STACK, &mut stack).unwrap();
    RunResult::Completed {
        regs: cpu.regs,
        eflags: cpu.eflags,
        data,
        stack,
    }
}

fn run_unicorn(code: &[u8], init: &Init, max_steps: u64) -> RunResult {
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_32).expect("unicorn");
    uc.mem_map(CODE as u64, CODE_SIZE as u64, Prot::ALL).unwrap();
    uc.mem_map(DATA as u64, DATA_SIZE as u64, Prot::READ | Prot::WRITE)
        .unwrap();
    uc.mem_map(STACK as u64, STACK_SIZE as u64, Prot::READ | Prot::WRITE)
        .unwrap();
    uc.mem_write(CODE as u64, code).unwrap();
    uc.mem_write(DATA as u64, &init.data).unwrap();
    for i in 0..8 {
        uc.reg_write(UREGS[i], init.regs[i] as u64).unwrap();
    }
    uc.reg_write(RegisterX86::EFLAGS, init.eflags as u64).unwrap();
    let code_end = (CODE + code.len() as u32) as u64;
    match uc.emu_start(CODE as u64, code_end, 0, max_steps as usize) {
        Ok(()) if uc.reg_read(RegisterX86::EIP).unwrap() as u32 == code_end as u32 => {
            let mut regs = [0u32; 8];
            for i in 0..8 {
                regs[i] = uc.reg_read(UREGS[i]).unwrap() as u32;
            }
            let eflags = uc.reg_read(RegisterX86::EFLAGS).unwrap() as u32;
            let data = uc.mem_read_as_vec(DATA as u64, DATA_SIZE).unwrap();
            let stack = uc.mem_read_as_vec(STACK as u64, STACK_SIZE).unwrap();
            RunResult::Completed {
                regs,
                eflags,
                data,
                stack,
            }
        }
        Ok(()) => {
            // ran out of instruction budget before reaching code_end
            RunResult::Fault {
                eip: uc.reg_read(RegisterX86::EIP).unwrap_or(0) as u32,
                desc: "unicorn: step limit".into(),
            }
        }
        Err(e) => {
            let eip = uc.reg_read(RegisterX86::EIP).unwrap_or(0) as u32;
            RunResult::Fault {
                eip,
                desc: format!("unicorn: {e:?}"),
            }
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

fn compare(
    tag: &str,
    code: &[u8],
    init: &Init,
    w: &RunResult,
    u: &RunResult,
    flag_mask: u32,
) -> Result<(), String> {
    let ctx = || {
        format!(
            "\n  code: {}\n  init regs: {:08x?} eflags={:#x}",
            hex(code),
            init.regs,
            init.eflags
        )
    };
    match (w, u) {
        (
            RunResult::Completed {
                regs: wr,
                eflags: we,
                data: wd,
                stack: ws,
            },
            RunResult::Completed {
                regs: ur,
                eflags: ue,
                data: ud,
                stack: us,
            },
        ) => {
            let mut errs = String::new();
            for i in 0..8 {
                if wr[i] != ur[i] {
                    errs += &format!(" {}: wire={:#010x} uni={:#010x}", REGNAME[i], wr[i], ur[i]);
                }
            }
            let fd = (we ^ ue) & flag_mask;
            if fd != 0 {
                errs += &format!(
                    " eflags: wire={:#x} uni={:#x} diff={:#x} (mask {:#x})",
                    we, ue, fd, flag_mask
                );
            }
            if wd != ud {
                let idx = wd.iter().zip(ud.iter()).position(|(a, b)| a != b).unwrap();
                errs += &format!(" data[{idx:#x}]: wire={:#x} uni={:#x}", wd[idx], ud[idx]);
            }
            if ws != us {
                let idx = ws.iter().zip(us.iter()).position(|(a, b)| a != b).unwrap();
                errs += &format!(" stack[{idx:#x}]: wire={:#x} uni={:#x}", ws[idx], us[idx]);
            }
            if errs.is_empty() {
                Ok(())
            } else {
                Err(format!("{tag}: MISMATCH{errs}{}", ctx()))
            }
        }
        (RunResult::Fault { eip: we, .. }, RunResult::Fault { eip: ue, .. }) => {
            if we == ue {
                Ok(())
            } else {
                Err(format!(
                    "{tag}: fault EIP differs: wire={we:#x} uni={ue:#x}{}",
                    ctx()
                ))
            }
        }
        (RunResult::Fault { desc, .. }, RunResult::Completed { .. }) => {
            Err(format!("{tag}: wire faulted ({desc}), unicorn completed{}", ctx()))
        }
        (RunResult::Completed { .. }, RunResult::Fault { desc, .. }) => {
            Err(format!("{tag}: wire completed, unicorn faulted ({desc}){}", ctx()))
        }
    }
}

// ---------------- templates ----------------

struct Template {
    name: &'static str,
    flag_mask: u32,
    gen: fn(&mut Rng, &mut Vec<u8>),
    pre: &'static [u8],
}

const PTR_REGS: [u8; 4] = [3, 5, 6, 7]; // EBX EBP ESI EDI (point into DATA)

/// Encode modrm (+SIB +disp) for `reg_field`. Memory forms only use pointer
/// registers as base with small positive displacements (stays in DATA).
fn encode_rm(rng: &mut Rng, reg_field: u8, allow_mem: bool) -> Vec<u8> {
    if !allow_mem || rng.below(2) == 0 {
        let rm = rng.below(8) as u8;
        return vec![0xc0 | (reg_field << 3) | rm];
    }
    encode_rm_mem(rng, reg_field)
}

fn encode_rm_mem(rng: &mut Rng, reg_field: u8) -> Vec<u8> {
    let base = PTR_REGS[rng.below(4) as usize];
    match rng.below(4) {
        0 => {
            if base == 5 {
                vec![0x40 | (reg_field << 3) | 5, 0]
            } else {
                vec![reg_field << 3 | base]
            }
        }
        1 => {
            let d = rng.below(0x40) as u8;
            vec![0x40 | (reg_field << 3) | base, d]
        }
        2 => {
            let d = rng.below(0x800);
            let mut e = vec![0x80 | (reg_field << 3) | base];
            e.extend_from_slice(&d.to_le_bytes());
            e
        }
        _ => {
            let scale = rng.below(4) as u8;
            let d = rng.below(0x20) as u8;
            vec![
                0x40 | (reg_field << 3) | 4,
                (scale << 6) | (4 << 3) | base,
                d,
            ]
        }
    }
}

fn gen_alu(rng: &mut Rng, out: &mut Vec<u8>) {
    let kind = rng.below(8) as u8;
    let base_op = kind * 8;
    match rng.below(6) {
        0 => {
            out.push(base_op + 1);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        1 => {
            out.push(base_op + 3);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        2 => {
            out.push(base_op);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        3 => {
            out.push(base_op + 2);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        4 => {
            out.push(base_op + 4);
            out.push(rng.u32() as u8);
        }
        _ => {
            out.push(base_op + 5);
            out.extend_from_slice(&rng.u32().to_le_bytes());
        }
    }
}

fn gen_group1(rng: &mut Rng, out: &mut Vec<u8>) {
    let op = [0x80u8, 0x81, 0x83][rng.below(3) as usize];
    out.push(op);
    { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
    match op {
        0x80 => out.push(rng.u32() as u8),
        0x81 => out.extend_from_slice(&rng.u32().to_le_bytes()),
        _ => out.push(rng.u32() as u8),
    }
}

fn gen_incdec(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(4) {
        0 => out.push(0x40 + rng.below(8) as u8),
        1 => out.push(0x48 + rng.below(8) as u8),
        2 => {
            out.push(0xfe);
            { let f = rng.below(2) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        _ => {
            out.push(0xff);
            { let f = rng.below(2) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
    }
}

fn gen_mov(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(9) {
        0 => {
            out.push(0x89);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        1 => {
            out.push(0x8b);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        2 => {
            out.push(0x88);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        3 => {
            out.push(0x8a);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        4 => {
            out.push(0xb8 + rng.below(8) as u8);
            out.extend_from_slice(&rng.u32().to_le_bytes());
        }
        5 => {
            out.push(0xb0 + rng.below(8) as u8);
            out.push(rng.u32() as u8);
        }
        6 => {
            out.push(0xc7);
            out.extend_from_slice(&encode_rm(rng, 0, true));
            out.extend_from_slice(&rng.u32().to_le_bytes());
        }
        7 => {
            out.push(0xc6);
            out.extend_from_slice(&encode_rm(rng, 0, true));
            out.push(rng.u32() as u8);
        }
        _ => {
            let op = [0xb6u8, 0xb7, 0xbe, 0xbf][rng.below(4) as usize];
            out.push(0x0f);
            out.push(op);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
    }
}

fn gen_xchg(rng: &mut Rng, out: &mut Vec<u8>) {
    if rng.below(2) == 0 {
        out.push(0x91 + rng.below(7) as u8);
    } else {
        out.push(if rng.below(2) == 0 { 0x86 } else { 0x87 });
        { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
    }
}

fn gen_lea(rng: &mut Rng, out: &mut Vec<u8>) {
    let base = PTR_REGS[rng.below(4) as usize];
    let reg = rng.below(8) as u8;
    match rng.below(4) {
        0 => {
            out.push(0x8d);
            if base == 5 {
                out.extend_from_slice(&[0x40 | (reg << 3) | 5, 0]);
            } else {
                out.push(reg << 3 | base);
            }
        }
        1 => {
            out.push(0x8d);
            out.extend_from_slice(&[0x40 | (reg << 3) | base, rng.below(0x40) as u8]);
        }
        2 => {
            out.push(0x8d);
            out.push(0x80 | (reg << 3) | base);
            out.extend_from_slice(&rng.below(0x800).to_le_bytes());
        }
        _ => {
            let scale = rng.below(4) as u8;
            let idx = [0u8, 1, 2][rng.below(3) as usize];
            let imm = rng.below(0x20);
            out.push(0xb8 + idx); // mov r, imm32 (flag-preserving)
            out.extend_from_slice(&imm.to_le_bytes());
            out.push(0x8d);
            out.extend_from_slice(&[
                0x40 | (reg << 3) | 4,
                (scale << 6) | (idx << 3) | base,
                rng.below(0x20) as u8,
            ]);
        }
    }
}

fn gen_shift(rng: &mut Rng, out: &mut Vec<u8>) {
    let kind = rng.below(8) as u8; // rol ror rcl rcr shl shr sal sar
    let form = rng.below(4);
    match form {
        0 => {
            out.push(0xc1);
            out.extend_from_slice(&encode_rm(rng, kind, true));
            let c = [0u8, 1, 1, 2, 5, 31, 32, 63][rng.below(8) as usize];
            out.push(c);
        }
        1 => {
            out.push(0xd1);
            out.extend_from_slice(&encode_rm(rng, kind, true));
        }
        2 => {
            out.push(0xd3);
            out.extend_from_slice(&encode_rm(rng, kind, true));
        }
        _ => {
            let op = [0xc0u8, 0xd0, 0xd2][rng.below(3) as usize];
            out.push(op);
            out.extend_from_slice(&encode_rm(rng, kind, true));
            if op == 0xc0 {
                out.push(rng.below(8) as u8);
            }
        }
    }
}

fn gen_muldiv(rng: &mut Rng, out: &mut Vec<u8>) {
    let width = rng.below(3);
    let signed = rng.below(2) == 0;
    match width {
        0 => {
            // (i)div dl with AX/divisor arranged so the quotient always fits
            let dl = 0x40 + rng.below(0xc0); // 64..=255
            let ah = if signed { 0 } else { rng.below(dl.max(1)) };
            let ax = ((ah as u32) << 8) | rng.below(0x100);
            out.extend_from_slice(&[0x66, 0xb8]);
            out.extend_from_slice(&(ax as u16).to_le_bytes());
            out.extend_from_slice(&[0xb2, dl as u8]);
            out.push(0xf6);
            out.push(0xc0 | ((if signed { 7 } else { 6 }) << 3) | 2);
        }
        1 => {
            let bx = 0x8000u16 + rng.below(0x7fff) as u16;
            out.extend_from_slice(&[0x66, 0xba, 0, 0]);
            out.extend_from_slice(&[0x66, 0xbb]);
            out.extend_from_slice(&bx.to_le_bytes());
            out.push(0xf7);
            out.push(0xc0 | ((if signed { 7 } else { 6 }) << 3) | 3);
        }
        _ => {
            let bx = 0x8000_0000u32 + rng.u32() % 0x7fff_ffff;
            out.push(0xba);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.push(0xbb);
            out.extend_from_slice(&bx.to_le_bytes());
            out.push(0xf7);
            out.push(0xc0 | ((if signed { 7 } else { 6 }) << 3) | 3);
        }
    }
}

fn gen_mul(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(4) {
        0 => {
            out.push(0xf6);
            { let f = if rng.below(2) == 0 { 4 } else { 5 }; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        1 => {
            out.push(0xf7);
            { let f = if rng.below(2) == 0 { 4 } else { 5 }; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        2 => {
            out.push(0x0f);
            out.push(0xaf);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        _ => {
            let op = if rng.below(2) == 0 { 0x69 } else { 0x6b };
            out.push(op);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
            if op == 0x69 {
                out.extend_from_slice(&rng.u32().to_le_bytes());
            } else {
                out.push(rng.u32() as u8);
            }
        }
    }
}

fn gen_stack(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(6) {
        0 => out.push(0x50 + rng.below(8) as u8),
        1 => out.push(0x58 + rng.below(8) as u8),
        2 => {
            out.push(0x68);
            out.extend_from_slice(&rng.u32().to_le_bytes());
        }
        3 => {
            out.push(0x6a);
            out.push(rng.u32() as u8);
        }
        4 => out.push(0x9c),
        _ => out.extend_from_slice(&[0x9c, 0x9d]), // pushf; popf (TF-safe)
    }
}

fn gen_setcc_cmov(rng: &mut Rng, out: &mut Vec<u8>) {
    out.push(0x0f);
    if rng.below(2) == 0 {
        out.push(0x90 + rng.below(16) as u8);
        out.extend_from_slice(&encode_rm(rng, 0, true));
    } else {
        out.push(0x40 + rng.below(16) as u8);
        { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
    }
}

fn gen_bt(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(2) {
        0 => {
            let op = [0xa3u8, 0xab, 0xb3, 0xbb][rng.below(4) as usize];
            let idx = rng.below(8) as u8;
            out.push(0xb8 + idx);
            out.extend_from_slice(&rng.below(0x20).to_le_bytes());
            out.push(0x0f);
            out.push(op);
            out.extend_from_slice(&encode_rm(rng, idx, true));
        }
        1 => {
            out.push(0x0f);
            out.push(0xba);
            { let f = 4 + rng.below(4) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
            out.push(rng.below(0x20) as u8);
        }
        _ => {
            out.push(0x0f);
            out.push(0xc8 + rng.below(8) as u8);
        }
    }
}

fn gen_bsf(rng: &mut Rng, out: &mut Vec<u8>) {
    out.push(0x0f);
    out.push(if rng.below(2) == 0 { 0xbc } else { 0xbd });
    let f = rng.below(8) as u8;
    out.extend_from_slice(&encode_rm(rng, f, true));
}

fn gen_lock(rng: &mut Rng, out: &mut Vec<u8>) {
    out.push(0x0f);
    out.push([0xb0u8, 0xb1, 0xc0, 0xc1][rng.below(4) as usize]);
    let f = rng.below(8) as u8;
    out.extend_from_slice(&encode_rm(rng, f, true));
}

fn gen_flagsmisc(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(7) {
        0 => out.push(0x9e),
        1 => out.push(0x9f),
        2 => out.push(0xf8),
        3 => out.push(0xf9),
        4 => out.push(0xf5),
        5 => out.push(0x98),
        _ => out.push(0x99),
    }
}

fn gen_not_neg_test(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(3) {
        0 => {
            out.push(0xf6);
            out.extend_from_slice(&encode_rm(rng, 0, true));
            out.push(rng.u32() as u8);
        }
        1 => {
            out.push(0xf7);
            out.extend_from_slice(&encode_rm(rng, 0, true));
            out.extend_from_slice(&rng.u32().to_le_bytes());
        }
        _ => {
            out.push(if rng.below(2) == 0 { 0xf6 } else { 0xf7 });
            { let f = 2 + rng.below(2) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
    }
}

fn gen_16bit(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(8) {
        0 => {
            out.push(0x66);
            let kind = rng.below(8) as u8;
            out.push(kind * 8 + if rng.below(2) == 0 { 1 } else { 3 });
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        1 => {
            out.push(0x66);
            out.push(if rng.below(2) == 0 { 0x89 } else { 0x8b });
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        2 => {
            out.push(0x66);
            out.push(0xb8 + rng.below(8) as u8);
            out.extend_from_slice(&(rng.u32() as u16).to_le_bytes());
        }
        3 => {
            out.push(0x66);
            let op = [0x40u8, 0x48, 0x50, 0x58][rng.below(4) as usize];
            out.push(op + rng.below(8) as u8);
        }
        4 => {
            out.push(0x66);
            let op = if rng.below(2) == 0 { 0x81 } else { 0x83 };
            out.push(op);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
            if op == 0x81 {
                out.extend_from_slice(&(rng.u32() as u16).to_le_bytes());
            } else {
                out.push(rng.u32() as u8);
            }
        }
        5 => {
            out.push(0x66);
            out.push(if rng.below(2) == 0 { 0x98 } else { 0x99 });
        }
        6 => {
            out.push(0x66);
            out.push(0x0f);
            out.push([0xb6u8, 0xbe][rng.below(2) as usize]);
            { let f = rng.below(8) as u8; out.extend_from_slice(&encode_rm(rng, f, true)); }
        }
        _ => {
            out.push(0x66);
            if rng.below(2) == 0 {
                out.push(0xc7);
                out.extend_from_slice(&encode_rm(rng, 0, true));
                out.extend_from_slice(&(rng.u32() as u16).to_le_bytes());
            } else {
                out.push(0x68);
                out.extend_from_slice(&(rng.u32() as u16).to_le_bytes());
            }
        }
    }
}

fn gen_nop_forms(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(4) {
        0 => out.push(0x90),
        1 => out.extend_from_slice(&[0x66, 0x90]),
        2 => {
            out.extend_from_slice(&[0x0f, 0x1f]);
            out.extend_from_slice(&encode_rm(rng, 0, true));
        }
        _ => out.extend_from_slice(&[0xf3, 0x90]),
    }
}

fn gen_x87_ctrl(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(4) {
        0 => out.extend_from_slice(&[0xdb, 0xe3]),
        1 => out.extend_from_slice(&[0xdb, 0xe2]),
        2 => {
            out.push(0xd9);
            out.extend_from_slice(&encode_rm_mem(rng, 5)); // fldcw
        }
        _ => {
            out.push(0xd9);
            out.extend_from_slice(&encode_rm_mem(rng, 7)); // fnstcw
        }
    }
}

// ---------------- driver ----------------

fn run_family(t: &Template, cases: usize, seed: u64) {
    let mut rng = Rng(seed);
    for case in 0..cases {
        let init = Init::random(&mut rng);
        let mut code = Vec::new();
        code.extend_from_slice(t.pre);
        let k = 1 + rng.below(12);
        for _ in 0..k {
            (t.gen)(&mut rng, &mut code);
        }
        let w = run_wire(&code, &init, 100_000);
        let u = run_unicorn(&code, &init, 100_000);
        if let Err(e) = compare(t.name, &code, &init, &w, &u, t.flag_mask) {
            panic!("case {case} (seed {seed:#x}): {e}");
        }
    }
}

const CASES: usize = 300;

#[test]
fn alu_family() {
    run_family(&Template { name: "alu", flag_mask: FLAG_MASK, gen: gen_alu, pre: &[] }, CASES, 0xa10);
}

#[test]
fn group1_family() {
    run_family(&Template { name: "group1", flag_mask: FLAG_MASK, gen: gen_group1, pre: &[] }, CASES, 0x6b1);
}

#[test]
fn incdec_family() {
    run_family(&Template { name: "incdec", flag_mask: FLAG_MASK, gen: gen_incdec, pre: &[] }, CASES, 0x1ec);
}

#[test]
fn mov_family() {
    run_family(&Template { name: "mov", flag_mask: FLAG_MASK, gen: gen_mov, pre: &[] }, CASES, 0x907);
}

#[test]
fn xchg_family() {
    run_family(&Template { name: "xchg", flag_mask: FLAG_MASK, gen: gen_xchg, pre: &[] }, CASES, 0x0c6);
}

#[test]
fn lea_family() {
    run_family(&Template { name: "lea", flag_mask: FLAG_MASK, gen: gen_lea, pre: &[] }, CASES, 0x1ea);
}

#[test]
fn shift_family() {
    run_family(&Template { name: "shift", flag_mask: SZP_CF, gen: gen_shift, pre: &[] }, CASES, 0x5f17);
}

#[test]
fn mul_family() {
    run_family(&Template { name: "mul", flag_mask: 0x0801, gen: gen_mul, pre: &[] }, CASES, 0x111);
}

#[test]
fn div_family() {
    run_family(&Template { name: "div", flag_mask: 0, gen: gen_muldiv, pre: &[] }, CASES, 0xd17);
}

#[test]
fn stack_family() {
    run_family(&Template { name: "stack", flag_mask: FLAG_MASK, gen: gen_stack, pre: &[] }, CASES, 0x57ac);
}

#[test]
fn setcc_cmov_family() {
    run_family(
        &Template { name: "setcc_cmov", flag_mask: FLAG_MASK, gen: gen_setcc_cmov, pre: &[] },
        CASES,
        0x5ecc,
    );
}

#[test]
fn bt_family() {
    // BT*: only CF is defined.
    run_family(&Template { name: "bt", flag_mask: 0x1, gen: gen_bt, pre: &[] }, CASES, 0xb170);
}

#[test]
fn bsf_bsr_family() {
    // BSF/BSR: only ZF is defined.
    run_family(&Template { name: "bsf_bsr", flag_mask: 0x40, gen: gen_bsf, pre: &[] }, CASES, 0xb5f);
}

#[test]
fn lock_family() {
    // CMPXCHG/XADD: all arithmetic flags are defined.
    run_family(&Template { name: "lock", flag_mask: FLAG_MASK, gen: gen_lock, pre: &[] }, CASES, 0x10cc);
}

#[test]
fn flagsmisc_family() {
    run_family(&Template { name: "flagsmisc", flag_mask: FLAG_MASK, gen: gen_flagsmisc, pre: &[] }, CASES, 0xf1a6);
}

#[test]
fn not_neg_family() {
    run_family(
        &Template { name: "not_neg_test", flag_mask: FLAG_MASK, gen: gen_not_neg_test, pre: &[] },
        CASES,
        0x9e6,
    );
}

#[test]
fn opsize16_family() {
    run_family(&Template { name: "opsize16", flag_mask: FLAG_MASK, gen: gen_16bit, pre: &[] }, CASES, 0x1666);
}

#[test]
fn nop_forms() {
    run_family(&Template { name: "nop", flag_mask: FLAG_MASK, gen: gen_nop_forms, pre: &[] }, 100, 0x90);
}

#[test]
fn x87_ctrl_family() {
    run_family(&Template { name: "x87ctrl", flag_mask: FLAG_MASK, gen: gen_x87_ctrl, pre: &[0xdb, 0xe3] }, 150, 0x87);
}

// ---------------- constructed tests ----------------

#[test]
fn string_ops() {
    let mut rng = Rng(0x57a1);
    for case in 0..200 {
        let mut init = Init::random(&mut rng);
        let mut code = Vec::new();
        code.push(0xb9); // mov ecx, small
        code.extend_from_slice(&(1 + rng.below(64)).to_le_bytes());
        code.push(if rng.below(2) == 0 { 0xfc } else { 0xfd });
        init.regs[6] = DATA + 0x2000 + (rng.u32() & 0xff);
        init.regs[7] = DATA + 0x2000 + (rng.u32() & 0xff);
        let ops = [0xa4u8, 0xa5, 0xa6, 0xa7, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf];
        let op = ops[rng.below(10) as usize];
        match rng.below(3) {
            0 => code.push(op),
            1 => code.extend_from_slice(&[0xf3, op]),
            _ => code.extend_from_slice(&[0xf2, op]),
        }
        let w = run_wire(&code, &init, 100_000);
        let u = run_unicorn(&code, &init, 100_000);
        if let Err(e) = compare("string", &code, &init, &w, &u, FLAG_MASK) {
            panic!("case {case}: {e}");
        }
    }
}

#[test]
fn control_flow() {
    let mut rng = Rng(0xc0a1);
    for case in 0..150 {
        let init = Init::random(&mut rng);
        let mut code = Vec::new();
        code.push(0xb9); // mov ecx, small (never 0: loop -2 would spin 2^32 times)
        code.extend_from_slice(&(1 + rng.below(7)).to_le_bytes());
        let call_pos = code.len();
        code.push(0xe8);
        code.extend_from_slice(&0u32.to_le_bytes());
        code.extend_from_slice(&[0x83, 0xc0, 0x03]); // add eax, 3
        let jmp_pos = code.len();
        code.push(0xe9);
        code.extend_from_slice(&0u32.to_le_bytes());
        let l1 = code.len();
        code.extend_from_slice(&[0xc8, 0x10, 0x00, 0x00]); // enter 16,0
        code.extend_from_slice(&[0x83, 0xc0, 0x05]); // add eax, 5
        code.extend_from_slice(&[0xc9, 0xc3]); // leave ; ret
        let l2 = code.len();
        code.extend_from_slice(&[0xe2, 0xfe]); // loop -2 (to self)
        code.extend_from_slice(&[0xe3, 0x01, 0x90]); // jecxz +1 over nop
        code.extend_from_slice(&[0x83, 0xc0, 0x07]); // add eax, 7
        let rel1 = (l1 as i32 - (call_pos as i32 + 5)) as u32;
        code[call_pos + 1..call_pos + 5].copy_from_slice(&rel1.to_le_bytes());
        let rel2 = (l2 as i32 - (jmp_pos as i32 + 5)) as u32;
        code[jmp_pos + 1..jmp_pos + 5].copy_from_slice(&rel2.to_le_bytes());
        let w = run_wire(&code, &init, 100_000);
        let u = run_unicorn(&code, &init, 100_000);
        if let Err(e) = compare("control", &code, &init, &w, &u, FLAG_MASK) {
            panic!("case {case}: {e}");
        }
    }
}

#[test]
fn indirect_control() {
    let mut rng = Rng(0x1d1c);
    for case in 0..100 {
        let mut init = Init::random(&mut rng);
        let mut code = Vec::new();
        // 0: mov esi, L1
        code.push(0xbe);
        let l1_imm_pos = code.len();
        code.extend_from_slice(&0u32.to_le_bytes());
        // 5: call esi
        code.extend_from_slice(&[0xff, 0xd6]);
        // 7: add eax, 2
        code.extend_from_slice(&[0x83, 0xc0, 0x02]);
        // 10: jmp L2
        let jmp_pos = code.len();
        code.extend_from_slice(&[0xe9, 0, 0, 0, 0]);
        // 15: L1: add eax, 9 ; ret
        let l1 = code.len();
        code.extend_from_slice(&[0x83, 0xc0, 0x09, 0xc3]);
        // 19: L3: add eax, 4 ; ret
        let l3 = code.len();
        code.extend_from_slice(&[0x83, 0xc0, 0x04, 0xc3]);
        // 23: L2: call [DATA+0x100]  -> L3
        let l2 = code.len();
        code.extend_from_slice(&[0xff, 0x15]);
        code.extend_from_slice(&(DATA + 0x100).to_le_bytes());
        // 29: end
        code[l1_imm_pos..l1_imm_pos + 4].copy_from_slice(&(CODE + l1 as u32).to_le_bytes());
        let rel2 = l2 as i32 - (jmp_pos as i32 + 5);
        code[jmp_pos + 1..jmp_pos + 5].copy_from_slice(&(rel2 as u32).to_le_bytes());
        init.data[0x100..0x104].copy_from_slice(&(CODE + l3 as u32).to_le_bytes());
        let w = run_wire(&code, &init, 100_000);
        let u = run_unicorn(&code, &init, 100_000);
        if let Err(e) = compare("indirect", &code, &init, &w, &u, FLAG_MASK) {
            panic!("case {case}: {e}");
        }
    }
}

#[test]
fn shift_of_count1() {
    let mut rng = Rng(0x0f1);
    for case in 0..200 {
        let init = Init::random(&mut rng);
        let kind = [0u8, 1, 4, 5, 6, 7][rng.below(6) as usize];
        let rm = rng.below(8) as u8;
        let code = vec![0xd1, 0xc0 | (kind << 3) | rm];
        let w = run_wire(&code, &init, 10_000);
        let u = run_unicorn(&code, &init, 10_000);
        if let Err(e) = compare("shift_of1", &code, &init, &w, &u, FLAG_MASK & !0x10) {
            panic!("case {case}: {e}");
        }
    }
}

#[test]
fn adc_sbb_chain() {
    let mut rng = Rng(0xadcb);
    for case in 0..150 {
        let init = Init::random(&mut rng);
        let mut code = Vec::new();
        for _ in 0..32 {
            let op = [0x13u8, 0x1b][rng.below(2) as usize];
            code.push(op);
            code.push(0xc0 | (rng.below(8) as u8) << 3 | rng.below(8) as u8);
        }
        let w = run_wire(&code, &init, 10_000);
        let u = run_unicorn(&code, &init, 10_000);
        if let Err(e) = compare("adc_sbb", &code, &init, &w, &u, FLAG_MASK) {
            panic!("case {case}: {e}");
        }
    }
}
