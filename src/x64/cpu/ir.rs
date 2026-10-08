//! Fixed 24-byte, typed scalar execution IR. This is host-independent data,
//! lowered from guest instructions, never host executable code or a Rust ABI
//! serialization. PE32's persisted .wir format is deliberately separate.
use super::{mask, reg, Cpu64, Instruction, Memory, OpKind, CF, M, OF, R};

const MEMORY: u8 = 254;
const CONSTANT: u8 = 255;
const NO_REGISTER: u8 = 255;
const ADDRESS32: u8 = 4;
const GS: u8 = 8;

// The low two opcode bits encode I8/I16/I32/I64. Keep operations small and
// fuse effective-address calculation with the scalar operation.
const NOP: u8 = 0;
const MOV: u8 = 1;
const ZERO_EXTEND: u8 = 2;
const SIGN_EXTEND: u8 = 3;
const LEA: u8 = 4;
const ADD: u8 = 5;
const ADC: u8 = 6;
const SUB: u8 = 7;
const SBB: u8 = 8;
const CMP: u8 = 9;
const AND: u8 = 10;
const OR: u8 = 11;
const XOR: u8 = 12;
const TEST: u8 = 13;
const INC: u8 = 14;
const DEC: u8 = 15;
const NEG: u8 = 16;
const NOT: u8 = 17;
const JUMP: u8 = 18;
const BRANCH: u8 = 19;
const CMOV: u8 = 20;
const SET: u8 = 21;
const PUSH: u8 = 22;
const POP: u8 = 23;
const CALL: u8 = 24;
const RETURN: u8 = 25;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct ScalarIr {
    immediate: u64,
    displacement: u64,
    opcode: u8,
    pub(crate) len: u8,
    dst: u8,
    src: u8,
    base: u8,
    index: u8,
    addressing: u8,
    aux: u8,
}
const _: () = assert!(std::mem::size_of::<ScalarIr>() == 24);

fn condition(m: M) -> Option<u8> {
    Some(match m {
        M::Jo | M::Cmovo | M::Seto => 0,
        M::Jno | M::Cmovno | M::Setno => 1,
        M::Jb | M::Cmovb | M::Setb => 2,
        M::Jae | M::Cmovae | M::Setae => 3,
        M::Je | M::Cmove | M::Sete => 4,
        M::Jne | M::Cmovne | M::Setne => 5,
        M::Jbe | M::Cmovbe | M::Setbe => 6,
        M::Ja | M::Cmova | M::Seta => 7,
        M::Js | M::Cmovs | M::Sets => 8,
        M::Jns | M::Cmovns | M::Setns => 9,
        M::Jp | M::Cmovp | M::Setp => 10,
        M::Jnp | M::Cmovnp | M::Setnp => 11,
        M::Jl | M::Cmovl | M::Setl => 12,
        M::Jge | M::Cmovge | M::Setge => 13,
        M::Jle | M::Cmovle | M::Setle => 14,
        M::Jg | M::Cmovg | M::Setg => 15,
        _ => return None,
    })
}

