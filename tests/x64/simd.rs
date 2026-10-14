//! Existing behavioral tests grouped by simd; expectations remain independent of API catalogs.

use super::*;

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
fn amd64_sse_sqrt_scalar_and_packed_match_integer_rounding_and_preserve_scalar_upper_lanes() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    let mut seed = 0x922145327f013456u64;
    for (size, packed) in [(4, false), (8, false), (4, true), (8, true)] {
        let mut code = Vec::new();
        if packed {
            if size == 8 {
                code.push(0x66);
            }
        } else {
            code.push(if size == 4 { 0xf3 } else { 0xf2 });
        }
        code.extend([0x0f, 0x51, 0xc1]);
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
                    for lane in 0..if packed { 16 / size } else { 1 } {
                        let bits = if n <= 5 {
                            bits
                        } else {
                            bits.rotate_left(lane as u32 * 17)
                        };
                        cpu.xmm[1][lane * size..lane * size + size]
                            .copy_from_slice(&bits.to_le_bytes()[..size]);
                    }
                    uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                    uc.reg_write_long(U::XMM0, &cpu.xmm[0]).unwrap();
                    uc.reg_write_long(U::XMM1, &cpu.xmm[1]).unwrap();
                    uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
                    cpu.step(&mut mem).unwrap();
                    assert_eq!(
                        &*uc.reg_read_long(U::XMM0).unwrap(),
                        &cpu.xmm[0],
                        "size={size} packed={packed} bits={bits:x} round={rounding}"
                    );
                }
            }
        }
    }
}

