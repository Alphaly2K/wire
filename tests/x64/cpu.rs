//! Existing behavioral tests grouped by cpu; expectations remain independent of API catalogs.

use super::*;

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
fn amd64_scalar_ir_preserves_address_width_gs_and_fault_atomicity() {
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.map(STACK, 8192, READ | WRITE).unwrap();
    mem.map(0x10000, 4096, READ | WRITE).unwrap();
    mem.enable_execution_stats();
    let mut cpu = Cpu64::new(BASE, STACK + 0x800);
    cpu.gs_base = STACK;
    cpu.regs[0] = 0xfeed000000010000;
    cpu.regs[1] = 0xbeef000000000002;
    // addr32 [eax+ecx*4], addr32 [ecx*4+0x10000] (no base), GS:addr32
    // and GS-prefixed LEA, which must ignore the segment base.
    let cases: &[(&[u8], u64)] = &[
        (&[0x67, 0x48, 0x8b, 0x14, 0x88], 0x123456789abcdef0),
        (
            &[0x67, 0x48, 0x8b, 0x14, 0x8d, 0, 0, 1, 0],
            0x123456789abcdef0,
        ),
        (
            &[0x65, 0x67, 0x48, 0x8b, 0x14, 0x8d, 0, 0, 0, 0],
            0xfedcba9876543210,
        ),
        (&[0x65, 0x67, 0x48, 0x8d, 0x14, 0x88], 0x10008),
    ];
    mem.write_int(0x10008, 8, 0x123456789abcdef0).unwrap();
    mem.write_int(STACK + 8, 8, 0xfedcba9876543210).unwrap();
    for (code, expected) in cases {
        mem.write(BASE, code).unwrap();
        cpu.rip = BASE;
        cpu.step(&mut mem).unwrap();
        assert_eq!(cpu.regs[2], *expected);
    }
    // A read succeeds, but an ALU write crosses into a readonly page.
    mem.write(BASE, &[0x48, 0x83, 0x00, 1]).unwrap();
    cpu.regs[0] = STACK + 4092;
    mem.write_int(cpu.regs[0], 8, u64::MAX).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.rip = BASE;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu, before);
    assert_eq!(mem.read_int(cpu.regs[0], 8).unwrap(), u64::MAX);
    // False CMOV still reads/faults, and a successful I32 false CMOV clears high bits.
    mem.write(BASE, &[0x0f, 0x44, 0x08]).unwrap();
    cpu.rflags &= !0x40;
    cpu.regs[0] = 1 << 48;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu, before);
    cpu.regs[0] = STACK;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[1], 2);
    // POP [RSP] uses the incremented RSP. A destination failure rolls it back.
    mem.write(BASE, &[0x8f, 0x04, 0x24]).unwrap();
    cpu.rip = BASE;
    cpu.regs[4] = STACK + 4088;
    mem.write_int(cpu.regs[4], 8, 0x5555666677778888).unwrap();
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu, before);
    mem.protect(STACK + 4096, 4096, READ | WRITE).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[4], STACK + 4096);
    assert_eq!(mem.read_int(cpu.regs[4], 8).unwrap(), 0x5555666677778888);
    assert_eq!(mem.execution_stats().unwrap().fallback_steps, 0);
}

