//! Fixed 24-byte, typed integer/vector execution IR. This is host-independent data,
//! lowered from guest instructions, never host executable code or a Rust ABI
//! serialization. PE32's persisted .wir format is deliberately separate.
use super::{mask, reg, Cpu64, Instruction, Memory, OpKind, CF, M, OF, R};
mod fp;
mod segment;
mod string;
mod vector;
pub(crate) use segment::{CheckedSegment, RegisterSegment, Segment};

const MEMORY: u8 = 254;
const CONSTANT: u8 = 255;
const NO_REGISTER: u8 = 255;
const ADDRESS32: u8 = 4;
const GS: u8 = 8;
const CHECKED_SEGMENT: u8 = 64;
const REGISTER_ONLY: u8 = 128;

trait IrMemory {
    const REGISTER_ONLY: bool = false;
    fn read_int(&self, address: u64, size: usize) -> Result<u64, String>;
    fn read_bytes(&self, _: u64, _: &mut [u8]) -> Result<(), String> {
        unreachable!("register segment byte read")
    }
    fn write_bytes(&mut self, _: u64, _: &[u8]) -> Result<(), String> {
        unreachable!("register segment byte write")
    }

    fn write_int(&mut self, address: u64, size: usize, value: u64) -> Result<(), String>;
    fn modify_int(
        &mut self,
        address: u64,
        size: usize,
        update: impl FnOnce(u64) -> u64,
    ) -> Result<(), String> {
        let value = self.read_int(address, size)?;
        self.write_int(address, size, update(value))
    }
}
impl IrMemory for Memory {
    fn read_bytes(&self, address: u64, out: &mut [u8]) -> Result<(), String> {
        Memory::read(self, address, out)
    }
    fn write_bytes(&mut self, address: u64, bytes: &[u8]) -> Result<(), String> {
        Memory::write(self, address, bytes)
    }
    fn read_int(&self, address: u64, size: usize) -> Result<u64, String> {
        Memory::read_int(self, address, size)
    }
    fn write_int(&mut self, address: u64, size: usize, value: u64) -> Result<(), String> {
        Memory::write_int(self, address, size, value)
    }
    fn modify_int(
        &mut self,
        address: u64,
        size: usize,
        update: impl FnOnce(u64) -> u64,
    ) -> Result<(), String> {
        Memory::modify_int(self, address, size, update)
    }
}

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
const SHIFT: u8 = 26;
const IMUL: u8 = 27;
const EXTEND_ACC: u8 = 28;
const BSWAP: u8 = 29;
const BIT_SCAN: u8 = 30;
const BIT_TEST: u8 = 31;
const VECTOR_FIRST: u8 = 32;
const STRING: u8 = 43;

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
    #[inline(always)]
    pub(crate) fn is_scalar(&self) -> bool {
        self.opcode & 128 == 0
    }
    #[inline(always)]
    pub(super) fn step_bounded(
        &self,
        cpu: &mut Cpu64,
        mem: &mut Memory,
        limit: u64,
    ) -> Result<u64, String> {
        if self.opcode & 128 != 0 {
            mem.diagnostic_operands(self.dst == MEMORY || self.src == MEMORY);
            if self.opcode >> 2 == STRING {
                return self.execute_string_bounded(cpu, mem, limit);
            }
            if self.opcode >> 2 >= fp::FIRST {
                self.execute_fp(cpu, mem)?;
            } else {
                self.execute_vector_checked(cpu, mem)
                    .map_err(|e| format!("{:#x} vector IR opcode {}: {e}", cpu.rip, self.opcode))?;
            }
            cpu.rip = cpu.rip.wrapping_add(self.len as u64);
            cpu.retired += 1;
            cpu.work += 1;
        } else {
            self.step_scalar(cpu, mem)?;
        }
        Ok(1)
    }
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
        if string::supports(i) {
            return string::lower(i, ir);
        }
        if fp::supports(m) {
            return fp::lower(i, ir);
        }
        if vector::supports(m) {
            return vector::lower(i, ir);
        }
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
            M::Imul if matches!(i.op_count(), 2 | 3) => {
                ir.aux = (i.op_count() == 3) as u8;
                IMUL
            }
            M::Cbw | M::Cwde | M::Cdqe | M::Cwd | M::Cdq | M::Cqo => EXTEND_ACC,
            M::Bswap => BSWAP,
            M::Bt | M::Bts | M::Btr | M::Btc => {
                ir.aux = match m {
                    M::Bt => 0,
                    M::Bts => 1,
                    M::Btr => 2,
                    _ => 3,
                };
                BIT_TEST
            }
            M::Bsf | M::Bsr => {
                ir.aux = (m == M::Bsr) as u8;
                BIT_SCAN
            }
            M::Shl | M::Sal | M::Shr | M::Sar | M::Rol | M::Ror => {
                ir.aux = match m {
                    M::Shr => 1,
                    M::Sar => 2,
                    M::Rol => 3,
                    M::Ror => 4,
                    _ => 0,
                };
                SHIFT
            }
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
            ) && !(op == BIT_TEST && ir.aux != 0)
                || i.op0_kind() != OpKind::Memory)
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
            EXTEND_ACC => match m {
                M::Cbw | M::Cwd => 16,
                M::Cwde | M::Cdq => 32,
                _ => 64,
            },
            _ => Cpu64::width(i, 0).ok()?,
        };
        ir.opcode = (op << 2) | Self::type_code(bits)?;
        if op == BIT_TEST && !matches!(bits, 16 | 32 | 64) {
            return None;
        }
        if op == BSWAP && !matches!(bits, 32 | 64) {
            return None; // Preserve the checked fallback's invalid-width error.
        }
        if op == EXTEND_ACC {
            let high = matches!(m, M::Cwd | M::Cdq | M::Cqo);
            ir.dst = if high { 2 } else { 0 };
            ir.src = 0;
            ir.aux =
                Self::type_code(if high { bits } else { bits / 2 })? | if high { 4 } else { 0 };
            ir.addressing |= REGISTER_ONLY;
            return Some(ir);
        }
        if op == NOP {
            ir.addressing |= REGISTER_ONLY;
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
        if op == IMUL && i.op_count() == 3 && ir.operand(i, 2)? != CONSTANT {
            return None;
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
                | SHIFT
                | BIT_TEST
        ) && ir.dst == CONSTANT
            || matches!(
                op,
                LEA | CMOV | ZERO_EXTEND | SIGN_EXTEND | IMUL | BSWAP | BIT_SCAN
            ) && ir.dst >= MEMORY
        {
            return None;
        }
        if ir.register_safe() {
            ir.addressing |= REGISTER_ONLY;
        }
        if ir.checked_member() {
            ir.addressing |= CHECKED_SEGMENT;
        }
        Some(ir)
    }

    fn register_safe(&self) -> bool {
        let op = self.opcode >> 2;
        (self.is_scalar() || (VECTOR_FIRST..STRING).contains(&op))
            && self.dst != MEMORY
            && (self.src != MEMORY || op == LEA)
            && !matches!(op, PUSH | POP | CALL | RETURN)
    }
    #[inline(always)]
    pub(crate) fn register_only(&self) -> bool {
        self.addressing & REGISTER_ONLY != 0
    }
    pub(crate) fn without_segment(mut self) -> Self {
        self.addressing &= !REGISTER_ONLY;
        self
    }

    #[inline(always)]
    pub(crate) fn checked_segment(&self) -> bool {
        self.addressing & CHECKED_SEGMENT != 0
    }
    pub(crate) fn without_checked_segment(mut self) -> Self {
        self.addressing &= !CHECKED_SEGMENT;
        self
    }
    fn checked_member(&self) -> bool {
        matches!(
            self.opcode >> 2,
            NOP | MOV
                | LEA
                | ADD
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
                | CMOV
                | SET
                | BRANCH
        ) || self.opcode >> 2 == JUMP && self.dst != MEMORY
    }
    // Inspect only this instruction's access. Permissions and guard effects
    // remain in the ordinary checked executor, in guest execution order.
    fn checked_access(&self, cpu: &Cpu64, mem: &Memory) -> bool {
        if self.opcode >> 2 == LEA || self.dst != MEMORY && self.src != MEMORY {
            return true;
        }
        mem.owned_noncode_access(self.address(cpu), 1usize << (self.opcode & 3))
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

    #[inline(always)]
    fn read(
        &self,
        cpu: &Cpu64,
        operand: u8,
        bits: u32,
        mem: &impl IrMemory,
    ) -> Result<u64, String> {
        Ok(match operand {
            MEMORY => mem.read_int(self.address(cpu), (bits / 8) as usize)?,
            CONSTANT => self.immediate,
            r => (cpu.regs[(r & 15) as usize] >> if r & 16 != 0 { 8 } else { 0 }) & mask(bits),
        })
    }

    #[inline(always)]
    fn write(
        &self,
        cpu: &mut Cpu64,
        value: u64,
        bits: u32,
        mem: &mut impl IrMemory,
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

    /// Each opcode has at most one store,
    /// and that checked store is its last fallible effect. Scalar rollback
    /// therefore preserves the original step's architectural fault boundary.
    #[inline(always)]
    pub(super) fn step(&self, cpu: &mut Cpu64, mem: &mut Memory) -> Result<(), String> {
        self.step_bounded(cpu, mem, 1).map(|_| ())
    }
    #[inline(always)]
    pub(super) fn step_scalar(&self, cpu: &mut Cpu64, mem: &mut Memory) -> Result<(), String> {
        mem.diagnostic_operands(self.dst == MEMORY || self.src == MEMORY);
        // A scalar opcode can only write its explicit destination and RSP.
        // Memory/constant selectors still produce an in-range, untouched GPR.
        // Save both before execution: POP RSP can alias them, and POP [RSP]
        // changes the stack pointer before its last fallible store.
        let dst = (self.dst & 15) as usize;
        let dst_value = cpu.regs[dst];
        let rsp = cpu.regs[4];
        let flags = cpu.rflags;
        let rip = cpu.rip;
        cpu.rip = rip.wrapping_add(self.len as u64);
        if let Err(e) = self.execute::<_, true>(cpu, mem) {
            cpu.regs[dst] = dst_value;
            cpu.regs[4] = rsp;
            cpu.rflags = flags;
            cpu.rip = rip;
            return Err(format!("{rip:#x} scalar IR opcode {}: {e}", self.opcode));
        }
        cpu.retired += 1;
        cpu.work += 1;
        Ok(())
    }

    #[inline(always)]
    fn execute<T: IrMemory, const UPDATE_FLAGS: bool>(
        &self,
        cpu: &mut Cpu64,
        mem: &mut T,
    ) -> Result<(), String> {
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
                if self.dst == MEMORY && !matches!(op, CMP | TEST) {
                    let address = self.address(cpu);
                    // A lowered binary operation has at most one memory
                    // operand; the source read here cannot fault or change state.
                    let b = self.read(cpu, self.src, bits, mem)?;
                    mem.modify_int(address, (bits / 8) as usize, |a| {
                        Self::binary_result::<UPDATE_FLAGS>(cpu, op, bits, a, b)
                    })?;
                    return Ok(());
                }
                let a = self.read(cpu, self.dst, bits, mem)?;
                let b = self.read(cpu, self.src, bits, mem)?;
                let result = Self::binary_result::<UPDATE_FLAGS>(cpu, op, bits, a, b);
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
                if UPDATE_FLAGS && op != NOT {
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
            BIT_TEST => {
                let offset = self.read(cpu, self.src, bits, mem)?;
                let bit = offset as u32 & (bits - 1);
                let update = |a: u64| match self.aux {
                    1 => a | (1u64 << bit),
                    2 => a & !(1u64 << bit),
                    3 => a ^ (1u64 << bit),
                    _ => a,
                };
                if self.dst == MEMORY {
                    let mut address = self.address(cpu);
                    if self.src != CONSTANT {
                        let signed = super::signed(offset, bits) as i64;
                        address = address.wrapping_add(
                            (signed.div_euclid(bits as i64) * (bits / 8) as i64) as u64,
                        );
                    }
                    let mut carry = 0;
                    if self.aux == 0 {
                        carry = (mem.read_int(address, (bits / 8) as usize)? >> bit) & 1;
                    } else {
                        mem.modify_int(address, (bits / 8) as usize, |a| {
                            carry = (a >> bit) & 1;
                            update(a)
                        })?;
                    }
                    if UPDATE_FLAGS {
                        cpu.rflags = (cpu.rflags & !CF) | carry;
                    }
                } else {
                    let a = self.read(cpu, self.dst, bits, mem)?;
                    if self.aux != 0 {
                        self.write(cpu, update(a), bits, mem)?;
                    }
                    if UPDATE_FLAGS {
                        cpu.rflags = (cpu.rflags & !CF) | ((a >> bit) & 1);
                    }
                }
            }
            SHIFT | IMUL | EXTEND_ACC | BSWAP | BIT_SCAN => {
                if T::REGISTER_ONLY {
                    self.execute_integer_extended::<_, UPDATE_FLAGS>(cpu, mem)?;
                } else {
                    self.execute_integer_extended_checked::<_, UPDATE_FLAGS>(cpu, mem)?;
                }
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
    // Keep the old scalar dispatch compact while preserving one definition
    // of each added integer operation for scalar and register-segment paths.
    #[inline(never)]
    fn execute_integer_extended_checked<T: IrMemory, const UPDATE_FLAGS: bool>(
        &self,
        cpu: &mut Cpu64,
        mem: &mut T,
    ) -> Result<(), String> {
        self.execute_integer_extended::<_, UPDATE_FLAGS>(cpu, mem)
    }
    #[inline(always)]
    fn execute_integer_extended<T: IrMemory, const UPDATE_FLAGS: bool>(
        &self,
        cpu: &mut Cpu64,
        mem: &mut T,
    ) -> Result<(), String> {
        let op = self.opcode >> 2;
        let bits = 8 << (self.opcode & 3);
        match op {
            SHIFT => {
                let a = self.read(cpu, self.dst, bits, mem)?;
                let count =
                    (self.read(cpu, self.src, 8, mem)? & if bits == 64 { 63 } else { 31 }) as u32;
                let mut result = a;
                if count != 0 {
                    let sign = 1u64 << (bits - 1);
                    let (carry, overflow) = match self.aux {
                        3 | 4 => {
                            let n = count % bits;
                            if n != 0 {
                                result = if self.aux == 3 {
                                    (a << n | a >> (bits - n)) & mask(bits)
                                } else {
                                    (a >> n | a << (bits - n)) & mask(bits)
                                };
                            }
                            let c = if self.aux == 3 {
                                result & 1 != 0
                            } else {
                                result & sign != 0
                            };
                            let o = if self.aux == 3 {
                                (result & sign != 0) ^ c
                            } else {
                                (result & sign != 0) ^ (result & (sign >> 1) != 0)
                            };
                            (c, o)
                        }
                        0 => {
                            result = if count >= bits {
                                0
                            } else {
                                (a << count) & mask(bits)
                            };
                            let c = count <= bits && a & (1 << (bits - count)) != 0;
                            (c, (result & sign != 0) ^ c)
                        }
                        1 => {
                            result = if count >= bits { 0 } else { a >> count };
                            (count <= bits && a & (1 << (count - 1)) != 0, a & sign != 0)
                        }
                        2 => {
                            let negative = a & sign != 0;
                            result = ((super::signed(a, bits) as i64 >> count.min(bits - 1))
                                as u64)
                                & mask(bits);
                            let c = if count >= bits {
                                negative
                            } else {
                                a & (1 << (count - 1)) != 0
                            };
                            (c, false)
                        }
                        _ => unreachable!(),
                    };
                    if UPDATE_FLAGS {
                        cpu.rflags = (cpu.rflags & !CF) | carry as u64;
                    }
                    if UPDATE_FLAGS && count == 1 {
                        cpu.rflags = (cpu.rflags & !OF) | if overflow { OF } else { 0 };
                    }
                    if UPDATE_FLAGS && self.aux < 3 {
                        cpu.rflags = (cpu.rflags & !(super::SF | super::ZF | super::PF))
                            | if result & sign != 0 { super::SF } else { 0 }
                            | if result == 0 { super::ZF } else { 0 }
                            | if (result as u8).count_ones().is_multiple_of(2) {
                                super::PF
                            } else {
                                0
                            };
                    }
                }
                self.write(cpu, result, bits, mem)?;
            }
            IMUL => {
                let a = self.read(
                    cpu,
                    if self.aux != 0 { self.src } else { self.dst },
                    bits,
                    mem,
                )?;
                let b = self.read(
                    cpu,
                    if self.aux != 0 { CONSTANT } else { self.src },
                    bits,
                    mem,
                )?;
                let product = super::signed(a, bits) * super::signed(b, bits);
                let low = product as u64 & mask(bits);
                let overflow = product != super::signed(low, bits);
                if UPDATE_FLAGS {
                    cpu.rflags = (cpu.rflags & !(CF | OF)) | if overflow { CF | OF } else { 0 };
                }
                self.write(cpu, low, bits, mem)?;
            }
            EXTEND_ACC => {
                let value = super::signed(cpu.regs[0], 8 << (self.aux & 3));
                self.write(
                    cpu,
                    if self.aux & 4 != 0 {
                        if value < 0 {
                            mask(bits)
                        } else {
                            0
                        }
                    } else {
                        value as u64
                    },
                    bits,
                    mem,
                )?;
            }
            BSWAP => {
                let value = self.read(cpu, self.dst, bits, mem)?;
                self.write(
                    cpu,
                    if bits == 32 {
                        (value as u32).swap_bytes() as u64
                    } else {
                        value.swap_bytes()
                    },
                    bits,
                    mem,
                )?;
            }
            BIT_SCAN => {
                let value = self.read(cpu, self.src, bits, mem)?;
                if UPDATE_FLAGS {
                    cpu.rflags = (cpu.rflags & !super::ZF) | if value == 0 { super::ZF } else { 0 };
                }
                if value != 0 {
                    self.write(
                        cpu,
                        if self.aux == 0 {
                            value.trailing_zeros()
                        } else {
                            63 - value.leading_zeros()
                        } as u64,
                        bits,
                        mem,
                    )?;
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    #[inline(always)]
    fn binary_result<const UPDATE_FLAGS: bool>(
        cpu: &mut Cpu64,
        op: u8,
        bits: u32,
        a: u64,
        b: u64,
    ) -> u64 {
        let carry = matches!(op, ADC | SBB) && cpu.rflags & CF != 0;
        let sub = matches!(op, SUB | SBB | CMP);
        let result = match op {
            AND | TEST => a & b,
            OR => a | b,
            XOR => a ^ b,
            _ if sub => a.wrapping_sub(b).wrapping_sub(carry as u64),
            _ => a.wrapping_add(b).wrapping_add(carry as u64),
        };
        if UPDATE_FLAGS {
            if matches!(op, AND | OR | XOR | TEST) {
                cpu.flags(result, 0, result, bits, false, false);
                cpu.rflags &= !(CF | OF | super::AF);
            } else {
                cpu.flags(a, b, result, bits, sub, carry);
            }
        }
        result
    }
}

#[cfg(test)]
mod equivalence {
    use super::*;
    use crate::x64::memory::{EXECUTE, READ, WRITE};

    #[test]
    fn added_typed_operations_match_transactional_fallback_on_success_and_faults() {
        const CODE: u64 = 0x140000000;
        const DATA: u64 = 0x200000000;
        let mut instructions: Vec<Vec<u8>> = vec![
            vec![0x48, 0x01, 0x07],
            vec![0x48, 0x11, 0x07],
            vec![0x48, 0x29, 0x07],
            vec![0x48, 0x19, 0x07],
            vec![0x48, 0x21, 0x07],
            vec![0x48, 0x09, 0x07],
            vec![0x48, 0x31, 0x07],
            vec![0x4d, 0x0f, 0xaf, 0xc1],
            vec![0x45, 0x0f, 0xaf, 0xc1],
            vec![0x66, 0x45, 0x0f, 0xaf, 0xc1],
            vec![0x4c, 0x0f, 0xaf, 0x07],
            vec![0x44, 0x0f, 0xaf, 0x07],
            vec![0x66, 0x44, 0x0f, 0xaf, 0x07],
            vec![0x4c, 0x6b, 0x07, 0x81],
            vec![0x44, 0x69, 0x07, 1, 2, 3, 0x80],
            vec![0x66, 0x98],
            vec![0x98],
            vec![0x48, 0x98],
            vec![0x66, 0x99],
            vec![0x99],
            vec![0x48, 0x99],
            vec![0x49, 0x0f, 0xc8],
            vec![0x41, 0x0f, 0xc8],
            vec![0x4c, 0x0f, 0xbc, 0x07],
            vec![0x44, 0x0f, 0xbd, 0x07],
            vec![0x48, 0xd3, 0xe0],
            vec![0x66, 0xd3, 0xe8],
            vec![0xd2, 0xfc],
            vec![0x48, 0xd3, 0xc0],
            vec![0x48, 0xd3, 0xc8],
            vec![0x48, 0xd3, 0x27],
            vec![0x48, 0xd3, 0x2f],
            vec![0x48, 0xd3, 0x3f],
            vec![0x48, 0xd3, 0x07],
            vec![0x48, 0xd3, 0x0f],
            vec![0x66, 0x4c, 0x0f, 0x6e, 0xc0],
            vec![0x66, 0x4c, 0x0f, 0x7e, 0xc0],
            vec![0x66, 0x44, 0x0f, 0x6e, 0x07],
            vec![0x66, 0x44, 0x0f, 0x7e, 0x07],
            vec![0xf3, 0x44, 0x0f, 0x10, 0xc1],
            vec![0xf2, 0x44, 0x0f, 0x10, 0xc1],
            vec![0xf3, 0x44, 0x0f, 0x10, 0x07],
            vec![0xf2, 0x44, 0x0f, 0x10, 0x07],
            vec![0xf3, 0x44, 0x0f, 0x11, 0x07],
            vec![0xf2, 0x44, 0x0f, 0x11, 0x07],
            vec![0xf3, 0x44, 0x0f, 0x6f, 0x07],
            vec![0xf3, 0x44, 0x0f, 0x7f, 0x07],
            vec![0x66, 0x44, 0x0f, 0x6f, 0x07],
            vec![0x66, 0x44, 0x0f, 0x7f, 0x07],
        ];
        for op in [
            0xdb, 0xdf, 0xeb, 0xef, 0xfc, 0xfd, 0xfe, 0xd4, 0xf8, 0xf9, 0xfa, 0xfb, 0x74, 0x75,
            0x76, 0x64, 0x65, 0x66,
        ] {
            instructions.push(vec![0x66, 0x45, 0x0f, op, 0xc1]);
            instructions.push(vec![0x66, 0x44, 0x0f, op, 0x07]);
        }
        for code in instructions {
            for count in [0, 1, 8, 16, 31, 32, 63, 64, 255] {
                for (offset, permissions) in [
                    (0, READ | WRITE),
                    (1, READ | WRITE),
                    (4090, READ | WRITE),
                    (0, READ),
                    (4090, READ),
                ] {
                    let setup = || {
                        let mut mem = Memory::default();
                        mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
                        mem.write(CODE, &code).unwrap();
                        mem.map(DATA, 8192, READ | WRITE).unwrap();
                        mem.write(DATA, &[0xd5; 8192]).unwrap();
                        mem.protect(DATA + 4096, 4096, permissions).unwrap();
                        if offset == 0 {
                            mem.protect(DATA, 4096, permissions).unwrap();
                        }
                        mem
                    };
                    let mut fast_mem = setup();
                    let mut ref_mem = setup();
                    let mut fast = Cpu64::new(CODE, DATA + 8192);
                    fast.regs[0] = 0xdeadbeef87654321;
                    fast.regs[1] = count;
                    fast.regs[7] = DATA + offset;
                    fast.rflags = 0xad7;
                    fast.mxcsr = 0x9fc0;
                    for (n, xmm) in fast.xmm.iter_mut().enumerate() {
                        *xmm = std::array::from_fn(|j| (n * 37 + j * 19) as u8);
                    }
                    let mut reference = fast.clone();
                    let i = fast.decode(&fast_mem).unwrap();
                    let ir = ScalarIr::lower(&i).expect("fixture must lower");
                    let result = ir.step(&mut fast, &mut fast_mem);
                    let mut next = reference.clone();
                    next.rip = i.next_ip();
                    let mut xm = false;
                    let reference_result = next.execute(&i, &mut ref_mem, &mut xm);
                    if reference_result.is_ok() {
                        next.retired += 1;
                        next.work += 1;
                        reference = next;
                    }
                    assert_eq!(
                        result.is_ok(),
                        reference_result.is_ok(),
                        "{i:?} offset={offset}"
                    );
                    assert_eq!(fast, reference, "{i:?} count={count} offset={offset}");
                    let mut actual = [0; 8192];
                    let mut expected = [0; 8192];
                    fast_mem.read(DATA, &mut actual).unwrap();
                    ref_mem.read(DATA, &mut expected).unwrap();
                    assert_eq!(actual, expected, "{i:?} offset={offset}");
                }
            }
        }
    }
}
