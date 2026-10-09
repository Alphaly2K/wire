//! Legacy, non-FP SSE only. Compute every source before committing the single
//! checked store/GPR/XMM destination. FLAGS, MXCSR and x87 remain untouched.
use super::*;

const XMM: u8 = 32;
const MOVE: u8 = VECTOR_FIRST;
const SCALAR_MOVE: u8 = MOVE + 1;
const TRANSFER: u8 = MOVE + 2;
const VAND: u8 = MOVE + 3;
const VANDN: u8 = MOVE + 4;
const VOR: u8 = MOVE + 5;
const VXOR: u8 = MOVE + 6;
const PADD: u8 = MOVE + 7;
const PSUB: u8 = MOVE + 8;
const EQUAL: u8 = MOVE + 9;
const GREATER: u8 = MOVE + 10;
const ALIGNED: u8 = 1;
const PRESERVE_HIGH: u8 = 2;

// Select lane width once at the cold vector dispatch boundary. Every copy
// below has a compile-time length; no lane builds a padded temporary buffer.
#[inline(always)]
fn packed<const OP: u8>(a: [u8; 16], b: [u8; 16], width: usize) -> [u8; 16] {
    macro_rules! lanes {
        ($unsigned:ty, $signed:ty, $bytes:literal) => {{
            let mut value = [0; 16];
            for offset in (0..16).step_by($bytes) {
                let a = <$unsigned>::from_le_bytes(a[offset..offset + $bytes].try_into().unwrap());
                let b = <$unsigned>::from_le_bytes(b[offset..offset + $bytes].try_into().unwrap());
                let result = match OP {
                    PADD => a.wrapping_add(b),
                    PSUB => a.wrapping_sub(b),
                    EQUAL => {
                        if a == b {
                            <$unsigned>::MAX
                        } else {
                            0
                        }
                    }
                    GREATER => {
                        if (a as $signed) > (b as $signed) {
                            <$unsigned>::MAX
                        } else {
                            0
                        }
                    }
                    _ => unreachable!(),
                };
                value[offset..offset + $bytes].copy_from_slice(&result.to_le_bytes());
            }
            value
        }};
    }
    match width {
        1 => lanes!(u8, i8, 1),
        2 => lanes!(u16, i16, 2),
        4 => lanes!(u32, i32, 4),
        8 => lanes!(u64, i64, 8),
        _ => unreachable!(),
    }
}

pub(super) fn supports(m: M) -> bool {
    matches!(
        m,
        M::Movups
            | M::Movaps
            | M::Movupd
            | M::Movapd
            | M::Movdqu
            | M::Movdqa
            | M::Movss
            | M::Movsd
            | M::Movd
            | M::Movq
            | M::Pand
            | M::Andps
            | M::Andpd
            | M::Pandn
            | M::Andnps
            | M::Andnpd
            | M::Por
            | M::Orps
            | M::Orpd
            | M::Pxor
            | M::Xorps
            | M::Xorpd
            | M::Paddb
            | M::Paddw
            | M::Paddd
            | M::Paddq
            | M::Psubb
            | M::Psubw
            | M::Psubd
            | M::Psubq
            | M::Pcmpeqb
            | M::Pcmpeqw
            | M::Pcmpeqd
            | M::Pcmpgtb
            | M::Pcmpgtw
            | M::Pcmpgtd
    )
}

pub(super) fn lower(i: &Instruction, mut ir: ScalarIr) -> Option<ScalarIr> {
    if i.has_lock_prefix() || i.has_rep_prefix() || i.has_repne_prefix() {
        return None;
    }
    let m = i.mnemonic();
    let op = match m {
        M::Movups | M::Movaps | M::Movupd | M::Movapd | M::Movdqu | M::Movdqa => MOVE,
        M::Movss | M::Movsd => SCALAR_MOVE,
        M::Movd | M::Movq => TRANSFER,
        M::Pand | M::Andps | M::Andpd => VAND,
        M::Pandn | M::Andnps | M::Andnpd => VANDN,
        M::Por | M::Orps | M::Orpd => VOR,
        M::Pxor | M::Xorps | M::Xorpd => VXOR,
        M::Paddb | M::Paddw | M::Paddd | M::Paddq => PADD,
        M::Psubb | M::Psubw | M::Psubd | M::Psubq => PSUB,
        M::Pcmpeqb | M::Pcmpeqw | M::Pcmpeqd => EQUAL,
        M::Pcmpgtb | M::Pcmpgtw | M::Pcmpgtd => GREATER,
        _ => return None,
    };
    ir.dst = ir.vector_operand(i, 0)?;
    ir.src = ir.vector_operand(i, 1)?;
    let vector = |r| (XMM..XMM + 16).contains(&r);
    if op == TRANSFER {
        if !vector(ir.dst) && !vector(ir.src) {
            return None;
        }
        // MOVD/Q never have two memory operands or a scalar constant source.
        if ir.dst == CONSTANT || ir.src == CONSTANT {
            return None;
        }
    } else if op <= SCALAR_MOVE {
        if !(vector(ir.dst) || ir.dst == MEMORY)
            || !(vector(ir.src) || ir.src == MEMORY)
            || ir.dst == MEMORY && ir.src == MEMORY
        {
            return None;
        }
    } else if !vector(ir.dst) || !(vector(ir.src) || ir.src == MEMORY) {
        return None;
    }
    let bits = match m {
        M::Paddb | M::Psubb | M::Pcmpeqb | M::Pcmpgtb => 8,
        M::Paddw | M::Psubw | M::Pcmpeqw | M::Pcmpgtw => 16,
        M::Paddd | M::Psubd | M::Pcmpeqd | M::Pcmpgtd | M::Movd | M::Movss => 32,
        _ => 64,
    };
    ir.opcode = (op << 2) | ScalarIr::type_code(bits)?;
    if matches!(m, M::Movaps | M::Movapd | M::Movdqa) || op > TRANSFER {
        ir.aux |= ALIGNED;
    }
    if op == SCALAR_MOVE && vector(ir.dst) && vector(ir.src) {
        ir.aux |= PRESERVE_HIGH;
    }
    Some(ir)
}

