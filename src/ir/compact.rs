//! Eight-byte typed instructions for the common scalar register hot path.
//! Guest boundaries and fallback operands live outside this instruction word.
use super::{Op, Record};
use crate::cpu::{AF, CF, OF, PF, SF, ZF};

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

const FLAG_BITS: [u32; 6] = [CF, PF, AF, ZF, SF, OF];
impl Inst {
    pub fn opcode(self) -> u8 {
        self.bits as u8
    }
    pub fn dst(self) -> usize {
        ((self.bits >> 8) & 63) as usize
    }
    pub fn a(self) -> usize {
        ((self.bits >> 14) & 63) as usize
    }
    pub fn b(self) -> usize {
        ((self.bits >> 20) & 63) as usize
    }
    pub fn eager_flags(self) -> u32 {
        let packed = self.bits >> 26;
        FLAG_BITS.iter().enumerate().fold(0, |flags, (i, bit)| {
            flags | if packed & (1 << i) != 0 { *bit } else { 0 }
        })
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
}

/// Small hot entry: the instruction plus the next architectural boundary.
/// Opcode profiling metadata is cold and only read when explicitly enabled.
#[derive(Clone, Copy)]
pub(crate) struct Hot {
    pub inst: Inst,
    pub next_ip: u32,
}