impl ScalarIr {
    pub(crate) fn lower(i: &Instruction) -> Option<Self> {
        let mut ir = Self {
            immediate: 0,
            displacement: 0,
            opcode: 0,
            len: i.len() as u8,
            dst: CONSTANT,
            src: CONSTANT,
            base: NO_REGISTER,
            index: NO_REGISTER,
            addressing: 0,
            aux: 0,
        };
        let m = i.mnemonic();
        let op = match m {
            M::Nop | M::Endbr64 | M::Pause => NOP,
            M::Mov => MOV,
            M::Movzx => ZERO_EXTEND,
            M::Movsx | M::Movsxd => SIGN_EXTEND,
            M::Lea => LEA,
            M::Add => ADD,
            M::Adc => ADC,
            M::Sub => SUB,
            M::Sbb => SBB,
            M::Cmp => CMP,
            M::And => AND,
            M::Or => OR,
            M::Xor => XOR,
            M::Test => TEST,
            M::Inc => INC,
            M::Dec => DEC,
            M::Neg => NEG,
            M::Not => NOT,
            M::Jmp => JUMP,
            M::Push => PUSH,
            M::Pop => POP,
            M::Call => CALL,
            M::Ret => RETURN,
            _ => {
                ir.aux = condition(m)?;
                if i.op0_kind() == OpKind::NearBranch64 {
                    BRANCH
                } else if i.op_count() == 1 {
                    SET
                } else {
                    CMOV
                }
            }
        };
        if i.has_lock_prefix()
            && (!matches!(
                op,
                ADD | ADC | SUB | SBB | AND | OR | XOR | INC | DEC | NEG | NOT
            ) || i.op0_kind() != OpKind::Memory)
            || (i.has_rep_prefix() || i.has_repne_prefix()) && !matches!(m, M::Ret | M::Pause)
        {
            return None; // The checked fallback reports invalid/unsupported prefixes.
        }
        let bits = match op {
            NOP | RETURN | JUMP | BRANCH | CALL => 64,
            PUSH => {
                if i.stack_pointer_increment() == -2 {
                    16
                } else {
                    64
                }
            }
            LEA => (i.op0_register().size() * 8) as u32,
            _ => Cpu64::width(i, 0).ok()?,
        };
        ir.opcode = (op << 2) | Self::type_code(bits)?;
        if op == NOP {
            return Some(ir);
        }
        if op == RETURN {
            ir.immediate = if i.op_count() == 0 {
                0
            } else {
                i.immediate16() as u64
            };
            return Some(ir);
        }
        ir.dst = ir.operand(i, 0)?;
        if i.op_count() > 1 {
            ir.src = ir.operand(i, 1)?;
        }
        if matches!(op, ZERO_EXTEND | SIGN_EXTEND) {
            ir.aux = Self::type_code(Cpu64::width(i, 1).ok()?)?;
        }
        // Destinations must be scalar registers/memory; LEA/CMOV/extensions
        // specifically require a register. An iced vector/segment register
        // never passes operand(), even if its byte width matches a scalar.
        if matches!(
            op,
            MOV | ADD
                | ADC
                | SUB
                | SBB
                | CMP
                | AND
                | OR
                | XOR
                | TEST
                | INC
                | DEC
                | NEG
                | NOT
                | SET
                | POP
        ) && ir.dst == CONSTANT
            || matches!(op, LEA | CMOV | ZERO_EXTEND | SIGN_EXTEND) && ir.dst >= MEMORY
        {
            return None;
        }
        Some(ir)
    }

    fn type_code(bits: u32) -> Option<u8> {
        Some(match bits {
            8 => 0,
            16 => 1,
            32 => 2,
            64 => 3,
            _ => return None,
        })
    }

    fn operand(&mut self, i: &Instruction, op: u32) -> Option<u8> {
        Some(match i.op_kind(op) {
            OpKind::Register => {
                let (index, _, shift) = reg(i.op_register(op)).ok()?;
                index as u8 | if shift == 8 { 16 } else { 0 }
            }
            OpKind::Memory => {
                if i.segment_prefix() == R::FS && i.mnemonic() != M::Lea {
                    return None;
                }
                self.displacement = if i.is_ip_rel_memory_operand() {
                    i.ip_rel_memory_address()
                } else {
                    i.memory_displacement64()
                };
                if !i.is_ip_rel_memory_operand() {
                    for (r, field) in [
                        (i.memory_base(), &mut self.base),
                        (i.memory_index(), &mut self.index),
                    ] {
                        if r != R::None {
                            let (index, bits, _) = reg(r).ok()?;
                            *field = index as u8;
                            if bits == 32 {
                                self.addressing |= ADDRESS32;
                            }
                        }
                    }
                }
                self.addressing |= i.memory_index_scale().trailing_zeros() as u8;
                if i.segment_prefix() == R::GS && i.mnemonic() != M::Lea {
                    self.addressing |= GS;
                }
                MEMORY
            }
            kind => {
                self.immediate = match kind {
                    OpKind::Immediate8 => i.immediate8() as u64,
                    OpKind::Immediate16 => i.immediate16() as u64,
                    OpKind::Immediate32 => i.immediate32() as u64,
                    OpKind::Immediate64 => i.immediate64(),
                    OpKind::Immediate8to16 => i.immediate8to16() as u16 as u64,
                    OpKind::Immediate8to32 => i.immediate8to32() as u32 as u64,
                    OpKind::Immediate8to64 => i.immediate8to64() as u64,
                    OpKind::Immediate32to64 => i.immediate32to64() as u64,
                    OpKind::NearBranch64 => i.near_branch64(),
                    _ => return None,
                };
                CONSTANT
            }
        })
    }

