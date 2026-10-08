//! One-element string execution keeps the public step/fault/restart contract.
//! The store is checked before any of RCX/RSI/RDI/RIP are committed.
use super::*;
const REPEATED: u8 = 1;
const ADDRESS_32: u8 = 2;
const MOVES: u8 = 4;
const SOURCE_GS: u8 = 8;

pub(super) fn supports(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Stosb | M::Stosw | M::Stosd | M::Stosq | M::Movsb | M::Movsw | M::Movsq
    ) || i.mnemonic() == M::Movsd
        && matches!(i.op0_kind(), OpKind::MemoryESRDI | OpKind::MemoryESEDI)
}

pub(super) fn lower(i: &Instruction, mut ir: ScalarIr) -> Option<ScalarIr> {
    if i.has_lock_prefix() || i.segment_prefix() == R::FS {
        return None;
    }
    let m = i.mnemonic();
    let bits = match m {
        M::Stosb | M::Movsb => 8,
        M::Stosw | M::Movsw => 16,
        M::Stosd | M::Movsd => 32,
        _ => 64,
    };
    ir.opcode = (STRING << 2) | ScalarIr::type_code(bits)?;
    if i.has_rep_prefix() || i.has_repne_prefix() {
        ir.aux |= REPEATED;
    }
    if i.op0_kind() == OpKind::MemoryESEDI {
        ir.aux |= ADDRESS_32;
    }
    if matches!(m, M::Movsb | M::Movsw | M::Movsd | M::Movsq) {
        ir.aux |= MOVES;
    }
    if i.segment_prefix() == R::GS {
        ir.aux |= SOURCE_GS;
    }
    Some(ir)
}

