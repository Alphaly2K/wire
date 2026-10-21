//! Scalar SSE uses APFloat and a two-field transaction. A source fault occurs
//! before arithmetic; #XM commits sticky MXCSR, never the destination or RIP.
use super::*;
use crate::simd::SimdState;
use rustc_apfloat::ieee::{Double, Single};
use rustc_apfloat::{Float, Round};

pub(super) const FIRST: u8 = 44;
const ARITHMETIC: u8 = FIRST;
const CONVERT: u8 = FIRST + 1;
const TO_INTEGER: u8 = FIRST + 2;
const FROM_INTEGER: u8 = FIRST + 3;
const COMPARE: u8 = FIRST + 4;
const SQRT: u8 = FIRST + 5;
const PREDICATE: u8 = FIRST + 6;
const XMM: u8 = 32;

pub(super) fn supports(m: M) -> bool {
    matches!(
        m,
        M::Addss
            | M::Addsd
            | M::Subss
            | M::Subsd
            | M::Mulss
            | M::Mulsd
            | M::Divss
            | M::Divsd
            | M::Minss
            | M::Minsd
            | M::Maxss
            | M::Maxsd
            | M::Cvtss2sd
            | M::Cvtsd2ss
            | M::Cvtss2si
            | M::Cvttss2si
            | M::Cvtsd2si
            | M::Cvttsd2si
            | M::Cvtsi2ss
            | M::Cvtsi2sd
            | M::Comiss
            | M::Comisd
            | M::Ucomiss
            | M::Ucomisd
            | M::Sqrtss
            | M::Sqrtsd
            | M::Cmpss
            | M::Cmpsd
    )
}
pub(super) fn lower(i: &Instruction, mut ir: ScalarIr) -> Option<ScalarIr> {
    // F2/F3 are mandatory opcode prefixes, already validated by iced.
    if i.has_lock_prefix() {
        return None;
    }
    let m = i.mnemonic();
    let double = matches!(
        m,
        M::Addsd
            | M::Subsd
            | M::Mulsd
            | M::Divsd
            | M::Minsd
            | M::Maxsd
            | M::Cvtsd2ss
            | M::Cvtsd2si
            | M::Cvttsd2si
            | M::Comisd
            | M::Ucomisd
            | M::Sqrtsd
            | M::Cmpsd
    );
    let mut bits = if double { 64 } else { 32 };
    let op = match m {
        M::Cvtss2sd | M::Cvtsd2ss => CONVERT,
        M::Cvtss2si | M::Cvttss2si | M::Cvtsd2si | M::Cvttsd2si => {
            ir.aux = double as u8 | ((matches!(m, M::Cvttss2si | M::Cvttsd2si) as u8) << 1);
            bits = Cpu64::width(i, 0).ok()?;
            TO_INTEGER
        }
        M::Cvtsi2ss | M::Cvtsi2sd => {
            ir.aux = (m == M::Cvtsi2sd) as u8;
            bits = Cpu64::width(i, 1).ok()?;
            FROM_INTEGER
        }
        M::Comiss | M::Comisd | M::Ucomiss | M::Ucomisd => {
            ir.aux = matches!(m, M::Comiss | M::Comisd) as u8;
            COMPARE
        }
        M::Sqrtss | M::Sqrtsd => SQRT,
        M::Cmpss | M::Cmpsd => {
            ir.aux = i.immediate8();
            PREDICATE
        }
        _ => {
            ir.aux = match m {
                M::Addss | M::Addsd => 0,
                M::Subss | M::Subsd => 1,
                M::Mulss | M::Mulsd => 2,
                M::Divss | M::Divsd => 3,
                M::Minss | M::Minsd => 4,
                _ => 5,
            };
            ARITHMETIC
        }
    };
    if !matches!(bits, 32 | 64) {
        return None;
    }
    ir.dst = ir.vector_operand(i, 0)?;
    ir.src = ir.vector_operand(i, 1)?;
    if op == TO_INTEGER {
        if ir.dst >= 16 || !(matches!(ir.src, 32..=47) | (ir.src == MEMORY)) {
            return None;
        }
    } else {
        if !(32..=47).contains(&ir.dst) {
            return None;
        }
        if op == FROM_INTEGER {
            if !(ir.src < 16 || ir.src == MEMORY) {
                return None;
            }
        } else if !((32..=47).contains(&ir.src) || ir.src == MEMORY) {
            return None;
        }
    }
    ir.opcode = (op << 2) | ScalarIr::type_code(bits)?;
    ir.addressing |= CHECKED_SEGMENT;
    Some(ir)
}
impl ScalarIr {
    #[inline(never)]
    pub(super) fn execute_fp(&self, cpu: &mut Cpu64, mem: &Memory) -> Result<(), String> {
        let op = self.opcode >> 2;
        let double = self.opcode & 3 == 3;
        let source_double = if op == TO_INTEGER {
            self.aux & 1 != 0
        } else {
            double
        };
        let source = if op == FROM_INTEGER {
            self.read(cpu, self.src, if double { 64 } else { 32 }, mem)?
        } else if source_double {
            u64::from_le_bytes(
                self.vector_read_typed::<8>(cpu, self.src, mem)?[..8]
                    .try_into()
                    .unwrap(),
            )
        } else {
            u32::from_le_bytes(
                self.vector_read_typed::<4>(cpu, self.src, mem)?[..4]
                    .try_into()
                    .unwrap(),
            ) as u64
        };
        let mut state = SimdState {
            mxcsr: cpu.mxcsr,
            rflags: cpu.rflags,
        };
        let mut exception = false;
        let result = match op {
            ARITHMETIC | PREDICATE | COMPARE => {
                let a = if source_double {
                    u64::from_le_bytes(cpu.xmm[(self.dst - XMM) as usize][..8].try_into().unwrap())
                } else {
                    u32::from_le_bytes(cpu.xmm[(self.dst - XMM) as usize][..4].try_into().unwrap())
                        as u64
                };
                if op == COMPARE {
                    let m = if self.aux != 0 { M::Comiss } else { M::Ucomiss };
                    if double {
                        state.simd_compare::<Double>(m, a, source, &mut exception)
                    } else {
                        state.simd_compare::<Single>(m, a, source, &mut exception)
                    }
                    .map(|()| 0)
                } else if op == PREDICATE {
                    if double {
                        state.simd_predicate::<Double>(a, source, self.aux, &mut exception)
                    } else {
                        state.simd_predicate::<Single>(a, source, self.aux, &mut exception)
                    }
                    .map(|p| if p { u64::MAX } else { 0 })
                } else {
                    let m = match self.aux {
                        0 => M::Addss,
                        1 => M::Subss,
                        2 => M::Mulss,
                        3 => M::Divss,
                        4 => M::Minss,
                        _ => M::Maxss,
                    };
                    if double {
                        state.simd_arithmetic::<Double>(m, a, source, &mut exception)
                    } else {
                        state.simd_arithmetic::<Single>(m, a, source, &mut exception)
                    }
                }
            }
            CONVERT => {
                if double {
                    state.simd_convert::<Double, Single>(source, &mut exception)
                } else {
                    state.simd_convert::<Single, Double>(source, &mut exception)
                }
            }
            TO_INTEGER => {
                let round = if self.aux & 2 != 0 {
                    Round::TowardZero
                } else {
                    state.simd_round()
                };
                if source_double {
                    state.simd_to_integer::<Double>(
                        source,
                        if double { 64 } else { 32 },
                        round,
                        &mut exception,
                    )
                } else {
                    state.simd_to_integer::<Single>(
                        source,
                        if double { 64 } else { 32 },
                        round,
                        &mut exception,
                    )
                }
            }
            FROM_INTEGER => {
                let value = if double {
                    source as i64 as i128
                } else {
                    source as i32 as i128
                };
                let round = state.simd_round();
                if self.aux != 0 {
                    state
                        .simd_accept(Double::from_i128_r(value, round), 0, &mut exception)
                        .map(|v| v.to_bits() as u64)
                } else {
                    state
                        .simd_accept(Single::from_i128_r(value, round), 0, &mut exception)
                        .map(|v| v.to_bits() as u64)
                }
            }
            SQRT => {
                if double {
                    state.simd_sqrt::<Double>(source, 52, 1023, &mut exception)
                } else {
                    state.simd_sqrt::<Single>(source, 23, 127, &mut exception)
                }
            }
            _ => unreachable!(),
        };
        cpu.mxcsr = state.mxcsr;
        let value = result?;
        if op == COMPARE {
            cpu.rflags = state.rflags;
        } else if op == TO_INTEGER {
            cpu.regs[self.dst as usize] = if double { value } else { value as u32 as u64 };
        } else {
            let output_double = if op == CONVERT {
                !double
            } else if op == FROM_INTEGER {
                self.aux != 0
            } else {
                double
            };
            if output_double {
                cpu.xmm[(self.dst - XMM) as usize][..8].copy_from_slice(&value.to_le_bytes());
            } else {
                cpu.xmm[(self.dst - XMM) as usize][..4]
                    .copy_from_slice(&(value as u32).to_le_bytes());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{EXECUTE, PAGE, READ, WRITE};
    use iced_x86::{Decoder, DecoderOptions};
    #[test]
    fn scalar_fp_descriptors_match_checked_fallback_at_all_commit_boundaries() {
        const PC: u64 = 0x140000000;
        const DATA: u64 = 0x200000000;
        let mut codes = Vec::new();
        for prefix in [0xf2, 0xf3] {
            for op in [
                0x58, 0x59, 0x5c, 0x5e, 0x5d, 0x5f, 0x5a, 0x51, 0x2c, 0x2d, 0x2a,
            ] {
                for operand in [0xc1, 0x03] {
                    codes.push(vec![prefix, 0x0f, op, operand]);
                    if matches!(op, 0x2a | 0x2c | 0x2d) {
                        codes.push(vec![prefix, 0x48, 0x0f, op, operand]);
                    }
                }
            }
            for operand in [0xc1, 0x03] {
                for pred in 0..8 {
                    codes.push(vec![prefix, 0x0f, 0xc2, operand, pred]);
                }
            }
        }
        for prefix in [None, Some(0x66)] {
            for op in [0x2e, 0x2f] {
                for operand in [0xc1, 0x03] {
                    let mut code = prefix.into_iter().collect::<Vec<_>>();
                    code.extend([0x0f, op, operand]);
                    codes.push(code);
                }
            }
        }
        let singles: [u64; 14] = [
            0, 0x80000000, 1, 0x80000001, 0x007fffff, 0x00800000, 0x3f800000, 0xbf800000,
            0x3f800001, 0x7f7fffff, 0x7f800000, 0xff800000, 0x7fc12345, 0x7f812345,
        ];
        let doubles: [u64; 14] = [
            0,
            0x8000000000000000,
            1,
            0x8000000000000001,
            0x000fffffffffffff,
            0x0010000000000000,
            0x3ff0000000000000,
            0xbff0000000000000,
            0x3ff0000000000001,
            0x7fefffffffffffff,
            0x7ff0000000000000,
            0xfff0000000000000,
            0x7ff8123456789abc,
            0x7ff0123456789abc,
        ];
        let mut mem = Memory::default();
        mem.map(PC, PAGE, READ | WRITE | EXECUTE).unwrap();
        mem.map(DATA, PAGE, READ | WRITE).unwrap();
        for code in codes {
            let i = Decoder::with_ip(64, &code, PC, DecoderOptions::NONE).decode();
            let ir = ScalarIr::lower(&i).expect("every scalar FP form is lowered");
            let double = matches!(
                i.mnemonic(),
                M::Addsd
                    | M::Subsd
                    | M::Mulsd
                    | M::Divsd
                    | M::Minsd
                    | M::Maxsd
                    | M::Cvtsd2ss
                    | M::Cvttsd2si
                    | M::Cvtsd2si
                    | M::Sqrtsd
                    | M::Comisd
                    | M::Ucomisd
                    | M::Cmpsd
            );
            for round in 0..4 {
                for mode in [0, 0x40, 0x8000, 0x8040] {
                    for masks in [0x1f80, 0] {
                        for seed in 0..singles.len() {
                            let a = if double { doubles[seed] } else { singles[seed] };
                            let b = if double {
                                doubles[(seed + 3) % doubles.len()]
                            } else {
                                singles[(seed + 3) % singles.len()]
                            };
                            let mut cpu = Cpu64::new(PC, DATA + PAGE);
                            cpu.mxcsr = masks | mode | (round << 13) | 0x20;
                            cpu.rflags = 0xad7;
                            cpu.regs[1] = b;
                            cpu.regs[3] = DATA + PAGE - 8;
                            cpu.xmm[0] = [0xa5; 16];
                            cpu.xmm[0][..8].copy_from_slice(&a.to_le_bytes());
                            cpu.xmm[1] = [0x5a; 16];
                            cpu.xmm[1][..8].copy_from_slice(&b.to_le_bytes());
                            mem.write_int(DATA + PAGE - 8, 8, b).unwrap();
                            let before = cpu.clone();
                            let mut slow = cpu.clone();
                            slow.rip = i.next_ip();
                            let mut xm = false;
                            let reference = slow.execute(&i, &mut mem, &mut xm);
                            if reference.is_err() {
                                let mxcsr = slow.mxcsr;
                                slow = before.clone();
                                if xm {
                                    slow.mxcsr = mxcsr;
                                }
                            } else {
                                slow.retired += 1;
                                slow.work += 1;
                            }
                            let actual = ir.step(&mut cpu, &mut mem);
                            assert_eq!(
                                actual.is_ok(),
                                reference.is_ok(),
                                "{i:?} seed={seed} MXCSR={:x}",
                                before.mxcsr
                            );
                            assert_eq!(cpu, slow, "{i:?} seed={seed} MXCSR={:x}", before.mxcsr);
                        }
                    }
                }
            }
            if i.op1_kind() == OpKind::Memory {
                let mut cpu = Cpu64::new(PC, 0);
                cpu.mxcsr = 0;
                cpu.regs[3] = DATA + PAGE - 1;
                let before = cpu.clone();
                assert!(ir.step(&mut cpu, &mut mem).is_err());
                assert_eq!(cpu, before);
                cpu.regs[3] = DATA;
                mem.virtual_protect(DATA, PAGE, 0x104).unwrap();
                let before = cpu.clone();
                assert!(ir.step(&mut cpu, &mut mem).unwrap_err().contains("guard"));
                assert_eq!(cpu, before);
                mem.virtual_protect(DATA, PAGE, 0x04).unwrap();
            }
        }
    }
}