#[test]
fn amd64_packed_sqrt_aggregates_exceptions_and_checks_memory_before_committing() {
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    for lane in [4, 8] {
        let mut code = Vec::new();
        if lane == 8 {
            code.push(0x66);
        }
        code.extend([0x0f, 0x51, 0xc1]);
        mem.write(BASE, &code).unwrap();
        let mut cpu = Cpu64::new(BASE, STACK);
        cpu.mxcsr &= !(1 << 7); // Unmask invalid, leave denormal masked.
        cpu.xmm[0] = [0x55; 16];
        let inputs: [u64; 4] = if lane == 4 {
            [0xbf800000, 2, 0x40800000, 0x41100000]
        } else {
            [0xbff0000000000000, 4, 0, 0]
        };
        for (n, bits) in inputs.into_iter().take(16 / lane).enumerate() {
            cpu.xmm[1][n * lane..n * lane + lane].copy_from_slice(&bits.to_le_bytes()[..lane]);
        }
        let before = cpu.clone();
        assert!(cpu.step(&mut mem).unwrap_err().contains("#XM"));
        assert_eq!(cpu.mxcsr, before.mxcsr | 3); // Both invalid and denormal lanes observed.
        assert_eq!(cpu.xmm, before.xmm);
        assert_eq!(cpu.rip, before.rip);
        assert_eq!(cpu.rflags, before.rflags);
        assert_eq!(cpu.retired, before.retired);
        // Memory form SQRTPS/PD xmm0,[rax]: m128 requires alignment and readable data.
        *code.last_mut().unwrap() = 0;
        mem.write(BASE, &code).unwrap();
        for address in [BASE + 0x101, BASE + 4096] {
            let mut cpu = before.clone();
            cpu.regs[0] = address;
            let before = cpu.clone();
            assert!(cpu.step(&mut mem).is_err());
            assert_eq!(cpu, before);
        }
        // The aligned memory form replaces every destination lane.
        cpu = before.clone();
        cpu.mxcsr |= 1 << 7;
        cpu.regs[0] = BASE + 0x100;
        mem.write(BASE + 0x100, &cpu.xmm[1]).unwrap();
        cpu.step(&mut mem).unwrap();
        assert_eq!(cpu.mxcsr & 0x3f, 3);
        assert_eq!(
            cpu.xmm[0][..lane],
            if lane == 4 {
                0xffc00000u64
            } else {
                0xfff8000000000000u64
            }
            .to_le_bytes()[..lane]
        );
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
fn amd64_sse_insert_extract_all_controls_and_operand_forms_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(BASE, 4096, Prot::ALL).unwrap();
    uc.mem_map(STACK, 4096, Prot::ALL).unwrap();
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.map(STACK, 4096, READ | WRITE).unwrap();
    let mut forms = vec![
        ("pextrw-sse2", vec![0x66, 0x45, 0x0f, 0xc5, 0xc1]),
        ("pinsrw", vec![0x66, 0x45, 0x0f, 0xc4, 0xc8]),
        ("pinsrw-m16", vec![0x66, 0x44, 0x0f, 0xc4, 0x09]),
    ];
    for (name, opcode, rex) in [
        ("pextrb", 0x14, 0x45),
        ("pextrb-rexw", 0x14, 0x4d),
        ("pextrw", 0x15, 0x45),
        ("pextrw-rexw", 0x15, 0x4d),
        ("pextrd", 0x16, 0x45),
        ("pextrq", 0x16, 0x4d),
        ("extractps", 0x17, 0x45),
        ("pinsrb", 0x20, 0x45),
        ("pinsrd", 0x22, 0x45),
        ("pinsrq", 0x22, 0x4d),
        ("insertps", 0x21, 0x45),
    ] {
        forms.push((name, vec![0x66, rex, 0x0f, 0x3a, opcode, 0xc8]));
        forms.push((name, vec![0x66, rex & !1, 0x0f, 0x3a, opcode, 0x09]));
    }
    // Source aliases the destination: selectors must use the original lanes.
    forms.push(("insertps-alias", vec![0x66, 0x45, 0x0f, 0x3a, 0x21, 0xc9]));
    for (name, mut code) in forms {
        code.push(0);
        for control in 0..=255u8 {
            *code.last_mut().unwrap() = control;
            mem.write(BASE, &code).unwrap();
            uc.mem_write(BASE, &code).unwrap();
            uc.ctl_remove_cache(BASE, BASE + 4096).unwrap();
            let mut cpu = Cpu64::new(BASE, STACK);
            cpu.regs[1] = STACK + 3;
            cpu.regs[8] = 0xfedcba9876543210;
            cpu.xmm[8] = [
                0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
                0xff, 0,
            ];
            cpu.xmm[9] = [
                0x80, 0x7f, 0xff, 0x7f, 1, 0, 0x80, 0xff, 0xff, 0xff, 0x81, 0x7f, 0, 0, 0, 0x80,
            ];
            cpu.rflags = 0xad7;
            cpu.mxcsr = 0xffbf;
            let initial = cpu.clone();
            let data: Vec<u8> = (0..32).map(|n| (n * 37 + 19) as u8).collect();
            mem.write(STACK, &data).unwrap();
            uc.mem_write(STACK, &data).unwrap();
            uc.reg_write(U::RCX, cpu.regs[1]).unwrap();
            uc.reg_write(U::R8, cpu.regs[8]).unwrap();
            uc.reg_write(U::RFLAGS, cpu.rflags).unwrap();
            uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
            uc.reg_write_long(U::XMM8, &cpu.xmm[8]).unwrap();
            uc.reg_write_long(U::XMM9, &cpu.xmm[9]).unwrap();
            uc.emu_start(BASE, BASE + code.len() as u64, 0, 1).unwrap();
            cpu.step(&mut mem).unwrap();
            let label = format!("{name} code={code:02x?} control={control:02x}");
            assert_eq!(cpu.regs[8], uc.reg_read(U::R8).unwrap(), "{label}");
            assert_eq!(&cpu.xmm[9], &*uc.reg_read_long(U::XMM9).unwrap(), "{label}");
            assert_eq!(cpu.xmm[8], initial.xmm[8], "{label}");
            assert_eq!(cpu.rflags, uc.reg_read(U::RFLAGS).unwrap(), "{label}");
            assert_eq!(cpu.mxcsr as u64, uc.reg_read(U::MXCSR).unwrap(), "{label}");
            let mut actual = [0; 32];
            mem.read(STACK, &mut actual).unwrap();
            assert_eq!(
                &actual,
                uc.mem_read_as_vec(STACK, 32).unwrap().as_slice(),
                "{label}"
            );
            assert_eq!(cpu.regs[1], initial.regs[1]);
            assert_eq!(cpu.rip, BASE + code.len() as u64);
            assert_eq!(cpu.retired, 1);
        }
    }
}

#[test]
fn amd64_sse_insert_extract_checked_widths_and_faults_preserve_state() {
    let mut mem = Memory::default();
    mem.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.map(STACK, 8192, READ | WRITE).unwrap();
    for (code, width, store) in [
        (vec![0x66, 0x44, 0x0f, 0x3a, 0x14, 0x09, 0xff], 1, true),
        (vec![0x66, 0x44, 0x0f, 0x3a, 0x15, 0x09, 0xff], 2, true),
        (vec![0x66, 0x44, 0x0f, 0x3a, 0x16, 0x09, 0xff], 4, true),
        (vec![0x66, 0x4c, 0x0f, 0x3a, 0x16, 0x09, 0xff], 8, true),
        (vec![0x66, 0x44, 0x0f, 0x3a, 0x17, 0x09, 0xff], 4, true),
        (vec![0x66, 0x44, 0x0f, 0x3a, 0x20, 0x09, 0xff], 1, false),
        (vec![0x66, 0x44, 0x0f, 0xc4, 0x09, 0xff], 2, false),
        (vec![0x66, 0x44, 0x0f, 0x3a, 0x22, 0x09, 0xff], 4, false),
        (vec![0x66, 0x4c, 0x0f, 0x3a, 0x22, 0x09, 0xff], 8, false),
        // All-zero INSERTPS still reads m32 and faults on inaccessible memory.
        (vec![0x66, 0x44, 0x0f, 0x3a, 0x21, 0x09, 0xff], 4, false),
    ] {
        mem.write(BASE, &code).unwrap();
        mem.protect(STACK + 4096, 4096, 0).unwrap();
        let mut cpu = Cpu64::new(BASE, STACK);
        cpu.regs[1] = STACK + 4096 - width;
        cpu.xmm[9] = [0xa5; 16];
        cpu.rflags = 0xad7;
        cpu.mxcsr = 0xffbf;
        cpu.step(&mut mem).unwrap(); // Exact-width access never touches the next page.
        for protection in [0x01, 0x02, 0x104] {
            // Read-only permits loads; NOACCESS and guard must fault on both paths.
            if !store && protection == 0x02 {
                continue;
            }
            mem.virtual_protect(STACK + 4096, 4096, protection).unwrap();
            let mut cpu = Cpu64::new(BASE, STACK);
            cpu.regs[1] = STACK + 4096 - width / 2;
            cpu.xmm[9] = [0x5a; 16];
            cpu.rflags = 0xad7;
            cpu.mxcsr = 0xffbf;
            let before = cpu.clone();
            let mut bytes = [0; 16];
            mem.read(STACK + 4080, &mut bytes).unwrap();
            assert!(
                cpu.step(&mut mem).is_err(),
                "code={code:02x?} protection={protection:x}"
            );
            assert_eq!(cpu, before);
            let mut after = [0; 16];
            mem.read(STACK + 4080, &mut after).unwrap();
            assert_eq!(after, bytes); // No partial store before a later page fault.
            if protection == 0x104 {
                cpu.step(&mut mem).unwrap(); // One-shot guard was consumed.
            }
        }
        mem.protect(STACK + 4096, 4096, READ | WRITE).unwrap();
    }
    // Unsupported MMX forms must not modify the aliased x87 state.
    for code in [[0x0f, 0xc5, 0xc1, 0xff], [0x0f, 0xc4, 0xc1, 0xff]] {
        mem.write(BASE, &code).unwrap();
        let mut cpu = Cpu64::new(BASE, STACK);
        let before = cpu.clone();
        assert!(cpu
            .step(&mut mem)
            .unwrap_err()
            .contains("unsupported AMD64 vector register"));
        assert_eq!(cpu, before);
    }
}
