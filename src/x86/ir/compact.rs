//! Eight-byte typed instructions for the common scalar register hot path.
//! Guest boundaries and fallback operands live outside this instruction word.
use super::{Op, Record};
use crate::x86::cpu::{AF, CF, OF, PF, SF, ZF};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Inst {
    pub bits: u32,
    pub aux: u32,
}
// Types are part of each opcode, never a separately decoded width field.
pub(crate) const NOP: u8 = 1;
pub(crate) const MOV_I32: u8 = 2;
pub(crate) const CONST_I32: u8 = 3;
pub(crate) const ADD_I32: u8 = 4;
pub(crate) const ADD_IMM_I32: u8 = 5;
pub(crate) const OR_I32: u8 = 6;
pub(crate) const OR_IMM_I32: u8 = 7;
pub(crate) const ADC_I32: u8 = 8;
pub(crate) const ADC_IMM_I32: u8 = 9;
pub(crate) const SBB_I32: u8 = 10;
pub(crate) const SBB_IMM_I32: u8 = 11;
pub(crate) const AND_I32: u8 = 12;
pub(crate) const AND_IMM_I32: u8 = 13;
pub(crate) const SUB_I32: u8 = 14;
pub(crate) const SUB_IMM_I32: u8 = 15;
pub(crate) const XOR_I32: u8 = 16;
pub(crate) const XOR_IMM_I32: u8 = 17;
pub(crate) const CMP_I32: u8 = 18;
pub(crate) const CMP_IMM_I32: u8 = 19;
pub(crate) const TEST_I32: u8 = 20;
pub(crate) const TEST_IMM_I32: u8 = 21;
pub(crate) const INC_I32: u8 = 22;
pub(crate) const DEC_I32: u8 = 23;
pub(crate) const JCC_BASE: u8 = 32; // Sixteen separate condition opcodes.
pub(crate) const JMP_I32: u8 = 48;
pub(crate) const LOAD_I32: u8 = 49;
pub(crate) const STORE_I32: u8 = 50;
pub(crate) const LEA_I32: u8 = 51;
pub(crate) const LOAD_GPR8: u8 = 52;
pub(crate) const LOAD_GPR16: u8 = 53;
pub(crate) const STORE_GPR8: u8 = 54;
pub(crate) const STORE_GPR16: u8 = 55;
pub(crate) const LOAD_U8_I32: u8 = 56;
pub(crate) const LOAD_U16_I32: u8 = 57;
pub(crate) const LOAD_S8_I32: u8 = 58;
pub(crate) const LOAD_S16_I32: u8 = 59;
// Runtime-only lowering of validated v3 records. These opcodes are never
// written into the persisted compact fields, so old AOT files stay valid.
pub(crate) const PUSH_I32: u8 = 60;
pub(crate) const PUSH_CONST_I32: u8 = 61;
pub(crate) const POP_I32: u8 = 62;
pub(crate) const CALL_CONST_I32: u8 = 63;
pub(crate) const CALL_I32: u8 = 64;
pub(crate) const JMP_REG_I32: u8 = 65;
pub(crate) const RET_I32: u8 = 66;
pub(crate) const LEAVE_I32: u8 = 67;
// The same twenty arithmetic forms, with the guest subregister type carried
// by the opcode. GPR8 retains x86's AH/CH/DH/BH numbering.
pub(crate) const ALU_GPR8_BASE: u8 = 68;
pub(crate) const ALU_GPR8_END: u8 = 87;
pub(crate) const ALU_GPR16_BASE: u8 = 88;
pub(crate) const ALU_GPR16_END: u8 = 107;
pub(crate) const MOV_GPR8: u8 = 108;
pub(crate) const MOV_GPR16: u8 = 109;
pub(crate) const CONST_GPR8: u8 = 110;
pub(crate) const CONST_GPR16: u8 = 111;
pub(crate) const MOV_U8_I32: u8 = 112;
pub(crate) const MOV_U16_I32: u8 = 113;
pub(crate) const MOV_S8_I32: u8 = 114;
pub(crate) const MOV_S16_I32: u8 = 115;
// Nine arithmetic/compare forms in each direction. Memory forms keep full
// FLAGS eagerly and reuse metadata for the checked address, like LOAD_I32.
pub(crate) const ALU_REG_MEM_I32_BASE: u8 = 116;
pub(crate) const ALU_MEM_REG_I32_BASE: u8 = 125;
pub(crate) const ALU_MEM_REG_I32_END: u8 = 133;

