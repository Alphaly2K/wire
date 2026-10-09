//! Same-page, four to sixteen register operations. Decode ahead cannot execute,
//! touch another page, consume a guard, or report a future guest fault.
use super::*;
use iced_x86::{Decoder, DecoderOptions};

const ARITHMETIC: u64 =
    CF | super::super::PF | super::super::AF | super::super::ZF | super::super::SF | OF;

pub(crate) struct RegisterSegment {
    instructions: Box<[ScalarIr]>,
    update_flags: u16,
    pub(crate) bytes: u64,
}
struct RegisterMemory;
impl IrMemory for RegisterMemory {
    const REGISTER_ONLY: bool = true;
    fn read_int(&self, _: u64, _: usize) -> Result<u64, String> {
        unreachable!("register segment memory read")
    }
    fn write_int(&mut self, _: u64, _: usize, _: u64) -> Result<(), String> {
        unreachable!("register segment memory write")
    }
}
impl RegisterSegment {
    pub(crate) fn lower(bytes: &[u8], pc: u64) -> Option<Self> {
        let mut decoder = Decoder::with_ip(64, bytes, pc, DecoderOptions::NONE);
        let mut instructions = Vec::with_capacity(16);
        while instructions.len() < 16 && decoder.can_decode() {
            let instruction = decoder.decode();
            if instruction.is_invalid() {
                break;
            }
            let Some(ir) = ScalarIr::lower(&instruction).filter(ScalarIr::register_only) else {
                break;
            };
            let terminal = matches!(ir.opcode >> 2, JUMP | BRANCH);
            instructions.push(ir);
            if terminal {
                break;
            }
        }
        // Tiny segments cost more to enter than their saved cache lookups.
        if instructions.len() < 4 {
            return None;
        }
        let mut needed = ARITHMETIC;
        let mut update_flags = 0;
        for (n, ir) in instructions.iter().enumerate().rev() {
            let (reads, writes) = ir.flag_dependencies();
            if writes & needed != 0 || ir.opcode >> 2 == SHIFT {
                update_flags |= 1 << n;
            }
            needed = (needed & !writes) | reads;
        }
        let bytes = instructions.iter().map(|ir| ir.len as u64).sum();
        Some(Self {
            instructions: instructions.into(),
            update_flags,
            bytes,
        })
    }
    pub(crate) fn work(&self) -> u64 {
        self.instructions.len() as u64
    }
    pub(crate) fn execute(&self, cpu: &mut Cpu64) {
        for (n, ir) in self.instructions.iter().enumerate() {
            cpu.rip = cpu.rip.wrapping_add(ir.len as u64);
            let result = if self.update_flags & (1 << n) != 0 {
                ir.execute::<_, true>(cpu, &mut RegisterMemory)
            } else {
                ir.execute::<_, false>(cpu, &mut RegisterMemory)
            };
            // The verifier excludes stack/memory/fallback operations. The
            // original executor is reused, avoiding a second ISA definition.
            result.expect("verified register instruction cannot fault");
        }
        cpu.retired += self.work();
        cpu.work += self.work();
    }
}
impl ScalarIr {
    fn flag_dependencies(&self) -> (u64, u64) {
        let op = self.opcode >> 2;
        let reads = match op {
            ADC | SBB => CF,
            BRANCH | CMOV | SET => ARITHMETIC,
            // Count zero and multi-bit shifts preserve some flags. Keep their
            // whole prior recipe live rather than guessing at a runtime count.
            SHIFT => ARITHMETIC,
            _ => 0,
        };
        let writes = match op {
            ADD | ADC | SUB | SBB | CMP | AND | OR | XOR | TEST | NEG => ARITHMETIC,
            INC | DEC => ARITHMETIC & !CF,
            IMUL => CF | OF,
            BIT_SCAN => super::super::ZF,
            BIT_TEST => CF,
            _ => 0,
        };
        (reads, writes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{EXECUTE, PAGE, READ, WRITE};
    #[test]
    fn every_budget_entry_and_flag_dependency_matches_single_transactional_execution() {
        const PC: u64 = 0x140000000;
        let programs: Vec<Vec<u8>> = vec![
            // ADC consumes ADD's carry; later CMP overwrites XOR/ADC flags.
            vec![
                0x48, 0x01, 0xd8, 0x48, 0x11, 0xd9, 0x49, 0x89, 0xc2, 0x4c, 0x31, 0xd1, 0x48, 0x8d,
                0x49, 3, 0x48, 0x39, 0xd8, 0x48, 0x0f, 0x42, 0xd0,
            ],
            // Partial writers preserve CF, then SETB observes it.
            vec![
                0x48, 0x29, 0xd8, 0x48, 0xff, 0xc1, 0x48, 0xff, 0xca, 0x0f, 0x92, 0xc0, 0x48, 0x11,
                0xd9,
            ],
            // IMUL writes only CF/OF; BSF writes only ZF; count-zero shift
            // preserves flags and its I32 write still clears the upper half.
            vec![
                0x4d, 0x0f, 0xaf, 0xc1, 0x48, 0x0f, 0xbc, 0xd3, 0xd3, 0xe0, 0x0f, 0x95, 0xc2, 0x48,
                0x98, 0x48, 0x99,
            ],
            // High-byte partial registers, extensions and 32-bit zeroing.
            vec![
                0xb4, 0x80, 0x66, 0x98, 0x98, 0x48, 0x98, 0x41, 0x0f, 0xc8, 0x45, 0x6b, 0xc1, 0x81,
            ],
            // A branch is the terminal instruction, with its flags live.
            vec![0x48, 0x31, 0xc0, 0x48, 0x85, 0xc0, 0x75, 0xf8],
            // Keep the flags writer and reader above mask bit seven and run
            // through the sixteen-instruction boundary at every budget.
            {
                let mut code = [0x48, 0x01, 0xd8].repeat(14);
                code.extend_from_slice(&[0x48, 0x39, 0xc8, 0x0f, 0x92, 0xc2, 0x48, 0x11, 0xd9]);
                code
            },
        ];
        for code in programs {
            // These programs only mutate registers. Reuse the checked memory
            // across seeds/budgets so large-cache builds do not repeatedly
            // allocate the whole translation table for identical code bytes.
            let setup = || {
                let mut m = Memory::default();
                m.map(PC, PAGE, READ | WRITE | EXECUTE).unwrap();
                m.write(PC, &code).unwrap();
                m.write(PC + code.len() as u64, &[0x0f, 0x0b]).unwrap();
                m
            };
            let mut fast_mem = setup();
            let mut slow_mem = setup();
            let mut decoder = Decoder::with_ip(64, &code, PC, DecoderOptions::NONE);
            let mut entries = vec![];
            while decoder.can_decode() {
                entries.push(decoder.decode().ip());
            }
            for entry in entries {
                for budget in 0..=20 {
                    for seed in 0..16u64 {
                        let mut fast = Cpu64::new(entry, 0);
                        fast.rflags = 0xad7;
                        for (n, value) in fast.regs.iter_mut().enumerate() {
                            *value = seed
                                .wrapping_mul(0x123456789abcdef)
                                .rotate_left(n as u32 * 3);
                        }
                        fast.regs[1] = seed;
                        let mut slow = fast.clone();
                        let mut left = budget;
                        // Install the entry without executing it; lowering ahead
                        // must not surface the terminal UD2 early.
                        fast.decode(&fast_mem).unwrap();
                        while left != 0 {
                            let before = fast.clone();
                            let result = fast.step_bounded(&mut fast_mem, left);
                            let work = match result {
                                Ok(work) => work,
                                Err(_) => {
                                    let i = slow.decode(&slow_mem).unwrap();
                                    let mut next = slow.clone();
                                    next.rip = i.next_ip();
                                    assert!(next.execute(&i, &mut slow_mem, &mut false).is_err());
                                    assert_eq!(fast, before);
                                    break;
                                }
                            };
                            assert!(work > 0 && work <= left);
                            for _ in 0..work {
                                let i = slow.decode(&slow_mem).unwrap();
                                let mut next = slow.clone();
                                next.rip = i.next_ip();
                                next.execute(&i, &mut slow_mem, &mut false).unwrap();
                                next.retired += 1;
                                next.work += 1;
                                slow = next;
                            }
                            assert_eq!(fast, slow, "entry={entry:x} budget={budget} seed={seed}");
                            left -= work;
                        }
                    }
                }
            }
        }
    }
}
