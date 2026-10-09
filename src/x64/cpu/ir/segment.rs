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
        // Verifier: no admitted operation reads RIP, and only the final
        // instruction can branch. Absolute RIP-relative operands were fixed
        // during lowering. A split budget uses single-instruction execution.
        cpu.rip = cpu.rip.wrapping_add(self.bytes);
        for (n, ir) in self.instructions.iter().enumerate() {
            let result = if !ir.is_scalar() {
                ir.execute_vector(cpu, &mut RegisterMemory)
            } else if self.update_flags & (1 << n) != 0 {
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
/// A short checked sequence commits each original instruction independently.
/// No FLAGS deletion is allowed across a possible data fault.
pub(crate) struct CheckedSegment {
    instructions: Box<[ScalarIr]>,
    pub(crate) bytes: u64,
}
impl CheckedSegment {
    pub(crate) fn lower(bytes: &[u8], pc: u64) -> Option<Self> {
        let mut decoder = Decoder::with_ip(64, bytes, pc, DecoderOptions::NONE);
        let mut instructions = Vec::with_capacity(8);
        let mut memory = false;
        while instructions.len() < 8 && decoder.can_decode() {
            let i = decoder.decode();
            if i.is_invalid() {
                break;
            }
            let Some(ir) = ScalarIr::lower(&i).filter(ScalarIr::checked_member) else {
                break;
            };
            memory |= ir.opcode >> 2 != LEA && (ir.dst == MEMORY || ir.src == MEMORY);
            let terminal = matches!(ir.opcode >> 2, JUMP | BRANCH);
            instructions.push(ir);
            if terminal {
                break;
            }
        }
        if !memory || instructions.len() < 4 {
            return None;
        }
        let bytes = instructions.iter().map(|ir| ir.len as u64).sum();
        Some(Self {
            instructions: instructions.into(),
            bytes,
        })
    }
    pub(crate) fn execute(
        &self,
        cpu: &mut Cpu64,
        mem: &mut Memory,
        remaining: u64,
    ) -> Result<(), String> {
        for ir in self.instructions.iter().take(remaining.min(8) as usize) {
            if !ir.checked_access(cpu, mem) {
                break;
            }
            // Reuse the same small per-instruction rollback and final commit.
            ir.step_scalar(cpu, mem)?;
        }
        Ok(())
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
            // Mixed high-XMM, fixed lanes, scalar transfers and FLAGS readers.
            vec![
                0x66, 0x45, 0x0f, 0xd4, 0xc1, 0x48, 0x01, 0xd8, 0x66, 0x45, 0x0f, 0x64, 0xd1, 0x66,
                0x49, 0x0f, 0x7e, 0xc2, 0x49, 0xff, 0xc1, 0x0f, 0x92, 0xc0, 0x66, 0x45, 0x0f, 0xef,
                0xc0, 0x66, 0x4d, 0x0f, 0x6e, 0xd2, 0x48, 0x11, 0xd9,
            ],
            vec![
                0x66, 0x0f, 0x6f, 0xc1, 0x66, 0x0f, 0xfb, 0xc0, 0x0f, 0x28, 0xc8, 0x48, 0x85, 0xc0,
                0x75, 0xf0,
            ],
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
                        for (n, value) in fast.xmm.iter_mut().enumerate() {
                            *value = seed
                                .wrapping_mul(0x123456789abcdef)
                                .rotate_left(n as u32)
                                .to_le_bytes()
                                .repeat(2)
                                .try_into()
                                .unwrap();
                        }
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
    #[test]
    fn checked_sequences_preserve_prefix_faults_and_every_budget() {
        const PC: u64 = 0x140000000;
        const DATA: u64 = 0x20000;
        // A false CMOV must still read its source after the successful prefix.
        let code = [
            0x48, 0x8b, 0x07, 0x48, 0x83, 0xc0, 3, 0x48, 0x89, 0x47, 8, 0x48, 0x39, 0x47, 16, 0x48,
            0x0f, 0x45, 0x0e, 0x48, 0x89, 0x4f, 24, 0x48, 0xff, 0xc2, 0x48, 0x39, 0xc1,
        ];
        let mut decoder = Decoder::with_ip(64, &code, PC, DecoderOptions::NONE);
        let mut entries = Vec::new();
        while decoder.can_decode() {
            entries.push(decoder.decode().ip());
        }
        for protection in [0x04u32, 0x02, 0x01, 0x104] {
            for entry in &entries {
                for budget in 0..=12 {
                    let setup = || {
                        let mut m = Memory::default();
                        m.map(PC, PAGE, READ | WRITE | EXECUTE).unwrap();
                        m.write(PC, &code).unwrap();
                        m.write(PC + code.len() as u64, &[0x0f, 0x0b]).unwrap();
                        m.map(DATA, PAGE * 2, READ | WRITE).unwrap();
                        m.write_int(DATA, 8, 4).unwrap();
                        m.write_int(DATA + 16, 8, 7).unwrap();
                        m.virtual_protect(DATA + PAGE, PAGE, protection).unwrap();
                        let mut cpu = Cpu64::new(*entry, 0);
                        cpu.regs[7] = DATA;
                        cpu.regs[6] = DATA + PAGE;
                        cpu.regs[0] = 7;
                        cpu.rflags = 0xad7;
                        cpu.decode(&m).unwrap();
                        (cpu, m)
                    };
                    let (mut fast, mut fm) = setup();
                    let (mut slow, mut sm) = setup();
                    let mut left = budget;
                    while left != 0 {
                        let before = fast.work;
                        let result = fast.step_bounded(&mut fm, left);
                        let work = fast.work - before;
                        assert!(work <= left);
                        for _ in 0..work {
                            slow.step(&mut sm).unwrap();
                        }
                        if result.is_err() {
                            assert!(slow.step(&mut sm).is_err());
                        }
                        assert_eq!(
                            fast, slow,
                            "protection={protection:x} entry={entry:x} budget={budget}"
                        );
                        let mut a = [0; 32];
                        let mut b = [0; 32];
                        fm.read(DATA, &mut a).unwrap();
                        sm.read(DATA, &mut b).unwrap();
                        assert_eq!(a, b);
                        if result.is_err() {
                            break;
                        }
                        assert!(work > 0);
                        left -= work;
                    }
                }
            }
        }
    }
    #[test]
    fn checked_sequences_exit_before_code_crossings_and_observed_pcs() {
        const PC: u64 = 0x140000000;
        const DATA: u64 = 0x20000;
        let code = [
            0x48, 0x8b, 0x07, 0x48, 0x83, 0xc0, 1, 0x48, 0x89, 0x06, 0x48, 0x8b, 0x1e, 0x48, 0x39,
            0xd8,
        ];
        for destination in [DATA + 8, DATA + PAGE - 4, PC + 10] {
            let setup = || {
                let mut m = Memory::default();
                m.map(PC, PAGE, READ | WRITE | EXECUTE).unwrap();
                m.write(PC, &code).unwrap();
                m.map(DATA, PAGE * 2, READ | WRITE).unwrap();
                m.write_int(DATA, 8, 7).unwrap();
                let mut cpu = Cpu64::new(PC, 0);
                cpu.regs[7] = DATA;
                cpu.regs[6] = destination;
                cpu.decode(&m).unwrap();
                (cpu, m)
            };
            let (mut fast, mut fm) = setup();
            let (mut slow, mut sm) = setup();
            fast.step_bounded(&mut fm, 5).unwrap();
            for _ in 0..fast.work {
                slow.step(&mut sm).unwrap();
            }
            assert_eq!(fast, slow);
            if destination != DATA + 8 {
                assert_eq!(fast.work, 2);
                assert_eq!(fast.rip, PC + 7);
            }
        }
        let mut m = Memory::default();
        m.map(PC, PAGE, READ | WRITE | EXECUTE).unwrap();
        m.write(PC, &code).unwrap();
        m.map(DATA, PAGE, READ | WRITE).unwrap();
        let mut cpu = Cpu64::new(PC, 0);
        cpu.regs[7] = DATA;
        cpu.regs[6] = DATA + 8;
        cpu.decode(&m).unwrap();
        cpu.step_with_budget_guarded(&mut m, 5, |start, end| !(start <= PC + 7 && PC + 7 < end))
            .unwrap();
        assert_eq!(cpu.work, 1);
        assert_eq!(cpu.rip, PC + 3);
    }
    #[test]
    fn warmed_checked_sequence_revalidates_code_guard_and_interior_patch() {
        const PC: u64 = 0x140000000;
        const DATA: u64 = 0x20000;
        let code = [
            0x48, 0x8b, 0x07, 0x48, 0x83, 0xc0, 1, 0x48, 0x89, 0x47, 8, 0x48, 0x39, 0xc3,
        ];
        let mut m = Memory::default();
        m.map(PC, PAGE, READ | WRITE | EXECUTE).unwrap();
        m.write(PC, &code).unwrap();
        m.map(DATA, PAGE, READ | WRITE).unwrap();
        m.write_int(DATA, 8, 3).unwrap();
        let mut cpu = Cpu64::new(PC, 0);
        cpu.regs[7] = DATA;
        cpu.decode(&m).unwrap();
        cpu.step_bounded(&mut m, 4).unwrap();
        assert_eq!(cpu.work, 4);
        cpu.rip = PC;
        m.virtual_protect(PC, PAGE, 0x140).unwrap();
        let before = cpu.clone();
        assert!(cpu.step_bounded(&mut m, 4).unwrap_err().contains("guard"));
        assert_eq!(cpu, before);
        cpu.step_bounded(&mut m, 4).unwrap();
        assert_eq!(cpu.work, 8);
        m.write(PC + 3, &[0x0f, 0x0b]).unwrap();
        cpu.rip = PC;
        cpu.step_bounded(&mut m, 4).unwrap();
        assert_eq!(cpu.rip, PC + 3);
        assert_eq!(cpu.work, 9);
        let before = cpu.clone();
        assert!(cpu.step_bounded(&mut m, 4).is_err());
        assert_eq!(cpu, before);
        m.virtual_protect(DATA, PAGE, 0x40).unwrap();
        m.virtual_protect(DATA, PAGE, 0x04).unwrap();
        assert!(!m.owned_noncode_access(DATA, 8));
    }
}