    fn address(&self, cpu: &Cpu64) -> u64 {
        let base = if self.base == NO_REGISTER {
            0
        } else {
            cpu.regs[self.base as usize]
        };
        let index = if self.index == NO_REGISTER {
            0
        } else {
            cpu.regs[self.index as usize]
        };
        let mut address = base
            .wrapping_add(index << (self.addressing & 3))
            .wrapping_add(self.displacement);
        if self.addressing & ADDRESS32 != 0 {
            address = address as u32 as u64;
        }
        if self.addressing & GS != 0 {
            address = address.wrapping_add(cpu.gs_base);
        }
        address
    }

    fn read(&self, cpu: &Cpu64, operand: u8, bits: u32, mem: &Memory) -> Result<u64, String> {
        Ok(match operand {
            MEMORY => mem.read_int(self.address(cpu), (bits / 8) as usize)?,
            CONSTANT => self.immediate,
            r => (cpu.regs[(r & 15) as usize] >> if r & 16 != 0 { 8 } else { 0 }) & mask(bits),
        })
    }

    fn write(
        &self,
        cpu: &mut Cpu64,
        value: u64,
        bits: u32,
        mem: &mut Memory,
    ) -> Result<(), String> {
        if self.dst == MEMORY {
            mem.write_int(self.address(cpu), (bits / 8) as usize, value)
        } else {
            let r = &mut cpu.regs[(self.dst & 15) as usize];
            if bits >= 32 {
                *r = value & mask(bits);
            } else {
                let shift = if self.dst & 16 != 0 { 8 } else { 0 };
                *r = (*r & !(mask(bits) << shift)) | ((value & mask(bits)) << shift);
            }
            Ok(())
        }
    }

    fn taken(&self, cpu: &Cpu64) -> bool {
        let flags = cpu.rflags;
        let c = flags & CF != 0;
        let z = flags & super::ZF != 0;
        let s = flags & super::SF != 0;
        let o = flags & OF != 0;
        let p = flags & super::PF != 0;
        let yes = match self.aux >> 1 {
            0 => o,
            1 => c,
            2 => z,
            3 => c || z,
            4 => s,
            5 => p,
            6 => s != o,
            7 => z || s != o,
            _ => unreachable!(),
        };
        yes ^ (self.aux & 1 != 0)
    }

    /// No vector/x87 state is touched. Each opcode has at most one store,
    /// and that checked store is its last fallible effect. Scalar rollback
    /// therefore preserves the original step's architectural fault boundary.
    pub(super) fn step(&self, cpu: &mut Cpu64, mem: &mut Memory) -> Result<(), String> {
        let regs = cpu.regs;
        let flags = cpu.rflags;
        let rip = cpu.rip;
        cpu.rip = rip.wrapping_add(self.len as u64);
        if let Err(e) = self.execute(cpu, mem) {
            cpu.regs = regs;
            cpu.rflags = flags;
            cpu.rip = rip;
            return Err(format!("{rip:#x} scalar IR opcode {}: {e}", self.opcode));
        }
        cpu.retired += 1;
        cpu.work += 1;
        Ok(())
    }