const FLAG_BITS: [u32; 6] = [CF, PF, AF, ZF, SF, OF];
impl Inst {
    pub fn opcode(self) -> u8 {
        self.bits as u8
    }
    /// Bound-cache slot, not part of the persisted instruction. Branches do
    /// not use register/FLAGS fields, so keep their link in those spare bits.
    pub fn branch_slot(self) -> u32 {
        self.bits >> 8
    }
    pub fn link_branch(&mut self, slot: u32) {
        if matches!(self.opcode(), JCC_BASE..=JMP_I32) && slot < 1 << 24 {
            self.bits |= slot << 8;
        }
    }
    // All current register-using hot opcodes operate on the eight x86 GPRs.
    // bind validates the complete six-bit fields against lowering first;
    // branch links (which reuse those bits) never use these accessors. Make
    // that proven range visible to Rust without unchecked array accesses.
    pub fn gpr_dst(self) -> usize {
        debug_assert_eq!((self.bits >> 8) & 63, (self.bits >> 8) & 7);
        ((self.bits >> 8) & 7) as usize
    }
    pub fn gpr_a(self) -> usize {
        debug_assert_eq!((self.bits >> 14) & 63, (self.bits >> 14) & 7);
        ((self.bits >> 14) & 7) as usize
    }
    pub fn gpr_b(self) -> usize {
        debug_assert_eq!((self.bits >> 20) & 63, (self.bits >> 20) & 7);
        ((self.bits >> 20) & 7) as usize
    }
    pub fn eager_flags(self) -> u32 {
        let packed = self.bits >> 26;
        (packed & 1)
            | ((packed & 2) << 1)
            | ((packed & 4) << 2)
            | ((packed & 8) << 3)
            | ((packed & 16) << 3)
            | ((packed & 32) << 6)
    }
    pub fn address_scale(self) -> u32 {
        (self.bits >> 26) & 3
    }
    pub fn address_segment(self) -> u32 {
        (self.bits >> 28) & 3
    }
    pub fn has_base(self) -> bool {
        self.bits & (1 << 30) != 0
    }
    pub fn has_index(self) -> bool {
        self.bits & (1 << 31) != 0
    }
    fn memory(op: u8, register: usize, address: super::Operand) -> Self {
        // Non-FLAGS opcodes reuse the top six bits for address metadata.
        // Presence bits avoid stealing a virtual register as a zero sentinel.
        Self {
            bits: op as u32
                | ((register as u32) << 8)
                | (((if address.base() == 15 {
                    0
                } else {
                    address.base()
                }) as u32)
                    << 14)
                | (((if address.index() == 15 {
                    0
                } else {
                    address.index()
                }) as u32)
                    << 20)
                | (address.scale() << 26)
                | (address.segment() << 28)
                | (u32::from(address.base() != 15) << 30)
                | (u32::from(address.index() != 15) << 31),
            aux: address.value,
        }
    }
    fn new(op: u8, dst: usize, a: usize, b: usize, flags: u32, aux: u32) -> Self {
        let packed = FLAG_BITS.iter().enumerate().fold(0, |n, (i, bit)| {
            n | if flags & bit != 0 { 1 << i } else { 0 }
        });
        Self {
            bits: op as u32
                | ((dst as u32) << 8)
                | ((a as u32) << 14)
                | ((b as u32) << 20)
                | (packed << 26),
            aux,
        }
    }
    pub fn lower(r: &Record) -> Option<Self> {
        let a = r.operand(0);
        let b = r.operand(1);
        let reg32 = |o: super::Operand| o.kind() == 1 && o.width() == 32;
        let (op, dst, src_a, src_b, aux) = match r.op() {
            Op::Nop => (NOP, 0, 0, 0, 0),
            Op::Mov if reg32(a) && reg32(b) => (MOV_I32, a.base(), b.base(), 0, 0),
            Op::Mov if reg32(a) && b.kind() == 2 => (CONST_I32, a.base(), 0, 0, b.value),
            Op::Mov if a.kind() == 1 && b.kind() == 3 => {
                let op = match a.width() {
                    8 => LOAD_GPR8,
                    16 => LOAD_GPR16,
                    32 => LOAD_I32,
                    _ => return None,
                };
                return Some(Self::memory(op, a.base(), b));
            }
            Op::Mov if a.kind() == 3 && b.kind() == 1 => {
                let op = match a.width() {
                    8 => STORE_GPR8,
                    16 => STORE_GPR16,
                    32 => STORE_I32,
                    _ => return None,
                };
                return Some(Self::memory(op, b.base(), a));
            }
            Op::Lea if reg32(a) && b.kind() == 3 => {
                return Some(Self::memory(LEA_I32, a.base(), b));
            }
            Op::Movzx | Op::Movsx if reg32(a) && b.kind() == 3 => {
                let op = match (r.op(), b.width()) {
                    (Op::Movzx, 8) => LOAD_U8_I32,
                    (Op::Movzx, 16) => LOAD_U16_I32,
                    (Op::Movsx, 8) => LOAD_S8_I32,
                    (Op::Movsx, 16) => LOAD_S16_I32,
                    _ => return None,
                };
                return Some(Self::memory(op, a.base(), b));
            }
            Op::Alu if r.width() == 32 && reg32(a) => {
                if r.sub() >= 9 {
                    (INC_I32 + (r.sub() - 9), a.base(), a.base(), 0, 1)
                } else if reg32(b) {
                    (ADD_I32 + r.sub() * 2, a.base(), a.base(), b.base(), 0)
                } else if b.kind() == 2 {
                    (ADD_IMM_I32 + r.sub() * 2, a.base(), a.base(), 0, b.value)
                } else {
                    return None;
                }
            }
            Op::Jcc => (JCC_BASE + r.sub(), 0, 0, 0, a.value),
            Op::Jmp if a.kind() == 2 => (JMP_I32, 0, 0, 0, a.value),
            _ => return None,
        };
        Some(Self::new(op, dst, src_a, src_b, r.eager_flags(), aux))
    }
    pub fn lower_runtime(r: &Record) -> Option<Self> {
        let a = r.operand(0);
        let b = r.operand(1);
        let reg32 = a.kind() == 1 && a.width() == 32;
        if r.op() == Op::Alu {
            if a.kind() == 1 && a.width() == r.width() && matches!(r.width(), 8 | 16) {
                let base = if r.width() == 8 {
                    ALU_GPR8_BASE
                } else {
                    ALU_GPR16_BASE
                };
                let (form, src, aux) = if r.sub() >= 9 {
                    (INC_I32 - ADD_I32 + r.sub() - 9, 0, 1)
                } else if b.kind() == 1 && b.width() == r.width() {
                    (r.sub() * 2, b.base(), 0)
                } else if b.kind() == 2 {
                    (r.sub() * 2 + 1, 0, b.value)
                } else {
                    return None;
                };
                return Some(Self::new(
                    base + form,
                    a.base(),
                    a.base(),
                    src,
                    r.eager_flags(),
                    aux,
                ));
            }
            if r.width() == 32
                && r.sub() <= 8
                && r.defs() == super::FLAGS
                && r.eager_flags() == super::FLAGS
            {
                if reg32 && b.kind() == 3 && b.width() == 32 {
                    return Some(Self::memory(ALU_REG_MEM_I32_BASE + r.sub(), a.base(), b));
                }
                if a.kind() == 3 && a.width() == 32 && b.kind() == 1 && b.width() == 32 {
                    return Some(Self::memory(ALU_MEM_REG_I32_BASE + r.sub(), b.base(), a));
                }
            }
        }
        let (op, register, aux) = match r.op() {
            Op::Mov
                if a.kind() == 1
                    && matches!(a.width(), 8 | 16)
                    && b.kind() == 1
                    && a.width() == b.width() =>
            {
                let op = if a.width() == 8 { MOV_GPR8 } else { MOV_GPR16 };
                return Some(Self::new(op, a.base(), b.base(), 0, 0, 0));
            }
            Op::Mov if a.kind() == 1 && matches!(a.width(), 8 | 16) && b.kind() == 2 => (
                if a.width() == 8 {
                    CONST_GPR8
                } else {
                    CONST_GPR16
                },
                a.base(),
                b.value,
            ),
            Op::Movzx | Op::Movsx if reg32 && b.kind() == 1 => {
                let op = match (r.op(), b.width()) {
                    (Op::Movzx, 8) => MOV_U8_I32,
                    (Op::Movzx, 16) => MOV_U16_I32,
                    (Op::Movsx, 8) => MOV_S8_I32,
                    (Op::Movsx, 16) => MOV_S16_I32,
                    _ => return None,
                };
                return Some(Self::new(op, a.base(), b.base(), 0, 0, 0));
            }
            Op::Push if r.width() == 32 && reg32 => (PUSH_I32, a.base(), 0),
            Op::Push if r.width() == 32 && a.kind() == 2 => (PUSH_CONST_I32, 0, a.value),
            Op::Pop if r.width() == 32 && reg32 => (POP_I32, a.base(), 0),
            Op::Call if a.kind() == 2 => (CALL_CONST_I32, 0, a.value),
            Op::Call if reg32 => (CALL_I32, a.base(), 0),
            Op::Jmp if reg32 => (JMP_REG_I32, a.base(), 0),
            Op::Ret => (RET_I32, 0, a.value),
            Op::Leave => (LEAVE_I32, 0, 0),
            _ => return None,
        };
        Some(Self::new(op, register, register, 0, 0, aux))
    }
}