#[test]
fn amd64_scalar_ir_all_conditions_and_typed_memory_alu_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 8192, Prot::ALL).unwrap();
    uc.mem_map(STACK, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 8192, READ | WRITE | EXECUTE).unwrap();
    mem.map(STACK, 4096, READ | WRITE).unwrap();
    mem.enable_execution_stats();
    // Cover every condition, including parity and signed-overflow combinations.
    for cc in 0..16 {
        for code in [
            vec![0x0f, 0x80 + cc, 8, 0, 0, 0],
            vec![0x48, 0x0f, 0x40 + cc, 0xc3],
            vec![0x0f, 0x90 + cc, 0xc4], // SETcc AH preserves other bits.
        ] {
            mem.write(BASE, &code).unwrap();
            uc.mem_write(BASE, &code).unwrap();
            uc.ctl_remove_cache(BASE, BASE + 4096).unwrap();
            for pattern in 0..64u64 {
                let flags = 0x202
                    | [1, 4, 16, 64, 128, 2048]
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| pattern & (1 << index) != 0)
                        .map(|(_, bit)| bit)
                        .sum::<u64>();
                let mut cpu = Cpu64::new(BASE, STACK + 0x800);
                cpu.regs[0] = 0x123456789abcdef0;
                cpu.regs[3] = 0xfedcba9876543210;
                cpu.rflags = flags;
                uc.reg_write(U::RAX, cpu.regs[0]).unwrap();
                uc.reg_write(U::RBX, cpu.regs[3]).unwrap();
                uc.reg_write(U::RFLAGS, flags).unwrap();
                uc.emu_start(BASE, BASE + 4096, 0, 1).unwrap();
                cpu.step(&mut mem).unwrap();
                assert_eq!(
                    cpu.regs[0],
                    uc.reg_read(U::RAX).unwrap(),
                    "cc={cc} flags={flags:x}"
                );
                assert_eq!(
                    cpu.rip,
                    uc.reg_read(U::RIP).unwrap(),
                    "cc={cc} flags={flags:x}"
                );
                assert_eq!(cpu.rflags, flags);
            }
        }
    }
    for bits in [8, 16, 32, 64] {
        let mut code = Vec::new();
        for bytes in [
            vec![if bits == 8 { 0x00 } else { 0x01 }, 0x07], // ADD [RDI],acc
            vec![if bits == 8 { 0x80 } else { 0x83 }, 0x17, 0xff], // ADC [RDI],-1
            vec![if bits == 8 { 0x2a } else { 0x2b }, 0x07], // SUB acc,[RDI]
            vec![if bits == 8 { 0x80 } else { 0x83 }, 0xd8, 0xfe], // SBB acc,-2
            vec![if bits == 8 { 0x3a } else { 0x3b }, 0x07], // CMP acc,[RDI]
            vec![if bits == 8 { 0x84 } else { 0x85 }, 0x07], // TEST [RDI],acc
            vec![if bits == 8 { 0xfe } else { 0xff }, 0x07], // INC [RDI]
            vec![if bits == 8 { 0xfe } else { 0xff }, 0x0f], // DEC [RDI]
            vec![if bits == 8 { 0xf6 } else { 0xf7 }, 0x1f], // NEG [RDI]
            vec![if bits == 8 { 0xf6 } else { 0xf7 }, 0x17], // NOT [RDI]
        ] {
            if bits == 16 {
                code.push(0x66);
            }
            if bits == 64 {
                code.push(0x48);
            }
            code.extend(bytes);
        }
        mem.write(BASE, &code).unwrap();
        uc.mem_write(BASE, &code).unwrap();
        uc.ctl_remove_cache(BASE, BASE + 4096).unwrap();
        for value in [0, 1, u64::MAX, 0x8000800080808080, 0xfedcba9876543210] {
            let mut cpu = Cpu64::new(BASE, STACK + 0x800);
            cpu.regs[0] = value;
            cpu.regs[7] = STACK;
            cpu.rflags = 0xad7;
            mem.write_int(STACK, 8, value.rotate_left(17)).unwrap();
            uc.mem_write(STACK, &value.rotate_left(17).to_le_bytes())
                .unwrap();
            uc.reg_write(U::RAX, value).unwrap();
            uc.reg_write(U::RDI, STACK).unwrap();
            uc.reg_write(U::RFLAGS, cpu.rflags).unwrap();
            while cpu.rip < BASE + code.len() as u64 {
                let decoded = cpu.decode(&mem).unwrap();
                uc.emu_start(cpu.rip, BASE + 4096, 0, 1).unwrap();
                cpu.step(&mut mem).unwrap();
                assert_eq!(cpu.regs[0], uc.reg_read(U::RAX).unwrap());
                assert_eq!(cpu.rip, uc.reg_read(U::RIP).unwrap());
                let flag_mask = if decoded.mnemonic() == iced_x86::Mnemonic::Test {
                    0x8c5
                } else {
                    0x8d5
                };
                assert_eq!(
                    cpu.rflags & flag_mask,
                    uc.reg_read(U::RFLAGS).unwrap() & flag_mask,
                    "bits={bits} {decoded:?}"
                );
                let mut bytes = [0; 8];
                uc.mem_read(STACK, &mut bytes).unwrap();
                assert_eq!(mem.read_int(STACK, 8).unwrap(), u64::from_le_bytes(bytes));
            }
        }
    }
    let stats = mem.execution_stats().unwrap();
    assert_eq!(stats.ir_steps, 16 * 3 * 64 + 4 * 5 * 10);
    assert_eq!(stats.fallback_steps, 0);
}

#[test]
fn amd64_adc_sbb_carry_boundaries_all_widths_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut memory = Memory::default();
    memory.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    for bits in [8, 16, 32, 64] {
        let mask = u64::MAX >> (64 - bits);
        let sign = 1u64 << (bits - 1);
        let values = [0, 1, sign - 1, sign, sign + 1, mask - 1, mask];
        for subtract in [false, true] {
            let opcode = if subtract { 0x18 } else { 0x10 };
            let code = match bits {
                8 => vec![opcode, 0xd8],
                16 => vec![0x66, opcode + 1, 0xd8],
                32 => vec![opcode + 1, 0xd8],
                _ => vec![0x48, opcode + 1, 0xd8],
            };
            memory.write(BASE, &code).unwrap();
            uc.mem_write(BASE, &code).unwrap();
            for a in values {
                for b in values {
                    for carry in [0, 1] {
                        let mut cpu = Cpu64::new(BASE, STACK);
                        cpu.regs[0] = (0xa5a55a5ac3c33c3c & !mask) | a;
                        cpu.regs[3] = (0x123456789abcdef0 & !mask) | b;
                        cpu.rflags = 0x202 | carry;
                        uc.reg_write(U::RAX, cpu.regs[0]).unwrap();
                        uc.reg_write(U::RBX, cpu.regs[3]).unwrap();
                        uc.reg_write(U::RFLAGS, cpu.rflags).unwrap();
                        uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                        cpu.step(&mut memory).unwrap();
                        assert_eq!(
                            cpu.regs[0],
                            uc.reg_read(U::RAX).unwrap(),
                            "{bits} sub={subtract} a={a:x} b={b:x} carry={carry}"
                        );
                        assert_eq!(
                            cpu.rflags & 0x8d5,
                            uc.reg_read(U::RFLAGS).unwrap() & 0x8d5,
                            "{bits} sub={subtract} a={a:x} b={b:x} carry={carry}"
                        );
                    }
                }
            }
        }
    }
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