    fn execute(&self, cpu: &mut Cpu64, mem: &mut Memory) -> Result<(), String> {
        let op = self.opcode >> 2;
        let bits = 8 << (self.opcode & 3);
        match op {
            NOP => {}
            MOV => {
                let value = self.read(cpu, self.src, bits, mem)?;
                self.write(cpu, value, bits, mem)?;
            }
            ZERO_EXTEND | SIGN_EXTEND => {
                let source_bits = 8 << self.aux;
                let value = self.read(cpu, self.src, source_bits, mem)?;
                let value = if op == SIGN_EXTEND {
                    super::signed(value, source_bits) as u64
                } else {
                    value
                };
                self.write(cpu, value, bits, mem)?;
            }
            LEA => self.write(cpu, self.address(cpu), bits, mem)?,
            ADD | ADC | SUB | SBB | CMP | AND | OR | XOR | TEST => {
                let a = self.read(cpu, self.dst, bits, mem)?;
                let b = self.read(cpu, self.src, bits, mem)?;
                let carry = matches!(op, ADC | SBB) && cpu.rflags & CF != 0;
                let sub = matches!(op, SUB | SBB | CMP);
                let result = match op {
                    AND | TEST => a & b,
                    OR => a | b,
                    XOR => a ^ b,
                    _ if sub => a.wrapping_sub(b).wrapping_sub(carry as u64),
                    _ => a.wrapping_add(b).wrapping_add(carry as u64),
                };
                if matches!(op, AND | OR | XOR | TEST) {
                    cpu.flags(result, 0, result, bits, false, false);
                    cpu.rflags &= !(CF | OF | super::AF);
                } else {
                    cpu.flags(a, b, result, bits, sub, carry);
                }
                if !matches!(op, CMP | TEST) {
                    self.write(cpu, result, bits, mem)?;
                }
            }
            INC | DEC | NEG | NOT => {
                let a = self.read(cpu, self.dst, bits, mem)?;
                let result = match op {
                    INC => a.wrapping_add(1),
                    DEC => a.wrapping_sub(1),
                    NEG => 0u64.wrapping_sub(a),
                    _ => !a,
                };
                if op != NOT {
                    let old_cf = cpu.rflags & CF;
                    if op == NEG {
                        cpu.flags(0, a, result, bits, true, false);
                    } else {
                        cpu.flags(a, 1, result, bits, op == DEC, false);
                        cpu.rflags = (cpu.rflags & !CF) | old_cf;
                    }
                }
                self.write(cpu, result, bits, mem)?;
            }
            JUMP => cpu.rip = self.read(cpu, self.dst, 64, mem)?,
            BRANCH => {
                if self.taken(cpu) {
                    cpu.rip = self.immediate;
                }
            }
            CMOV => {
                // A false CMOV still faults on its memory source and clears
                // upper bits of an I32 destination, matching the ISA.
                let source = self.read(cpu, self.src, bits, mem)?;
                let value = if self.taken(cpu) {
                    source
                } else {
                    self.read(cpu, self.dst, bits, mem)?
                };
                self.write(cpu, value, bits, mem)?;
            }
            SET => self.write(cpu, self.taken(cpu) as u64, bits, mem)?,
            PUSH | CALL => {
                let source = self.read(cpu, self.dst, bits, mem)?;
                let value = if op == CALL { cpu.rip } else { source };
                let rsp = cpu.regs[4]
                    .checked_sub((bits / 8) as u64)
                    .ok_or("AMD64 stack underflow")?;
                mem.write_int(rsp, (bits / 8) as usize, value)?;
                cpu.regs[4] = rsp;
                if op == CALL {
                    cpu.rip = source;
                }
            }
            POP => {
                let value = mem.read_int(cpu.regs[4], (bits / 8) as usize)?;
                cpu.regs[4] = cpu.regs[4]
                    .checked_add((bits / 8) as u64)
                    .ok_or("AMD64 stack overflow")?;
                // POP [RSP+...] computes its destination after the increment.
                self.write(cpu, value, bits, mem)?;
            }
            RETURN => {
                let target = mem.read_int(cpu.regs[4], 8)?;
                cpu.regs[4] = cpu.regs[4]
                    .checked_add(8 + self.immediate)
                    .ok_or("AMD64 stack overflow")?;
                cpu.rip = target;
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