/// Small hot entry: the instruction plus the next architectural boundary.
/// Opcode profiling metadata is cold and only read when explicitly enabled.
#[derive(Clone, Copy)]
pub(crate) struct Hot {
    pub inst: Inst,
    pub next_ip: u32,
    pub next_slot: u32,
}
impl Hot {
    // MAX_RECORDS fits well below 28 bits. The upper nibble records a
    // statically proven register-only run, without enlarging the hot entry.
    pub fn next_slot(self) -> u32 {
        self.next_slot & 0x0fff_ffff
    }
    pub fn mixed_run(self) -> bool {
        self.next_slot & 0x8000_0000 != 0
    }
    pub fn set_mixed_run(&mut self) {
        self.next_slot |= 0x8000_0000;
    }
    pub fn scalar_len(self) -> usize {
        ((self.next_slot >> 28) & 7) as usize + 1
    }
    pub fn set_scalar_len(&mut self, len: usize) {
        debug_assert!((1..=8).contains(&len));
        self.next_slot =
            (self.next_slot & 0x8000_0000) | self.next_slot() | (((len - 1) as u32) << 28);
    }
}

/// These operations cannot fault, access memory, enter the host or change
/// ESP. A branch may end a run, but cannot have a following operation in it.
pub(crate) fn scalar_member(i: Inst) -> bool {
    let op = i.opcode();
    match op {
        NOP | JCC_BASE..=JMP_I32 => true,
        CMP_I32..=TEST_IMM_I32 => true,
        MOV_GPR8 | CONST_GPR8 => true, // index 4 is AH, not ESP
        ALU_GPR8_BASE..=ALU_GPR8_END => true,
        ALU_GPR16_BASE..=ALU_GPR16_END => {
            let form = op - ALU_GPR16_BASE + ADD_I32;
            matches!(form, CMP_I32..=TEST_IMM_I32) || i.gpr_dst() != crate::x86::cpu::ESP
        }
        MOV_I32
        | CONST_I32
        | ADD_I32..=DEC_I32
        | LEA_I32
        | MOV_GPR16
        | CONST_GPR16
        | MOV_U8_I32..=MOV_S16_I32 => i.gpr_dst() != crate::x86::cpu::ESP,
        _ => false,
    }
}