impl ScalarIr {
    pub(super) fn vector_operand(&mut self, i: &Instruction, op: u32) -> Option<u8> {
        if i.op_kind(op) == OpKind::Register && (R::XMM0..=R::XMM15).contains(&i.op_register(op)) {
            Some(XMM + (i.op_register(op) as u32 - R::XMM0 as u32) as u8)
        } else {
            self.operand(i, op)
        }
    }

    pub(super) fn vector_read_typed<const SIZE: usize>(
        &self,
        cpu: &Cpu64,
        r: u8,
        mem: &Memory,
    ) -> Result<[u8; 16], String> {
        let mut value = [0; 16];
        if r == MEMORY {
            mem.read(self.address(cpu), &mut value[..SIZE])?;
        } else if r >= XMM {
            value[..SIZE].copy_from_slice(&cpu.xmm[(r - XMM) as usize][..SIZE]);
        } else {
            value[..SIZE]
                .copy_from_slice(&self.read(cpu, r, (SIZE * 8) as u32, mem)?.to_le_bytes()[..SIZE]);
        }
        Ok(value)
    }

    #[inline(never)]
    pub(super) fn execute_vector(&self, cpu: &mut Cpu64, mem: &mut Memory) -> Result<(), String> {
        let op = self.opcode >> 2;
        let width = 1usize << (self.opcode & 3);
        if self.aux & ALIGNED != 0
            && (self.dst == MEMORY || self.src == MEMORY)
            && !self.address(cpu).is_multiple_of(16)
        {
            return Err(if op == MOVE {
                "unaligned AMD64 aligned vector access"
            } else if op >= PADD {
                "unaligned AMD64 packed integer access"
            } else {
                "unaligned AMD64 legacy vector ALU access"
            }
            .into());
        }
        let size = if matches!(op, SCALAR_MOVE | TRANSFER) {
            width
        } else {
            16
        };
        let b = match size {
            4 => self.vector_read_typed::<4>(cpu, self.src, mem)?,
            8 => self.vector_read_typed::<8>(cpu, self.src, mem)?,
            16 => self.vector_read_typed::<16>(cpu, self.src, mem)?,
            _ => unreachable!(),
        };
        let value = match op {
            MOVE | TRANSFER => b,
            SCALAR_MOVE => {
                let mut value = b;
                if self.aux & PRESERVE_HIGH != 0 {
                    let old = &cpu.xmm[(self.dst - XMM) as usize];
                    if size == 4 {
                        value[4..].copy_from_slice(&old[4..]);
                    } else {
                        value[8..].copy_from_slice(&old[8..]);
                    }
                }
                value
            }
            VAND | VANDN | VOR | VXOR => {
                let a = cpu.xmm[(self.dst - XMM) as usize];
                std::array::from_fn(|n| match op {
                    VAND => a[n] & b[n],
                    VANDN => !a[n] & b[n],
                    VOR => a[n] | b[n],
                    _ => a[n] ^ b[n],
                })
            }
            PADD => packed::<PADD>(cpu.xmm[(self.dst - XMM) as usize], b, width),
            PSUB => packed::<PSUB>(cpu.xmm[(self.dst - XMM) as usize], b, width),
            EQUAL => packed::<EQUAL>(cpu.xmm[(self.dst - XMM) as usize], b, width),
            GREATER => packed::<GREATER>(cpu.xmm[(self.dst - XMM) as usize], b, width),
            _ => unreachable!(),
        };
        // This is the only write and the last fallible action.
        if self.dst == MEMORY {
            mem.write(self.address(cpu), &value[..size])?;
        } else if self.dst >= XMM {
            cpu.xmm[(self.dst - XMM) as usize] = value;
        } else {
            self.write(
                cpu,
                u64::from_le_bytes(value[..8].try_into().unwrap()),
                (width * 8) as u32,
                mem,
            )?;
        }
        Ok(())
    }
}
