//! Lockstep debugger: execute one instruction sequence on the wire
//! interpreter and Unicorn simultaneously, comparing state after every
//! instruction. Prints the first divergence with full context.
//!
//! usage:
//!   cargo run --example lockstep -- <code-hex> [--regs eax,ecx,edx,ebx,esp,ebp,esi,edi] [--eflags HEX]
//! Values hex, regs comma-separated (no 0x).

use unicorn_engine::{Arch, Mode, Prot, RegisterX86, Unicorn};
use wire::cpu::Cpu;
use wire::mem::GuestMem;

const CODE: u32 = 0x1_0000;
const CODE_SIZE: u32 = 0x1_0000;
const DATA: u32 = 0x2_0000;
const DATA_SIZE: u32 = 0x1_0000;
const STACK: u32 = 0x3_0000;
const STACK_SIZE: u32 = 0x1_0000;
const STACK_TOP: u32 = STACK + STACK_SIZE - 16;

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
const REGNAME: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];

fn unhex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: lockstep <code-hex> [--regs r,r,r,r,r,r,r,r] [--eflags HEX] [--data-hex HEX]"
        );
        std::process::exit(2);
    }
    let code = unhex(&args[1]);
    let mut regs = [
        0x1111_1111u32,
        0x2222_2222,
        0x3333_3333,
        DATA + 0x1000,
        STACK_TOP,
        DATA + 0x1800,
        DATA + 0x2000,
        DATA + 0x2800,
    ];
    let mut eflags: u32 = 0x202;
    let mut data_fill: Vec<u8> = Vec::new();
    let mut xmm = [[0; 16]; 8];
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--regs" => {
                i += 1;
                for (j, part) in args[i].split(',').enumerate() {
                    regs[j] = u32::from_str_radix(part, 16).unwrap();
                }
            }
            "--xmm-hex" => {
                i += 1;
                for (j, part) in args[i].split(',').enumerate() {
                    xmm[j].copy_from_slice(&unhex(part));
                }
            }
            "--eflags" => {
                i += 1;
                eflags = u32::from_str_radix(&args[i], 16).unwrap();
            }
            "--data-hex" => {
                i += 1;
                data_fill = unhex(&args[i]);
            }
            _ => {}
        }
        i += 1;
    }

    // wire side
    let mut mem = GuestMem::new().unwrap();
    mem.commit(CODE, CODE_SIZE).unwrap();
    mem.commit(DATA, DATA_SIZE).unwrap();
    mem.commit(STACK, STACK_SIZE).unwrap();
    mem.write_bytes(CODE, &code).unwrap();
    for (k, b) in data_fill.iter().enumerate() {
        mem.write_u8(DATA + k as u32, *b).unwrap();
    }
    let mut cpu = Cpu::new(CODE, regs[4], 0);
    cpu.regs = regs;
    cpu.eflags = eflags;
    cpu.xmm = xmm;

    // unicorn side
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_32).unwrap();
    uc.mem_map(CODE as u64, CODE_SIZE as u64, Prot::ALL)
        .unwrap();
    uc.mem_map(DATA as u64, DATA_SIZE as u64, Prot::READ | Prot::WRITE)
        .unwrap();
    uc.mem_map(STACK as u64, STACK_SIZE as u64, Prot::READ | Prot::WRITE)
        .unwrap();
    uc.mem_write(CODE as u64, &code).unwrap();
    if !data_fill.is_empty() {
        uc.mem_write(DATA as u64, &data_fill).unwrap();
    }
    for j in 0..8 {
        uc.reg_write(UREGS[j], regs[j] as u64).unwrap();
    }
    uc.reg_write(RegisterX86::EFLAGS, eflags as u64).unwrap();
    uc.reg_write(RegisterX86::EIP, CODE as u64).unwrap();
    uc.reg_write(RegisterX86::MXCSR, 0x1f80).unwrap();
    for (j, v) in xmm.iter().enumerate() {
        uc.reg_write_long(RegisterX86::XMM0 as i32 + j as i32, v)
            .unwrap();
    }

    let code_end = CODE + code.len() as u32;
    for step in 0..10_000u64 {
        let w_eip = cpu.eip;
        let u_eip = uc.reg_read(RegisterX86::EIP).unwrap() as u32;
        if w_eip != u_eip {
            println!("EIP diverged at step {step}: wire={w_eip:#x} uni={u_eip:#x}");
        }
        if w_eip == code_end && u_eip == code_end {
            println!("both completed after {step} instructions; states equal");
            return;
        }
        // wire single step
        let w_err = cpu.step(&mut mem).err();
        // unicorn single step
        let u_err = uc.emu_start(u_eip as u64, 0, 0, 1).err();
        match (&w_err, &u_err) {
            (Some(we), Some(ue)) => {
                println!(
                    "both faulted at step {step} (eip {w_eip:#x}):\n  wire: {we}\n  uni:  {ue}"
                );
                return;
            }
            (Some(we), None) => {
                println!("wire faulted, unicorn ok at step {step} (eip {w_eip:#x}):\n  wire: {we}");
                return;
            }
            (None, Some(ue)) => {
                println!("wire ok, unicorn faulted at step {step} (eip {w_eip:#x}):\n  uni: {ue}");
                return;
            }
            _ => {}
        }
        // compare
        let mut diff = String::new();
        for j in 0..8 {
            let uv = uc.reg_read(UREGS[j]).unwrap() as u32;
            if cpu.regs[j] != uv {
                diff += &format!(
                    " {}: wire={:#010x} uni={:#010x}",
                    REGNAME[j], cpu.regs[j], uv
                );
            }
        }
        for j in 0..8 {
            let uv = uc
                .reg_read_long(RegisterX86::XMM0 as i32 + j as i32)
                .unwrap();
            if cpu.xmm[j] != *uv {
                diff += &format!(" xmm{j}: wire={:02x?} uni={:02x?}", cpu.xmm[j], uv);
            }
        }
        let ue = uc.reg_read(RegisterX86::EFLAGS).unwrap() as u32;
        if (cpu.eflags ^ ue) & 0xcd5 != 0 {
            diff += &format!(" eflags: wire={:#x} uni={:#x}", cpu.eflags, ue);
        }
        if !diff.is_empty() {
            let mut bytes = Vec::new();
            for k in 0..10 {
                if let Ok(b) = mem.read_u8(w_eip.wrapping_add(k)) {
                    bytes.push(format!("{b:02x}"));
                }
            }
            println!(
                "DIVERGED after step {step} (instruction at {w_eip:#x}: {}):{diff}",
                bytes.join(" ")
            );
            return;
        }
    }
    println!("step limit");
}