impl ScalarIr {
    pub(super) fn execute_string_bounded(
        &self,
        cpu: &mut Cpu64,
        mem: &mut Memory,
        limit: u64,
    ) -> Result<u64, String> {
        let size = 1usize << (self.opcode & 3);
        let address32 = self.aux & ADDRESS_32 != 0;
        let repeated = self.aux & REPEATED != 0;
        let moves = self.aux & MOVES != 0;
        let truncate = |v: u64| if address32 { v as u32 as u64 } else { v };
        let count = truncate(cpu.regs[1]);
        let mut elements = 1;
        if !repeated || count != 0 {
            let dest = truncate(cpu.regs[7]);
            let source = moves.then(|| {
                truncate(cpu.regs[6]).wrapping_add(if self.aux & SOURCE_GS != 0 {
                    cpu.gs_base
                } else {
                    0
                })
            });
            let chunk = if repeated && !address32 && cpu.rflags & 1024 == 0 && limit > 1 {
                mem.try_string_chunk(dest, source, size, count.min(limit), cpu.regs[0])?
            } else {
                None
            };
            if let Some(completed) = chunk {
                elements = completed;
            } else {
                let value = if let Some(source) = source {
                    mem.read_int(source, size)
                } else {
                    Ok(cpu.regs[0])
                };
                value
                    .and_then(|value| mem.write_int(dest, size, value))
                    .map_err(|e| format!("{:#x} string IR opcode {}: {e}", cpu.rip, self.opcode))?;
            }
            let delta = if cpu.rflags & 1024 == 0 {
                elements * size as u64
            } else {
                0u64.wrapping_sub(size as u64)
            };
            cpu.regs[7] = truncate(cpu.regs[7].wrapping_add(delta));
            if moves {
                cpu.regs[6] = truncate(cpu.regs[6].wrapping_add(delta));
            }
            if repeated {
                cpu.regs[1] = count - elements;
                if count > elements {
                    cpu.work += elements;
                    return Ok(elements);
                }
            }
        }
        cpu.rip = cpu.rip.wrapping_add(self.len as u64);
        cpu.retired += 1;
        cpu.work += elements;
        Ok(elements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{EXECUTE, READ, WRITE};

    #[test]
    fn chunks_match_original_single_element_transactions_at_every_budget_and_fault() {
        const CODE: u64 = 0x140000000;
        const DATA: u64 = 0x200000;
        for size in [1, 2, 4, 8] {
            for moves in [false, true] {
                for address32 in [false, true] {
                    let mut code = vec![0xf3];
                    if address32 {
                        code.push(0x67);
                    }
                    if size == 2 {
                        code.push(0x66);
                    }
                    if size == 8 {
                        code.push(0x48);
                    }
                    code.push(if moves {
                        if size == 1 {
                            0xa4
                        } else {
                            0xa5
                        }
                    } else if size == 1 {
                        0xaa
                    } else {
                        0xab
                    });
                    let i =
                        iced_x86::Decoder::with_ip(64, &code, CODE, iced_x86::DecoderOptions::NONE)
                            .decode();
                    for (count, dest, source, backwards, protected, executable) in [
                        (0, u64::MAX, u64::MAX, false, false, false),
                        (1, DATA, DATA + 8192, false, false, false),
                        (7, DATA + 1, DATA + 8193, false, false, false),
                        (65, DATA, DATA + 8192, false, false, false),
                        (129, DATA, DATA + 8192, false, false, false),
                        (65, DATA + 4096 - size, DATA + 8192, false, true, false),
                        (65, DATA + 8192, DATA + 4096 - size, false, true, false),
                        (7, DATA + 32, DATA + 24, false, false, false),
                        (7, DATA + 24, DATA + 32, false, false, false),
                        (7, DATA + 128, DATA + 8192 + 128, true, false, false),
                        (65, DATA, DATA + 8192, false, false, true),
                    ] {
                        for budget in 0..=count + 2 {
                            let setup = || {
                                let mut mem = Memory::default();
                                mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
                                mem.write(CODE, &code).unwrap();
                                mem.map(
                                    DATA,
                                    16384,
                                    READ | WRITE | if executable { EXECUTE } else { 0 },
                                )
                                .unwrap();
                                let bytes: Vec<_> =
                                    (0..16384).map(|n| (n * 37 + 19) as u8).collect();
                                mem.write(DATA, &bytes).unwrap();
                                if protected {
                                    mem.virtual_protect(DATA + 4096, 4096, 0x104).unwrap();
                                }
                                mem
                            };
                            let mut fast_mem = setup();
                            let mut ref_mem = setup();
                            let mut fast = Cpu64::new(CODE, DATA + 16384);
                            fast.regs[0] = 0x1122334455667788;
                            fast.regs[1] = count;
                            fast.regs[6] = source;
                            fast.regs[7] = dest;
                            if address32 {
                                for n in [1, 6, 7] {
                                    fast.regs[n] |= 0xdeadbeef00000000;
                                }
                            }
                            if backwards {
                                fast.rflags |= 1024;
                            }
                            let mut reference = fast.clone();
                            let mut fast_fault = false;
                            let mut ref_fault = false;
                            while fast.work < budget && fast.rip == CODE {
                                if fast
                                    .step_bounded(&mut fast_mem, budget - fast.work)
                                    .is_err()
                                {
                                    fast_fault = true;
                                    break;
                                }
                            }
                            while reference.work < budget && reference.rip == CODE {
                                let mut next = reference.clone();
                                next.rip = i.next_ip();
                                match next.execute_string(&i, &mut ref_mem) {
                                    Err(_) => {
                                        ref_fault = true;
                                        break;
                                    }
                                    Ok(partial) => {
                                        next.retired += (!partial) as u64;
                                        next.work += 1;
                                        reference = next;
                                    }
                                }
                            }
                            assert_eq!(
                                fast_fault, ref_fault,
                                "size={size} count={count} budget={budget}"
                            );
                            assert_eq!(
                                fast, reference,
                                "size={size} count={count} budget={budget}"
                            );
                            assert_eq!(
                                fast_mem.virtual_query(DATA + 4096).unwrap(),
                                ref_mem.virtual_query(DATA + 4096).unwrap()
                            );
                            // Reading the guarded page for comparison consumes it on both sides.
                            fast_mem.virtual_protect(DATA + 4096, 4096, 4).unwrap();
                            ref_mem.virtual_protect(DATA + 4096, 4096, 4).unwrap();
                            let mut actual = [0; 16384];
                            let mut expected = [0; 16384];
                            fast_mem.read(DATA, &mut actual).unwrap();
                            ref_mem.read(DATA, &mut expected).unwrap();
                            assert_eq!(actual, expected);
                        }
                    }
                }
            }
        }
    }
}
