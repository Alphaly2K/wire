//! Checked AMD64 bring-up interpreter. Unsupported instructions fail at RIP.
//! All flags are computed eagerly; the PE32 FLAGS optimizer is not reused here.
use super::memory::Memory;
use iced_x86::{
    Decoder, DecoderError, DecoderOptions, Instruction, Mnemonic as M, OpKind, Register as R,
};
use rustc_apfloat::ieee::{Double, Single};
use rustc_apfloat::{Float, FloatConvert, Round, Status, StatusAnd};
mod x87;
const CF: u64 = 1;
const PF: u64 = 4;
const AF: u64 = 16;
const ZF: u64 = 64;
const SF: u64 = 128;
const OF: u64 = 2048;
const ARITH: u64 = CF | PF | AF | ZF | SF | OF;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu64 {
    pub regs: [u64; 16], // RAX,RCX,RDX,RBX,RSP,RBP,RSI,RDI,R8..R15
    pub xmm: [[u8; 16]; 16],
    pub mxcsr: u32,
    pub fpu: crate::cpu::Fpu,
    pub rip: u64,
    pub rflags: u64,
    pub gs_base: u64,
    pub retired: u64,
    /// Budget units, including each completed REP element; partial REP is not retired.
    pub work: u64,
}
fn reg(r: R) -> Result<(usize, u32, u32), String> {
    let full = r.full_register();
    let index = match full {
        R::RAX => 0,
        R::RCX => 1,
        R::RDX => 2,
        R::RBX => 3,
        R::RSP => 4,
        R::RBP => 5,
        R::RSI => 6,
        R::RDI => 7,
        R::R8 => 8,
        R::R9 => 9,
        R::R10 => 10,
        R::R11 => 11,
        R::R12 => 12,
        R::R13 => 13,
        R::R14 => 14,
        R::R15 => 15,
        _ => return Err(format!("unsupported AMD64 register {r:?}")),
    };
    Ok((
        index,
        (r.size() * 8) as u32,
        if matches!(r, R::AH | R::BH | R::CH | R::DH) {
            8
        } else {
            0
        },
    ))
}
fn mask(bits: u32) -> u64 {
    if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}
fn signed(value: u64, bits: u32) -> i128 {
    ((value << (64 - bits)) as i64 >> (64 - bits)) as i128
}
impl Cpu64 {
    pub fn new(rip: u64, rsp: u64) -> Self {
        let mut regs = [0; 16];
        regs[4] = rsp;
        Self {
            regs,
            xmm: [[0; 16]; 16],
            mxcsr: 0x1f80,
            fpu: crate::cpu::Fpu::default(),
            rip,
            rflags: 0x202,
            gs_base: 0,
            retired: 0,
            work: 0,
        }
    }
    fn get(&self, r: R) -> Result<u64, String> {
        if r == R::None {
            return Ok(0);
        }
        let (i, b, s) = reg(r)?;
        Ok((self.regs[i] >> s) & mask(b))
    }
    fn set(&mut self, r: R, value: u64) -> Result<(), String> {
        let (i, b, s) = reg(r)?;
        let m = mask(b);
        if b >= 32 {
            self.regs[i] = value & m;
        } else {
            self.regs[i] = (self.regs[i] & !(m << s)) | ((value & m) << s);
        }
        Ok(())
    }
    fn address(&self, i: &Instruction) -> Result<u64, String> {
        let mut at = if i.is_ip_rel_memory_operand() {
            i.ip_rel_memory_address()
        } else {
            self.get(i.memory_base())?
                .wrapping_add(
                    self.get(i.memory_index())?
                        .wrapping_mul(i.memory_index_scale() as u64),
                )
                .wrapping_add(i.memory_displacement64())
        };
        if matches!(
            i.memory_base(),
            R::EAX
                | R::ECX
                | R::EDX
                | R::EBX
                | R::ESP
                | R::EBP
                | R::ESI
                | R::EDI
                | R::R8D
                | R::R9D
                | R::R10D
                | R::R11D
                | R::R12D
                | R::R13D
                | R::R14D
                | R::R15D
        ) {
            at = at as u32 as u64;
        }
        match if i.mnemonic() == M::Lea {
            R::None
        } else {
            i.segment_prefix()
        } {
            R::GS => at = at.wrapping_add(self.gs_base),
            R::FS => return Err("AMD64 FS base not implemented".into()),
            _ => {}
        }
        Ok(at)
    }
    fn width(i: &Instruction, op: u32) -> Result<u32, String> {
        match i.op_kind(op) {
            OpKind::Register => Ok((i.op_register(op).size() * 8) as u32),
            OpKind::Memory => Ok((i.memory_size().size() * 8) as u32),
            _ => Err("operand has no scalar width".into()),
        }
    }
    fn read(&self, i: &Instruction, op: u32, mem: &Memory) -> Result<u64, String> {
        Ok(match i.op_kind(op) {
            OpKind::Register => self.get(i.op_register(op))?,
            OpKind::Memory => mem.read_int(self.address(i)?, i.memory_size().size())?,
            OpKind::Immediate8 => i.immediate8() as u64,
            OpKind::Immediate16 => i.immediate16() as u64,
            OpKind::Immediate32 => i.immediate32() as u64,
            OpKind::Immediate64 => i.immediate64(),
            OpKind::Immediate8to16 => i.immediate8to16() as u16 as u64,
            OpKind::Immediate8to32 => i.immediate8to32() as u32 as u64,
            OpKind::Immediate8to64 => i.immediate8to64() as u64,
            OpKind::Immediate32to64 => i.immediate32to64() as u64,
            OpKind::NearBranch64 => i.near_branch64(),
            _ => return Err(format!("unsupported AMD64 operand {:?}", i.op_kind(op))),
        })
    }
    fn write(
        &mut self,
        i: &Instruction,
        op: u32,
        value: u64,
        mem: &mut Memory,
    ) -> Result<(), String> {
        match i.op_kind(op) {
            OpKind::Register => self.set(i.op_register(op), value),
            OpKind::Memory => mem.write_int(self.address(i)?, i.memory_size().size(), value),
            _ => Err("invalid AMD64 destination".into()),
        }
    }
    fn xmm_index(r: R) -> Result<usize, String> {
        if (R::XMM0..=R::XMM15).contains(&r) {
            Ok(r as usize - R::XMM0 as usize)
        } else {
            Err(format!("unsupported AMD64 vector register {r:?}"))
        }
    }
    fn vector_read(
        &self,
        i: &Instruction,
        op: u32,
        mem: &Memory,
        size: usize,
    ) -> Result<[u8; 16], String> {
        let mut value = [0; 16];
        match i.op_kind(op) {
            OpKind::Register => value[..size]
                .copy_from_slice(&self.xmm[Self::xmm_index(i.op_register(op))?][..size]),
            OpKind::Memory => mem.read(self.address(i)?, &mut value[..size])?,
            _ => return Err("unsupported AMD64 vector source".into()),
        }
        Ok(value)
    }
    fn vector_write(
        &mut self,
        i: &Instruction,
        op: u32,
        value: [u8; 16],
        mem: &mut Memory,
        size: usize,
    ) -> Result<(), String> {
        match i.op_kind(op) {
            OpKind::Register => {
                self.xmm[Self::xmm_index(i.op_register(op))?] = value;
                Ok(())
            }
            OpKind::Memory => mem.write(self.address(i)?, &value[..size]),
            _ => Err("unsupported AMD64 vector destination".into()),
        }
    }
    fn flags(&mut self, a: u64, b: u64, result: u64, bits: u32, sub: bool, carry: bool) {
        let m = mask(bits);
        let sign = 1u64 << (bits - 1);
        let a = a & m;
        let b = b & m;
        let r = result & m;
        let c = if sub {
            (a as u128) < b as u128 + carry as u128
        } else {
            a as u128 + b as u128 + carry as u128 > m as u128
        };
        let o = if sub {
            ((a ^ b) & (a ^ r) & sign) != 0
        } else {
            (!(a ^ b) & (a ^ r) & sign) != 0
        };
        self.rflags = (self.rflags & !ARITH)
            | c as u64
            | if (r as u8).count_ones().is_multiple_of(2) {
                PF
            } else {
                0
            }
            | if (a ^ b ^ r) & 16 != 0 { AF } else { 0 }
            | if r == 0 { ZF } else { 0 }
            | if r & sign != 0 { SF } else { 0 }
            | if o { OF } else { 0 };
    }
    fn condition(&self, m: M) -> Option<bool> {
        let c = self.rflags & CF != 0;
        let z = self.rflags & ZF != 0;
        let s = self.rflags & SF != 0;
        let o = self.rflags & OF != 0;
        let p = self.rflags & PF != 0;
        Some(match m {
            M::Je => z,
            M::Jne => !z,
            M::Ja => !c && !z,
            M::Jae => !c,
            M::Jb => c,
            M::Jbe => c || z,
            M::Jg => !z && s == o,
            M::Jge => s == o,
            M::Jl => s != o,
            M::Jle => z || s != o,
            M::Jo => o,
            M::Jno => !o,
            M::Js => s,
            M::Jns => !s,
            M::Jp => p,
            M::Jnp => !p,
            _ => return None,
        })
    }
    pub fn decode(&self, mem: &Memory) -> Result<Instruction, String> {
        let mut bytes = [0; 15];
        for n in 1..=15 {
            mem.fetch(
                self.rip.checked_add(n as u64 - 1).ok_or("RIP overflow")?,
                &mut bytes[n - 1..n],
            )?;
            let mut decoder = Decoder::with_ip(64, &bytes[..n], self.rip, DecoderOptions::NONE);
            let i = decoder.decode();
            if !i.is_invalid() {
                return Ok(i);
            }
            if decoder.last_error() != DecoderError::NoMoreBytes {
                return Err(format!("invalid AMD64 instruction at {:#x}", self.rip));
            }
        }
        Err("AMD64 instruction exceeds 15 bytes".into())
    }
    pub fn step(&mut self, mem: &mut Memory) -> Result<(), String> {
        let i = self.decode(mem)?;
        let string = matches!(
            i.mnemonic(),
            M::Stosb | M::Stosw | M::Stosd | M::Stosq | M::Movsb | M::Movsw | M::Movsq
        ) || (i.mnemonic() == M::Movsd
            && matches!(i.op0_kind(), OpKind::MemoryESRDI | OpKind::MemoryESEDI));
        if (i.has_lock_prefix()
            && (!matches!(
                i.mnemonic(),
                M::Xchg
                    | M::Cmpxchg
                    | M::Cmpxchg8b
                    | M::Cmpxchg16b
                    | M::Xadd
                    | M::Bts
                    | M::Btr
                    | M::Btc
                    | M::Add
                    | M::Adc
                    | M::Sub
                    | M::Sbb
                    | M::And
                    | M::Or
                    | M::Xor
                    | M::Inc
                    | M::Dec
                    | M::Neg
                    | M::Not
            ) || i.op0_kind() != OpKind::Memory))
            || ((i.has_rep_prefix() || i.has_repne_prefix())
                && !string
                && !matches!(i.mnemonic(), M::Ret | M::Pause))
        {
            return Err(format!("unsupported AMD64 prefix at {:#x}", self.rip));
        }
        let mut next = self.clone();
        next.rip = i.next_ip();
        let mut simd_exception = false;
        let result = if string {
            next.execute_string(&i, mem)
        } else {
            next.execute(&i, mem, &mut simd_exception).map(|()| false)
        };
        let partial = match result {
            Ok(partial) => partial,
            Err(e) => {
                // #XM updates sticky status, but does not retire or commit a destination.
                if simd_exception {
                    self.mxcsr = next.mxcsr;
                }
                return Err(format!("{:#x} {:?}: {e}", self.rip, i.mnemonic()));
            }
        };
        next.retired += (!partial) as u64;
        next.work += 1;
        *self = next;
        Ok(())
    }
    fn execute_string(&mut self, i: &Instruction, mem: &mut Memory) -> Result<bool, String> {
        let size = match i.mnemonic() {
            M::Stosb | M::Movsb => 1,
            M::Stosw | M::Movsw => 2,
            M::Stosd | M::Movsd => 4,
            _ => 8,
        };
        let address32 = matches!(i.op0_kind(), OpKind::MemoryESEDI);
        let repeated = i.has_rep_prefix() || i.has_repne_prefix();
        let count = if address32 {
            self.regs[1] as u32 as u64
        } else {
            self.regs[1]
        };
        if repeated && count == 0 {
            return Ok(false);
        }
        let dest = if address32 {
            self.regs[7] as u32 as u64
        } else {
            self.regs[7]
        };
        let moves = matches!(i.mnemonic(), M::Movsb | M::Movsw | M::Movsd | M::Movsq);
        let value = if moves {
            let source = if address32 {
                self.regs[6] as u32 as u64
            } else {
                self.regs[6]
            };
            let source = match i.segment_prefix() {
                R::GS => source.wrapping_add(self.gs_base),
                R::FS => return Err("AMD64 FS string base not implemented".into()),
                _ => source,
            };
            mem.read_int(source, size)?
        } else {
            self.regs[0]
        };
        mem.write_int(dest, size, value)?;
        let delta = if self.rflags & 1024 == 0 {
            size as u64
        } else {
            0u64.wrapping_sub(size as u64)
        };
        for index in if moves { &[6, 7][..] } else { &[7][..] } {
            let value = self.regs[*index].wrapping_add(delta);
            self.regs[*index] = if address32 {
                value as u32 as u64
            } else {
                value
            };
        }
        if repeated {
            self.regs[1] = count - 1;
            if count > 1 {
                self.rip = i.ip();
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn simd_round(&self) -> Round {
        match (self.mxcsr >> 13) & 3 {
            0 => Round::NearestTiesToEven,
            1 => Round::TowardNegative,
            2 => Round::TowardPositive,
            _ => Round::TowardZero,
        }
    }
    fn simd_status(status: Status) -> u32 {
        let mut flags = 0;
        for (exception, bit) in [
            (Status::INVALID_OP, 0),
            (Status::DIV_BY_ZERO, 2),
            (Status::OVERFLOW, 3),
            (Status::UNDERFLOW, 4),
            (Status::INEXACT, 5),
        ] {
            if status.intersects(exception) {
                flags |= 1 << bit;
            }
        }
        flags
    }
    fn simd_accept<F: Float>(
        &mut self,
        result: StatusAnd<F>,
        mut flags: u32,
        simd_exception: &mut bool,
    ) -> Result<F, String> {
        flags |= Self::simd_status(result.status);
        let mut value = result.value;
        if value.is_denormal() && self.mxcsr & (1 << 15) != 0 && self.mxcsr & (1 << 11) != 0 {
            value = F::ZERO.copy_sign(value);
            flags |= (1 << 4) | (1 << 5);
        }
        self.mxcsr |= flags;
        if flags & !(self.mxcsr >> 7) & 0x3f != 0 {
            *simd_exception = true;
            return Err("unmasked SIMD floating-point exception (#XM)".into());
        }
        Ok(value)
    }
    fn simd_input<F: Float>(&self, value: F, flags: &mut u32) -> F {
        if !value.is_denormal() {
            value
        } else if self.mxcsr & (1 << 6) != 0 {
            F::ZERO.copy_sign(value)
        } else {
            *flags |= 1 << 1;
            value
        }
    }
    fn simd_to_integer<F: Float>(
        &mut self,
        value: u64,
        width: u32,
        round: Round,
        simd_exception: &mut bool,
    ) -> Result<u64, String> {
        let mut input = F::from_bits(value as u128);
        if input.is_denormal() && self.mxcsr & (1 << 6) != 0 {
            input = F::ZERO.copy_sign(input);
        }
        // Float-to-integer conversions report invalid/precision, never denormal/underflow.
        let result = input.to_i128_r(width as usize, round, &mut false);
        let flags = Self::simd_status(result.status);
        self.mxcsr |= flags;
        if flags & !(self.mxcsr >> 7) & 0x3f != 0 {
            *simd_exception = true;
            return Err("unmasked SIMD floating-point exception (#XM)".into());
        }
        Ok(if result.status.intersects(Status::INVALID_OP) {
            1u64 << (width - 1)
        } else {
            result.value as u64
        })
    }
    fn simd_arithmetic<F: Float>(
        &mut self,
        mnemonic: M,
        a: u64,
        b: u64,
        simd_exception: &mut bool,
    ) -> Result<u64, String> {
        let mut flags = 0;
        let a = self.simd_input(F::from_bits(a as u128), &mut flags);
        let b = self.simd_input(F::from_bits(b as u128), &mut flags);
        if matches!(
            mnemonic,
            M::Minss | M::Minsd | M::Minps | M::Minpd | M::Maxss | M::Maxsd | M::Maxps | M::Maxpd
        ) {
            // Legacy SSE selects operand 2 for unordered or equal inputs, including
            // opposite signed zeros. NaN payload/signaling bits are passed through.
            if a.is_nan() || b.is_nan() {
                flags = 1;
            }
            self.simd_accept(Status::OK.and(F::ZERO), flags, simd_exception)?;
            let min = matches!(mnemonic, M::Minss | M::Minsd | M::Minps | M::Minpd);
            let selected = if (min && a < b) || (!min && a > b) {
                a
            } else {
                b
            };
            return Ok(selected.to_bits() as u64);
        }
        let round = self.simd_round();
        if flags & 2 != 0 && self.mxcsr & (1 << 8) == 0 && !a.is_nan() && !b.is_nan() {
            self.simd_accept(Status::OK.and(F::ZERO), flags, simd_exception)?;
        }
        let mut result = match mnemonic {
            M::Addss | M::Addsd | M::Addps | M::Addpd => a.add_r(b, round),
            M::Subss | M::Subsd | M::Subps | M::Subpd => a.sub_r(b, round),
            M::Mulss | M::Mulsd | M::Mulps | M::Mulpd => a.mul_r(b, round),
            _ => a.div_r(b, round),
        };
        if a.is_nan() || b.is_nan() {
            // SSE prefers a quiet NaN over a signaling NaN, then the left operand.
            let selected = if !a.is_nan() || (a.is_signaling() && b.is_nan() && !b.is_signaling()) {
                b
            } else {
                a
            };
            result.value = selected.add_r(F::ZERO, round).value;
            flags &= !(1 << 1); // NaN handling precedes the denormal operand exception.
        } else if result.status.contains(Status::INVALID_OP) {
            // Intel's masked invalid-operation indefinite is a negative quiet NaN.
            result.value = F::NAN.copy_sign(-F::ZERO);
        }
        Ok(self.simd_accept(result, flags, simd_exception)?.to_bits() as u64)
    }
    fn simd_predicate<F: Float>(
        &mut self,
        a: u64,
        b: u64,
        predicate: u8,
        simd_exception: &mut bool,
    ) -> Result<bool, String> {
        let mut flags = 0;
        let a = self.simd_input(F::from_bits(a as u128), &mut flags);
        let b = self.simd_input(F::from_bits(b as u128), &mut flags);
        let unordered = a.is_nan() || b.is_nan();
        let p = predicate & 7;
        if unordered {
            flags = if a.is_signaling() || b.is_signaling() || matches!(p, 1 | 2 | 5 | 6) {
                1
            } else {
                0
            };
        }
        self.simd_accept(Status::OK.and(F::ZERO), flags, simd_exception)?;
        Ok(match p {
            0 => a == b,
            1 => a < b,
            2 => a <= b,
            3 => unordered,
            4 => a != b,
            5 => unordered || a >= b,
            6 => unordered || a > b,
            _ => !unordered,
        })
    }
    fn simd_compare<F: Float>(
        &mut self,
        mnemonic: M,
        a: u64,
        b: u64,
        simd_exception: &mut bool,
    ) -> Result<(), String> {
        let mut flags = 0;
        let a = self.simd_input(F::from_bits(a as u128), &mut flags);
        let b = self.simd_input(F::from_bits(b as u128), &mut flags);
        let unordered = a.is_nan() || b.is_nan();
        if unordered {
            flags = if a.is_signaling()
                || b.is_signaling()
                || matches!(mnemonic, M::Comiss | M::Comisd)
            {
                1
            } else {
                0
            };
        }
        self.simd_accept(Status::OK.and(F::ZERO), flags, simd_exception)?;
        let comparison = if unordered {
            ZF | PF | CF
        } else if a < b {
            CF
        } else if a == b {
            ZF
        } else {
            0
        };
        self.rflags = (self.rflags & !ARITH) | comparison;
        Ok(())
    }
    fn simd_convert<F: Float + FloatConvert<D>, D: Float>(
        &mut self,
        bits: u64,
        simd_exception: &mut bool,
    ) -> Result<u64, String> {
        let mut flags = 0;
        let source = self.simd_input(F::from_bits(bits as u128), &mut flags);
        if flags & 2 != 0 && self.mxcsr & (1 << 8) == 0 {
            self.simd_accept(Status::OK.and(D::ZERO), flags, simd_exception)?;
        }
        let result: StatusAnd<D> = source.convert_r(self.simd_round(), &mut false);
        Ok(self.simd_accept(result, flags, simd_exception)?.to_bits() as u64)
    }
    fn simd_sqrt<F: Float>(
        &mut self,
        bits: u64,
        fraction: u32,
        bias: i32,
        simd_exception: &mut bool,
    ) -> Result<u64, String> {
        let mut flags = 0;
        let value = self.simd_input(F::from_bits(bits as u128), &mut flags);
        let result = if value.is_nan() {
            value.add_r(F::ZERO, self.simd_round())
        } else if value.is_zero() {
            Status::OK.and(value)
        } else if value.is_negative() {
            flags = 1; // Invalid takes priority over a negative denormal operand.
            Status::INVALID_OP.and(F::NAN.copy_sign(-F::ZERO))
        } else if value.is_infinite() {
            Status::OK.and(value)
        } else {
            // Exact integer square root of the normalized significand. No host FP
            // rounding or precision is involved, including the final rounding bit.
            let bits = value.to_bits() as u64;
            let mut exponent = (bits >> fraction) as i32 - bias;
            let mut significand = bits & ((1 << fraction) - 1);
            if exponent == -bias {
                let shift = significand.leading_zeros() - (63 - fraction);
                significand <<= shift;
                exponent = 1 - bias - shift as i32;
            } else {
                significand |= 1 << fraction;
            }
            let radicand = (significand as u128) << (fraction + (exponent & 1) as u32);
            let root = radicand.isqrt();
            let remainder = radicand - root * root;
            let increment = remainder != 0
                && (self.simd_round() == Round::TowardPositive
                    || (self.simd_round() == Round::NearestTiesToEven && remainder > root));
            let encoded = (((exponent.div_euclid(2) + bias) as u64) << fraction)
                + (root as u64 - (1 << fraction))
                + increment as u64;
            let status = if remainder == 0 {
                Status::OK
            } else {
                Status::INEXACT
            };
            status.and(F::from_bits(encoded as u128))
        };
        Ok(self.simd_accept(result, flags, simd_exception)?.to_bits() as u64)
    }
    fn simd_estimate(bits: u32, square_root: bool) -> u32 {
        let sign = bits & 0x80000000;
        let absolute = bits & 0x7fffffff;
        // RCP/RSQRT ignore rounding controls and never report SIMD exceptions.
        // They treat every denormal input as signed zero, and flush tiny output.
        if absolute < 0x00800000 {
            return sign | 0x7f800000;
        }
        if absolute > 0x7f800000 {
            return bits | 0x00400000;
        }
        if square_root && sign != 0 {
            return 0xffc00000;
        }
        if absolute == 0x7f800000 {
            return sign;
        }
        let source = if square_root {
            let mut arithmetic = Cpu64::new(0, 0);
            arithmetic
                .simd_sqrt::<Single>(bits as u64, 23, 127, &mut false)
                .unwrap() as u32
        } else {
            bits
        };
        // A correctly rounded reciprocal is within Intel's maximum relative
        // estimate error (1.5 * 2^-12); a hardware-specific table is not required.
        let result = Single::from_bits(0x3f800000)
            .div_r(Single::from_bits(source as u128), Round::NearestTiesToEven)
            .value;
        if result.is_denormal() {
            sign
        } else {
            result.to_bits() as u32
        }
    }
    fn execute(
        &mut self,
        i: &Instruction,
        mem: &mut Memory,
        simd_exception: &mut bool,
    ) -> Result<(), String> {
        if let Some(taken) = self.condition(i.mnemonic()) {
            if taken {
                self.rip = i.near_branch64();
            }
            return Ok(());
        }
        match i.mnemonic() {
            M::Fld
            | M::Fild
            | M::Fst
            | M::Fstp
            | M::Fist
            | M::Fistp
            | M::Fisttp
            | M::Fadd
            | M::Faddp
            | M::Fiadd
            | M::Fmul
            | M::Fmulp
            | M::Fimul
            | M::Fsub
            | M::Fsubp
            | M::Fsubr
            | M::Fsubrp
            | M::Fisub
            | M::Fisubr
            | M::Fdiv
            | M::Fdivp
            | M::Fdivr
            | M::Fdivrp
            | M::Fidiv
            | M::Fidivr
            | M::Fcom
            | M::Fcomp
            | M::Fcompp
            | M::Ficom
            | M::Ficomp
            | M::Fucom
            | M::Fucomp
            | M::Fucompp
            | M::Fcomi
            | M::Fcomip
            | M::Fucomi
            | M::Fucomip
            | M::Fcmovb
            | M::Fcmove
            | M::Fcmovbe
            | M::Fcmovu
            | M::Fcmovnb
            | M::Fcmovne
            | M::Fcmovnbe
            | M::Fcmovnu
            | M::Fxch
            | M::Ffree
            | M::Ffreep
            | M::Fchs
            | M::Fabs
            | M::Ftst
            | M::Fxam
            | M::Fld1
            | M::Fldl2t
            | M::Fldl2e
            | M::Fldpi
            | M::Fldlg2
            | M::Fldln2
            | M::Fldz
            | M::Fsqrt
            | M::Frndint
            | M::Fscale
            | M::F2xm1
            | M::Fyl2x
            | M::Fyl2xp1
            | M::Fsin
            | M::Fcos
            | M::Fsincos
            | M::Fincstp
            | M::Fdecstp
            | M::Fnop => {
                self.execute_x87(i, mem)?;
            }
            M::Nop
            | M::Endbr64
            | M::Pause
            | M::Prefetchw
            | M::Prefetchnta
            | M::Prefetcht0
            | M::Prefetcht1
            | M::Prefetcht2 => {}
            M::Fnstcw => mem.write_int(self.address(i)?, 2, self.fpu.cw as u64)?,
            M::Fnstsw => self.write(i, 0, self.fpu.sw as u64, mem)?,
            M::Fldcw | M::Wait => {
                if self.fpu.sw & !self.fpu.cw & 0x3f != 0 {
                    return Err(format!(
                        "unmasked x87 floating-point exception (#MF); status={:#x}",
                        self.fpu.sw
                    ));
                }
                if i.mnemonic() == M::Fldcw {
                    self.fpu.cw = mem.read_int(self.address(i)?, 2)? as u16;
                    if self.fpu.sw & !self.fpu.cw & 0x3f != 0 {
                        self.fpu.sw |= 0x8080;
                    }
                }
            }
            M::Fnclex => self.fpu.sw &= 0x7f00,
            M::Fninit => {
                let payload = self.fpu.regs;
                self.fpu = crate::cpu::Fpu::default();
                self.fpu.regs = payload;
            }
            M::Cmpxchg8b | M::Cmpxchg16b => {
                let wide = i.mnemonic() == M::Cmpxchg16b;
                let size = if wide { 16 } else { 8 };
                let address = self.address(i)?;
                if wide && !address.is_multiple_of(16) {
                    return Err("unaligned CMPXCHG16B (#GP)".into());
                }
                mem.check(address, size, super::memory::WRITE)?;
                let mut bytes = [0; 16];
                mem.read(address, &mut bytes[..size])?;
                let (memory, expected, replacement) = if wide {
                    (
                        u128::from_le_bytes(bytes),
                        (self.regs[2] as u128) << 64 | self.regs[0] as u128,
                        (self.regs[1] as u128) << 64 | self.regs[3] as u128,
                    )
                } else {
                    (
                        u64::from_le_bytes(bytes[..8].try_into().unwrap()) as u128,
                        ((self.regs[2] as u32 as u64) << 32 | self.regs[0] as u32 as u64) as u128,
                        ((self.regs[1] as u32 as u64) << 32 | self.regs[3] as u32 as u64) as u128,
                    )
                };
                let equal = memory == expected;
                mem.write(
                    address,
                    &(if equal { replacement } else { memory }).to_le_bytes()[..size],
                )?;
                self.rflags = (self.rflags & !ZF) | if equal { ZF } else { 0 };
                if !equal {
                    if wide {
                        self.regs[0] = memory as u64;
                        self.regs[2] = (memory >> 64) as u64;
                    } else {
                        self.set(R::EAX, memory as u32 as u64)?;
                        self.set(R::EDX, (memory >> 32) as u32 as u64)?;
                    }
                }
            }
            M::Ldmxcsr => {
                let value = self.read(i, 0, mem)? as u32;
                if value & !0xffff != 0 {
                    return Err("reserved MXCSR bits (#GP)".into());
                }
                self.mxcsr = value;
            }
            M::Stmxcsr => self.write(i, 0, self.mxcsr as u64, mem)?,
            M::Cvtss2sd | M::Cvtsd2ss | M::Cvtps2pd | M::Cvtpd2ps => {
                let packed = matches!(i.mnemonic(), M::Cvtps2pd | M::Cvtpd2ps);
                let widening = matches!(i.mnemonic(), M::Cvtss2sd | M::Cvtps2pd);
                let from = if widening { 4 } else { 8 };
                let to = if widening { 8 } else { 4 };
                let count = if packed { 2 } else { 1 };
                if i.mnemonic() == M::Cvtpd2ps
                    && i.op1_kind() == OpKind::Memory
                    && !self.address(i)?.is_multiple_of(16)
                {
                    return Err("unaligned AMD64 packed floating-point conversion".into());
                }
                let source = self.vector_read(i, 1, mem, from * count)?;
                let mut value = if packed {
                    [0; 16]
                } else {
                    self.vector_read(i, 0, mem, 16)?
                };
                for n in 0..count {
                    let mut bits = [0; 8];
                    bits[..from].copy_from_slice(&source[n * from..n * from + from]);
                    let result = if widening {
                        self.simd_convert::<Single, Double>(
                            u64::from_le_bytes(bits),
                            simd_exception,
                        )
                    } else {
                        self.simd_convert::<Double, Single>(
                            u64::from_le_bytes(bits),
                            simd_exception,
                        )
                    };
                    match result {
                        Ok(result) => {
                            value[n * to..n * to + to].copy_from_slice(&result.to_le_bytes()[..to])
                        }
                        Err(e) if !*simd_exception => return Err(e),
                        Err(_) => {}
                    }
                }
                if *simd_exception {
                    return Err("unmasked SIMD floating-point exception (#XM)".into());
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Rcpps | M::Rcpss | M::Rsqrtps | M::Rsqrtss => {
                let scalar = matches!(i.mnemonic(), M::Rcpss | M::Rsqrtss);
                let size = if scalar { 4 } else { 16 };
                if !scalar && i.op1_kind() == OpKind::Memory && !self.address(i)?.is_multiple_of(16)
                {
                    return Err("unaligned AMD64 reciprocal vector access".into());
                }
                let source = self.vector_read(i, 1, mem, size)?;
                let mut value = self.vector_read(i, 0, mem, 16)?;
                for n in (0..size).step_by(4) {
                    let bits = u32::from_le_bytes(source[n..n + 4].try_into().unwrap());
                    value[n..n + 4].copy_from_slice(
                        &Self::simd_estimate(bits, matches!(i.mnemonic(), M::Rsqrtps | M::Rsqrtss))
                            .to_le_bytes(),
                    );
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Sqrtss | M::Sqrtsd => {
                let size = if i.mnemonic() == M::Sqrtss { 4 } else { 8 };
                let source =
                    u64::from_le_bytes(self.vector_read(i, 1, mem, size)?[..8].try_into().unwrap());
                let value = if size == 4 {
                    self.simd_sqrt::<Single>(source, 23, 127, simd_exception)?
                } else {
                    self.simd_sqrt::<Double>(source, 52, 1023, simd_exception)?
                };
                let destination = Self::xmm_index(i.op0_register())?;
                self.xmm[destination][..size].copy_from_slice(&value.to_le_bytes()[..size]);
            }
            M::Ucomiss | M::Comiss | M::Ucomisd | M::Comisd => {
                let size = if matches!(i.mnemonic(), M::Ucomiss | M::Comiss) {
                    4
                } else {
                    8
                };
                let a =
                    u64::from_le_bytes(self.vector_read(i, 0, mem, size)?[..8].try_into().unwrap());
                let b =
                    u64::from_le_bytes(self.vector_read(i, 1, mem, size)?[..8].try_into().unwrap());
                if size == 4 {
                    self.simd_compare::<Single>(i.mnemonic(), a, b, simd_exception)?;
                } else {
                    self.simd_compare::<Double>(i.mnemonic(), a, b, simd_exception)?;
                }
            }
            M::Cvtsi2ss | M::Cvtsi2sd => {
                let bits = Self::width(i, 1)?;
                let integer = signed(self.read(i, 1, mem)?, bits);
                let round = self.simd_round();
                let (size, value) = if i.mnemonic() == M::Cvtsi2ss {
                    (
                        4,
                        self.simd_accept(Single::from_i128_r(integer, round), 0, simd_exception)?
                            .to_bits() as u64,
                    )
                } else {
                    (
                        8,
                        self.simd_accept(Double::from_i128_r(integer, round), 0, simd_exception)?
                            .to_bits() as u64,
                    )
                };
                let destination = Self::xmm_index(i.op0_register())?;
                self.xmm[destination][..size].copy_from_slice(&value.to_le_bytes()[..size]);
            }
            M::Cvtdq2pd | M::Cvtdq2ps | M::Cvtps2dq | M::Cvttps2dq | M::Cvtpd2dq | M::Cvttpd2dq => {
                let from_integer = matches!(i.mnemonic(), M::Cvtdq2pd | M::Cvtdq2ps);
                let double = matches!(i.mnemonic(), M::Cvtdq2pd | M::Cvtpd2dq | M::Cvttpd2dq);
                let size = if i.mnemonic() == M::Cvtdq2pd { 8 } else { 16 };
                if size == 16
                    && i.op1_kind() == OpKind::Memory
                    && !self.address(i)?.is_multiple_of(16)
                {
                    return Err("unaligned AMD64 packed integer conversion access".into());
                }
                let source = self.vector_read(i, 1, mem, size)?;
                let mut value = [0; 16];
                let round = if matches!(i.mnemonic(), M::Cvttps2dq | M::Cvttpd2dq) {
                    Round::TowardZero
                } else {
                    self.simd_round()
                };
                for lane in 0..if double { 2 } else { 4 } {
                    let from = if !from_integer && double { 8 } else { 4 };
                    let to = if from_integer && double { 8 } else { 4 };
                    let mut bits = [0; 8];
                    bits[..from].copy_from_slice(&source[lane * from..lane * from + from]);
                    let bits = u64::from_le_bytes(bits);
                    let result = if from_integer {
                        let integer = bits as u32 as i32 as i128;
                        if double {
                            Ok(Double::from_i128(integer).value.to_bits() as u64)
                        } else {
                            self.simd_accept(Single::from_i128_r(integer, round), 0, simd_exception)
                                .map(|value| value.to_bits() as u64)
                        }
                    } else if double {
                        self.simd_to_integer::<Double>(bits, 32, round, simd_exception)
                    } else {
                        self.simd_to_integer::<Single>(bits, 32, round, simd_exception)
                    };
                    match result {
                        Ok(result) => value[lane * to..lane * to + to]
                            .copy_from_slice(&result.to_le_bytes()[..to]),
                        Err(error) if !*simd_exception => return Err(error),
                        Err(_) => {}
                    }
                }
                if *simd_exception {
                    return Err("unmasked SIMD floating-point exception (#XM)".into());
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Cvtss2si | M::Cvtsd2si | M::Cvttss2si | M::Cvttsd2si => {
                let size = if matches!(i.mnemonic(), M::Cvtss2si | M::Cvttss2si) {
                    4
                } else {
                    8
                };
                let source =
                    u64::from_le_bytes(self.vector_read(i, 1, mem, size)?[..8].try_into().unwrap());
                let width = Self::width(i, 0)?;
                let round = if matches!(i.mnemonic(), M::Cvttss2si | M::Cvttsd2si) {
                    Round::TowardZero
                } else {
                    self.simd_round()
                };
                let value = if size == 4 {
                    self.simd_to_integer::<Single>(source, width, round, simd_exception)?
                } else {
                    self.simd_to_integer::<Double>(source, width, round, simd_exception)?
                };
                self.write(i, 0, value, mem)?;
            }
            M::Movmskps | M::Movmskpd => {
                let source = self.vector_read(i, 1, mem, 16)?;
                let lane = match i.mnemonic() {
                    M::Movmskps => 4,
                    M::Movmskpd => 8,
                    _ => 1,
                };
                let mut value = 0;
                for n in 0..16 / lane {
                    value |= ((source[(n + 1) * lane - 1] >> 7) as u64) << n;
                }
                self.write(i, 0, value, mem)?;
            }
            M::Addss
            | M::Addsd
            | M::Subss
            | M::Subsd
            | M::Mulss
            | M::Mulsd
            | M::Divss
            | M::Divsd
            | M::Addps
            | M::Addpd
            | M::Subps
            | M::Subpd
            | M::Mulps
            | M::Mulpd
            | M::Divps
            | M::Divpd
            | M::Minss
            | M::Minsd
            | M::Minps
            | M::Minpd
            | M::Maxss
            | M::Maxsd
            | M::Maxps
            | M::Maxpd
            | M::Cmpps
            | M::Cmppd
            | M::Cmpss
            | M::Cmpsd => {
                let packed = matches!(
                    i.mnemonic(),
                    M::Addps
                        | M::Addpd
                        | M::Subps
                        | M::Subpd
                        | M::Mulps
                        | M::Mulpd
                        | M::Divps
                        | M::Divpd
                        | M::Minps
                        | M::Minpd
                        | M::Maxps
                        | M::Maxpd
                        | M::Cmpps
                        | M::Cmppd
                );
                let size = if matches!(
                    i.mnemonic(),
                    M::Addss
                        | M::Subss
                        | M::Mulss
                        | M::Divss
                        | M::Addps
                        | M::Subps
                        | M::Mulps
                        | M::Divps
                        | M::Minss
                        | M::Minps
                        | M::Maxss
                        | M::Maxps
                        | M::Cmpps
                        | M::Cmpss
                ) {
                    4
                } else {
                    8
                };
                let transfer = if packed { 16 } else { size };
                if packed && i.op1_kind() == OpKind::Memory && !self.address(i)?.is_multiple_of(16)
                {
                    return Err("unaligned AMD64 packed floating-point access".into());
                }
                let a = self.vector_read(i, 0, mem, 16)?;
                let b = self.vector_read(i, 1, mem, transfer)?;
                let mut value = a;
                for n in (0..transfer).step_by(size) {
                    let mut av = [0; 8];
                    let mut bv = [0; 8];
                    av[..size].copy_from_slice(&a[n..n + size]);
                    bv[..size].copy_from_slice(&b[n..n + size]);
                    let result =
                        if matches!(i.mnemonic(), M::Cmpps | M::Cmppd | M::Cmpss | M::Cmpsd) {
                            let result = if size == 4 {
                                self.simd_predicate::<Single>(
                                    u64::from_le_bytes(av),
                                    u64::from_le_bytes(bv),
                                    i.immediate8(),
                                    simd_exception,
                                )
                            } else {
                                self.simd_predicate::<Double>(
                                    u64::from_le_bytes(av),
                                    u64::from_le_bytes(bv),
                                    i.immediate8(),
                                    simd_exception,
                                )
                            };
                            result.map(|p| if p { u64::MAX } else { 0 })
                        } else if size == 4 {
                            self.simd_arithmetic::<Single>(
                                i.mnemonic(),
                                u64::from_le_bytes(av),
                                u64::from_le_bytes(bv),
                                simd_exception,
                            )
                        } else {
                            self.simd_arithmetic::<Double>(
                                i.mnemonic(),
                                u64::from_le_bytes(av),
                                u64::from_le_bytes(bv),
                                simd_exception,
                            )
                        };
                    // Accrue every lane's exceptions before delivering a packed #XM.
                    match result {
                        Ok(result) => {
                            value[n..n + size].copy_from_slice(&result.to_le_bytes()[..size])
                        }
                        Err(e) if !*simd_exception => return Err(e),
                        Err(_) => {}
                    }
                }
                if *simd_exception {
                    return Err("unmasked SIMD floating-point exception (#XM)".into());
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Movups | M::Movaps | M::Movupd | M::Movapd | M::Movdqu | M::Movdqa => {
                if matches!(i.mnemonic(), M::Movaps | M::Movapd | M::Movdqa)
                    && (i.op0_kind() == OpKind::Memory || i.op1_kind() == OpKind::Memory)
                    && !self.address(i)?.is_multiple_of(16)
                {
                    return Err("unaligned AMD64 aligned vector access".into());
                }
                let value = self.vector_read(i, 1, mem, 16)?;
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Xorps
            | M::Xorpd
            | M::Pxor
            | M::Pand
            | M::Por
            | M::Pandn
            | M::Andps
            | M::Andpd
            | M::Orps
            | M::Orpd
            | M::Andnps
            | M::Andnpd => {
                if i.op1_kind() == OpKind::Memory && !self.address(i)?.is_multiple_of(16) {
                    return Err("unaligned AMD64 legacy vector ALU access".into());
                }
                let a = self.vector_read(i, 0, mem, 16)?;
                let b = self.vector_read(i, 1, mem, 16)?;
                let mut value = [0; 16];
                for n in 0..16 {
                    value[n] = match i.mnemonic() {
                        M::Pand | M::Andps | M::Andpd => a[n] & b[n],
                        M::Pandn | M::Andnps | M::Andnpd => !a[n] & b[n],
                        M::Por | M::Orps | M::Orpd => a[n] | b[n],
                        _ => a[n] ^ b[n],
                    };
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Punpcklbw
            | M::Punpcklwd
            | M::Punpckldq
            | M::Punpcklqdq
            | M::Punpckhbw
            | M::Punpckhwd
            | M::Punpckhdq
            | M::Punpckhqdq
            | M::Unpcklps
            | M::Unpcklpd
            | M::Unpckhps
            | M::Unpckhpd => {
                if i.op1_kind() == OpKind::Memory && !self.address(i)?.is_multiple_of(16) {
                    return Err("unaligned AMD64 vector unpack access".into());
                }
                let a = self.vector_read(i, 0, mem, 16)?;
                let b = self.vector_read(i, 1, mem, 16)?;
                let width = match i.mnemonic() {
                    M::Punpcklbw | M::Punpckhbw => 1,
                    M::Punpcklwd | M::Punpckhwd => 2,
                    M::Punpckldq | M::Punpckhdq | M::Unpcklps | M::Unpckhps => 4,
                    _ => 8,
                };
                let start = if matches!(
                    i.mnemonic(),
                    M::Punpckhbw
                        | M::Punpckhwd
                        | M::Punpckhdq
                        | M::Punpckhqdq
                        | M::Unpckhps
                        | M::Unpckhpd
                ) {
                    8
                } else {
                    0
                };
                let mut value = [0; 16];
                for n in 0..8 / width {
                    value[n * width * 2..n * width * 2 + width]
                        .copy_from_slice(&a[start + n * width..start + (n + 1) * width]);
                    value[n * width * 2 + width..(n + 1) * width * 2]
                        .copy_from_slice(&b[start + n * width..start + (n + 1) * width]);
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Pshufd | M::Pshuflw | M::Pshufhw | M::Shufps | M::Shufpd => {
                if i.op1_kind() == OpKind::Memory && !self.address(i)?.is_multiple_of(16) {
                    return Err("unaligned AMD64 vector shuffle access".into());
                }
                let b = self.vector_read(i, 1, mem, 16)?;
                let a = if matches!(i.mnemonic(), M::Shufps | M::Shufpd) {
                    self.vector_read(i, 0, mem, 16)?
                } else {
                    b
                };
                let immediate = i.immediate8();
                let mut value = b;
                if i.mnemonic() == M::Shufpd {
                    value[..8].copy_from_slice(
                        &a[(immediate as usize & 1) * 8..(immediate as usize & 1) * 8 + 8],
                    );
                    value[8..].copy_from_slice(
                        &b[((immediate as usize >> 1) & 1) * 8
                            ..((immediate as usize >> 1) & 1) * 8 + 8],
                    );
                } else {
                    let width = if matches!(i.mnemonic(), M::Pshuflw | M::Pshufhw) {
                        2
                    } else {
                        4
                    };
                    let start = if i.mnemonic() == M::Pshufhw { 8 } else { 0 };
                    for n in 0..4 {
                        let source = if i.mnemonic() == M::Shufps && n < 2 {
                            a
                        } else {
                            b
                        };
                        let selected = (immediate as usize >> (n * 2)) & 3;
                        value[start + n * width..start + (n + 1) * width].copy_from_slice(
                            &source[start + selected * width..start + (selected + 1) * width],
                        );
                    }
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Movss | M::Movsd => {
                let size = if i.mnemonic() == M::Movss { 4 } else { 8 };
                let mut value = self.vector_read(i, 1, mem, size)?;
                if i.op0_kind() == OpKind::Register && i.op1_kind() == OpKind::Register {
                    let old = self.vector_read(i, 0, mem, 16)?;
                    value[size..].copy_from_slice(&old[size..]);
                }
                self.vector_write(i, 0, value, mem, size)?;
            }
            M::Movlhps | M::Movhlps => {
                let mut value = self.vector_read(i, 0, mem, 16)?;
                let source = self.vector_read(i, 1, mem, 16)?;
                if i.mnemonic() == M::Movlhps {
                    value[8..].copy_from_slice(&source[..8]);
                } else {
                    value[..8].copy_from_slice(&source[8..]);
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Movlps | M::Movhps | M::Movlpd | M::Movhpd => {
                let offset = if matches!(i.mnemonic(), M::Movhps | M::Movhpd) {
                    8
                } else {
                    0
                };
                if i.op0_kind() == OpKind::Memory {
                    let source = self.vector_read(i, 1, mem, 16)?;
                    mem.write(self.address(i)?, &source[offset..offset + 8])?;
                } else {
                    let source = self.vector_read(i, 1, mem, 8)?;
                    let mut value = self.vector_read(i, 0, mem, 16)?;
                    value[offset..offset + 8].copy_from_slice(&source[..8]);
                    self.vector_write(i, 0, value, mem, 16)?;
                }
            }
            M::Pcmpeqb
            | M::Pcmpeqw
            | M::Pcmpeqd
            | M::Pcmpgtb
            | M::Pcmpgtw
            | M::Pcmpgtd
            | M::Paddb
            | M::Paddw
            | M::Paddd
            | M::Paddq
            | M::Psubb
            | M::Psubw
            | M::Psubd
            | M::Psubq => {
                if i.op1_kind() == OpKind::Memory && !self.address(i)?.is_multiple_of(16) {
                    return Err("unaligned AMD64 packed integer access".into());
                }
                let a = self.vector_read(i, 0, mem, 16)?;
                let b = self.vector_read(i, 1, mem, 16)?;
                let mut value = [0; 16];
                let width = match i.mnemonic() {
                    M::Pcmpeqb | M::Pcmpgtb | M::Paddb | M::Psubb => 1,
                    M::Pcmpeqw | M::Pcmpgtw | M::Paddw | M::Psubw => 2,
                    M::Paddq | M::Psubq => 8,
                    _ => 4,
                };
                for n in (0..16).step_by(width) {
                    let mut av = [0; 8];
                    let mut bv = [0; 8];
                    av[..width].copy_from_slice(&a[n..n + width]);
                    bv[..width].copy_from_slice(&b[n..n + width]);
                    let av = u64::from_le_bytes(av);
                    let bv = u64::from_le_bytes(bv);
                    let result = match i.mnemonic() {
                        M::Pcmpeqb | M::Pcmpeqw | M::Pcmpeqd => {
                            if av == bv {
                                u64::MAX
                            } else {
                                0
                            }
                        }
                        M::Pcmpgtb | M::Pcmpgtw | M::Pcmpgtd => {
                            if signed(av, (width * 8) as u32) > signed(bv, (width * 8) as u32) {
                                u64::MAX
                            } else {
                                0
                            }
                        }
                        M::Psubb | M::Psubw | M::Psubd | M::Psubq => av.wrapping_sub(bv),
                        _ => av.wrapping_add(bv),
                    };
                    value[n..n + width].copy_from_slice(&result.to_le_bytes()[..width]);
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Pmovmskb => {
                let source = self.vector_read(i, 1, mem, 16)?;
                let value = source
                    .iter()
                    .enumerate()
                    .fold(0u64, |mask, (n, b)| mask | ((b >> 7) as u64) << n);
                self.write(i, 0, value, mem)?;
            }
            M::Psrldq | M::Pslldq => {
                let source = self.vector_read(i, 0, mem, 16)?;
                let count = i.immediate8() as usize;
                let mut value = [0; 16];
                if count < 16 {
                    if i.mnemonic() == M::Psrldq {
                        value[..16 - count].copy_from_slice(&source[count..]);
                    } else {
                        value[count..].copy_from_slice(&source[..16 - count]);
                    }
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Psllw
            | M::Pslld
            | M::Psllq
            | M::Psrlw
            | M::Psrld
            | M::Psrlq
            | M::Psraw
            | M::Psrad => {
                let a = self.vector_read(i, 0, mem, 16)?;
                if i.op1_kind() == OpKind::Memory && !self.address(i)?.is_multiple_of(16) {
                    return Err("unaligned AMD64 vector shift count".into());
                }
                let count = if i.op1_kind() == OpKind::Immediate8 {
                    i.immediate8() as u64
                } else {
                    u64::from_le_bytes(self.vector_read(i, 1, mem, 16)?[..8].try_into().unwrap())
                };
                let width = match i.mnemonic() {
                    M::Psllw | M::Psrlw | M::Psraw => 2,
                    M::Psllq | M::Psrlq => 8,
                    _ => 4,
                };
                let bits = (width * 8) as u32;
                let mut value = [0; 16];
                for n in (0..16).step_by(width) {
                    let mut lane = [0; 8];
                    lane[..width].copy_from_slice(&a[n..n + width]);
                    let lane = u64::from_le_bytes(lane);
                    let result = if matches!(i.mnemonic(), M::Psraw | M::Psrad) {
                        (signed(lane, bits) >> count.min((bits - 1) as u64)) as u64
                    } else if count >= bits as u64 {
                        0
                    } else if matches!(i.mnemonic(), M::Psllw | M::Pslld | M::Psllq) {
                        lane << count
                    } else {
                        lane >> count
                    };
                    value[n..n + width].copy_from_slice(&result.to_le_bytes()[..width]);
                }
                self.vector_write(i, 0, value, mem, 16)?;
            }
            M::Movd | M::Movq => {
                let size = if i.mnemonic() == M::Movd { 4 } else { 8 };
                if i.op0_kind() == OpKind::Register && Self::xmm_index(i.op0_register()).is_ok() {
                    let value = if i.op1_kind() == OpKind::Register
                        && Self::xmm_index(i.op1_register()).is_ok()
                    {
                        self.vector_read(i, 1, mem, size)?
                    } else {
                        let mut value = [0; 16];
                        value[..size].copy_from_slice(&self.read(i, 1, mem)?.to_le_bytes()[..size]);
                        value
                    };
                    self.vector_write(i, 0, value, mem, size)?;
                } else {
                    let value = self.vector_read(i, 1, mem, size)?;
                    self.write(
                        i,
                        0,
                        u64::from_le_bytes(value[..8].try_into().unwrap()),
                        mem,
                    )?;
                }
            }
            M::Bt | M::Bts | M::Btr | M::Btc => {
                let bits = Self::width(i, 0)?;
                let offset = self.read(i, 1, mem)?;
                let bit = offset as u32 & (bits - 1);
                let address = if i.op0_kind() == OpKind::Memory {
                    let mut address = self.address(i)?;
                    if i.op1_kind() == OpKind::Register {
                        let signed = match bits {
                            16 => offset as i16 as i64,
                            32 => offset as i32 as i64,
                            _ => offset as i64,
                        };
                        address = address.wrapping_add(
                            (signed.div_euclid(bits as i64) * (bits / 8) as i64) as u64,
                        );
                    }
                    Some(address)
                } else {
                    None
                };
                let a = if let Some(address) = address {
                    mem.read_int(address, (bits / 8) as usize)?
                } else {
                    self.read(i, 0, mem)?
                };
                self.rflags = (self.rflags & !CF) | ((a >> bit) & 1);
                if i.mnemonic() != M::Bt {
                    let b = match i.mnemonic() {
                        M::Bts => a | (1 << bit),
                        M::Btr => a & !(1 << bit),
                        _ => a ^ (1 << bit),
                    };
                    if let Some(address) = address {
                        mem.write_int(address, (bits / 8) as usize, b)?;
                    } else {
                        self.write(i, 0, b, mem)?;
                    }
                }
            }
            M::Cpuid => {
                let leaf = self.regs[0] as u32;
                let result = match leaf {
                    // Conservative integer profile, with no advertised SIMD/AVX/OSXSAVE.
                    0 => [
                        1,
                        u32::from_le_bytes(*b"Wire"),
                        u32::from_le_bytes(*b"CPU "),
                        u32::from_le_bytes(*b"Virt"),
                    ],
                    1 => [0x600, 0x10000, 0, 0],
                    0x80000000 => [0x80000008, 0, 0, 0],
                    0x80000001 => [0, 0, 0, (1 << 29) | (1 << 20)], // Long mode and checked NX.
                    0x80000008 => [0x3030, 0, 0, 0],
                    _ => [0; 4],
                };
                for (r, value) in [0, 3, 1, 2].into_iter().zip(result) {
                    self.regs[r] = value as u64;
                }
            }
            M::Cbw => self.set(R::AX, self.get(R::AL)? as i8 as i16 as u16 as u64)?,
            M::Cwde => self.set(R::EAX, self.get(R::AX)? as i16 as i32 as u32 as u64)?,
            M::Cdqe => self.set(R::RAX, self.get(R::EAX)? as i32 as i64 as u64)?,
            M::Cwd => self.set(
                R::DX,
                if self.get(R::AX)? & 0x8000 != 0 {
                    u16::MAX as u64
                } else {
                    0
                },
            )?,
            M::Cdq => self.set(
                R::EDX,
                if self.get(R::EAX)? & 0x80000000 != 0 {
                    u32::MAX as u64
                } else {
                    0
                },
            )?,
            M::Cqo => self.set(
                R::RDX,
                if self.get(R::RAX)? >> 63 != 0 {
                    u64::MAX
                } else {
                    0
                },
            )?,
            M::Bswap => {
                let bits = Self::width(i, 0)?;
                let a = self.read(i, 0, mem)?;
                let value = match bits {
                    32 => (a as u32).swap_bytes() as u64,
                    64 => a.swap_bytes(),
                    _ => return Err("invalid BSWAP width".into()),
                };
                self.write(i, 0, value, mem)?;
            }
            M::Bsf | M::Bsr => {
                let a = self.read(i, 1, mem)?;
                self.rflags = (self.rflags & !ZF) | if a == 0 { ZF } else { 0 };
                if a != 0 {
                    let index = if i.mnemonic() == M::Bsf {
                        a.trailing_zeros()
                    } else {
                        63 - a.leading_zeros()
                    };
                    self.write(i, 0, index as u64, mem)?;
                }
            }
            M::Mul | M::Imul | M::Div | M::Idiv => {
                let bits = Self::width(i, 0)?;
                let (acc, high) = match bits {
                    8 => (R::AL, R::AH),
                    16 => (R::AX, R::DX),
                    32 => (R::EAX, R::EDX),
                    64 => (R::RAX, R::RDX),
                    _ => return Err("invalid AMD64 multiply width".into()),
                };
                if matches!(i.mnemonic(), M::Div | M::Idiv) {
                    let divisor = self.read(i, 0, mem)? & mask(bits);
                    if divisor == 0 {
                        return Err("AMD64 divide by zero".into());
                    }
                    let dividend = if bits == 8 {
                        self.get(R::AX)? as u128
                    } else {
                        (self.get(high)? as u128) << bits | self.get(acc)? as u128
                    };
                    let (quotient, remainder) = if i.mnemonic() == M::Div {
                        let q = dividend / divisor as u128;
                        if q > mask(bits) as u128 {
                            return Err("AMD64 division overflow".into());
                        }
                        (q as u64, (dividend % divisor as u128) as u64)
                    } else {
                        let shift = 128 - bits * 2;
                        let dividend = ((dividend << shift) as i128) >> shift;
                        let divisor = signed(divisor, bits);
                        let q = dividend
                            .checked_div(divisor)
                            .ok_or("AMD64 division overflow")?;
                        let min = -(1i128 << (bits - 1));
                        let max = (1i128 << (bits - 1)) - 1;
                        if q < min || q > max {
                            return Err("AMD64 division overflow".into());
                        }
                        (q as u64, (dividend % divisor) as u64)
                    };
                    self.set(acc, quotient)?;
                    self.set(high, remainder)?;
                } else {
                    let implicit = i.op_count() == 1;
                    let a = if implicit {
                        self.get(acc)?
                    } else {
                        self.read(i, if i.op_count() == 3 { 1 } else { 0 }, mem)?
                    };
                    let b = self.read(i, if implicit { 0 } else { i.op_count() - 1 }, mem)?;
                    let product = if i.mnemonic() == M::Mul {
                        (a as u128) * (b as u128)
                    } else {
                        (signed(a, bits) * signed(b, bits)) as u128
                    };
                    let low = product as u64 & mask(bits);
                    let overflow = if i.mnemonic() == M::Mul {
                        product >> bits != 0
                    } else {
                        product as i128 != signed(low, bits)
                    };
                    self.rflags = (self.rflags & !(CF | OF)) | if overflow { CF | OF } else { 0 };
                    if implicit {
                        self.set(acc, low)?;
                        self.set(high, (product >> bits) as u64)?;
                    } else {
                        self.write(i, 0, low, mem)?;
                    }
                }
            }
            M::Xchg | M::Xadd | M::Cmpxchg => {
                let bits = Self::width(i, 0)?;
                let a = self.read(i, 0, mem)?;
                let b = self.read(i, 1, mem)?;
                if i.mnemonic() == M::Cmpxchg {
                    let acc = match bits {
                        8 => R::AL,
                        16 => R::AX,
                        32 => R::EAX,
                        64 => R::RAX,
                        _ => return Err("invalid CMPXCHG width".into()),
                    };
                    let expected = self.get(acc)?;
                    self.flags(expected, a, expected.wrapping_sub(a), bits, true, false);
                    if expected == a {
                        self.write(i, 0, b, mem)?;
                    } else {
                        self.write(i, 0, a, mem)?;
                        self.set(acc, a)?;
                    }
                } else {
                    let value = if i.mnemonic() == M::Xadd {
                        let sum = a.wrapping_add(b);
                        self.flags(a, b, sum, bits, false, false);
                        sum
                    } else {
                        b
                    };
                    self.write(i, 0, value, mem)?;
                    self.write(i, 1, a, mem)?;
                }
            }
            M::Mov => {
                let value = self.read(i, 1, mem)?;
                self.write(i, 0, value, mem)?;
            }
            M::Movzx | M::Movsx | M::Movsxd => {
                let b = Self::width(i, 1)?;
                let mut value = self.read(i, 1, mem)?;
                if i.mnemonic() != M::Movzx && value & (1u64 << (b - 1)) != 0 {
                    value |= !mask(b);
                }
                self.write(i, 0, value, mem)?;
            }
            M::Lea => {
                let address = self.address(i)?;
                self.write(i, 0, address, mem)?;
            }
            M::Cmove
            | M::Cmovne
            | M::Cmova
            | M::Cmovae
            | M::Cmovb
            | M::Cmovbe
            | M::Cmovg
            | M::Cmovge
            | M::Cmovl
            | M::Cmovle
            | M::Cmovo
            | M::Cmovno
            | M::Cmovs
            | M::Cmovns
            | M::Cmovp
            | M::Cmovnp
            | M::Sete
            | M::Setne
            | M::Seta
            | M::Setae
            | M::Setb
            | M::Setbe
            | M::Setg
            | M::Setge
            | M::Setl
            | M::Setle
            | M::Seto
            | M::Setno
            | M::Sets
            | M::Setns
            | M::Setp
            | M::Setnp => {
                let branch = match i.mnemonic() {
                    M::Cmove | M::Sete => M::Je,
                    M::Cmovne | M::Setne => M::Jne,
                    M::Cmova | M::Seta => M::Ja,
                    M::Cmovae | M::Setae => M::Jae,
                    M::Cmovb | M::Setb => M::Jb,
                    M::Cmovbe | M::Setbe => M::Jbe,
                    M::Cmovg | M::Setg => M::Jg,
                    M::Cmovge | M::Setge => M::Jge,
                    M::Cmovl | M::Setl => M::Jl,
                    M::Cmovle | M::Setle => M::Jle,
                    M::Cmovo | M::Seto => M::Jo,
                    M::Cmovno | M::Setno => M::Jno,
                    M::Cmovs | M::Sets => M::Js,
                    M::Cmovns | M::Setns => M::Jns,
                    M::Cmovp | M::Setp => M::Jp,
                    M::Cmovnp | M::Setnp => M::Jnp,
                    _ => unreachable!(),
                };
                let taken = self.condition(branch).unwrap();
                let value = if i.op_count() == 1 {
                    taken as u64
                } else {
                    // CMOV reads its source even when false; a 32-bit destination clears upper bits.
                    let source = self.read(i, 1, mem)?;
                    if taken {
                        source
                    } else {
                        self.read(i, 0, mem)?
                    }
                };
                self.write(i, 0, value, mem)?;
            }
            M::Rcl | M::Rcr => {
                let bits = Self::width(i, 0)?;
                let value = self.read(i, 0, mem)? & mask(bits);
                let masked = (self.read(i, 1, mem)? & if bits == 64 { 63 } else { 31 }) as u32;
                let n = masked % (bits + 1);
                let extended = value as u128 | ((self.rflags & CF) as u128) << bits;
                let m = (1u128 << (bits + 1)) - 1;
                let rotated = if n == 0 {
                    extended
                } else if i.mnemonic() == M::Rcl {
                    ((extended << n) | (extended >> (bits + 1 - n))) & m
                } else {
                    ((extended >> n) | (extended << (bits + 1 - n))) & m
                };
                let result = (rotated as u64) & mask(bits);
                if n != 0 {
                    let carry = (rotated >> bits) as u64 & 1;
                    self.rflags = (self.rflags & !CF) | carry;
                    if masked == 1 {
                        let sign = result >> (bits - 1) & 1;
                        let of = if i.mnemonic() == M::Rcl {
                            sign ^ carry
                        } else {
                            sign ^ (result >> (bits - 2) & 1)
                        };
                        self.rflags = (self.rflags & !OF) | (of << 11);
                    }
                }
                self.write(i, 0, result, mem)?;
            }
            M::Shl | M::Sal | M::Shr | M::Sar | M::Rol | M::Ror => {
                let bits = Self::width(i, 0)?;
                let a = self.read(i, 0, mem)? & mask(bits);
                let count = (self.read(i, 1, mem)? & if bits == 64 { 63 } else { 31 }) as u32;
                let mut result = a;
                if count != 0 {
                    let sign = 1u64 << (bits - 1);
                    let (carry, overflow) = match i.mnemonic() {
                        M::Rol | M::Ror => {
                            let n = count % bits;
                            if n != 0 {
                                result = if i.mnemonic() == M::Rol {
                                    (a << n | a >> (bits - n)) & mask(bits)
                                } else {
                                    (a >> n | a << (bits - n)) & mask(bits)
                                };
                            }
                            let c = if i.mnemonic() == M::Rol {
                                result & 1 != 0
                            } else {
                                result & sign != 0
                            };
                            let o = if i.mnemonic() == M::Rol {
                                (result & sign != 0) ^ c
                            } else {
                                (result & sign != 0) ^ (result & (sign >> 1) != 0)
                            };
                            (c, o)
                        }
                        M::Shl | M::Sal => {
                            result = if count >= bits {
                                0
                            } else {
                                (a << count) & mask(bits)
                            };
                            let c = count <= bits && a & (1 << (bits - count)) != 0;
                            (c, (result & sign != 0) ^ c)
                        }
                        M::Shr => {
                            result = if count >= bits { 0 } else { a >> count };
                            (count <= bits && a & (1 << (count - 1)) != 0, a & sign != 0)
                        }
                        M::Sar => {
                            let negative = a & sign != 0;
                            let signed = if negative { a | !mask(bits) } else { a } as i64;
                            result = ((signed >> count.min(bits - 1)) as u64) & mask(bits);
                            let c = if count >= bits {
                                negative
                            } else {
                                a & (1 << (count - 1)) != 0
                            };
                            (c, false)
                        }
                        _ => unreachable!(),
                    };
                    self.rflags = (self.rflags & !CF) | carry as u64;
                    if count == 1 {
                        self.rflags = (self.rflags & !OF) | if overflow { OF } else { 0 };
                    }
                    if !matches!(i.mnemonic(), M::Rol | M::Ror) {
                        self.rflags = (self.rflags & !(SF | ZF | PF))
                            | if result & sign != 0 { SF } else { 0 }
                            | if result == 0 { ZF } else { 0 }
                            | if (result as u8).count_ones().is_multiple_of(2) {
                                PF
                            } else {
                                0
                            };
                    }
                }
                self.write(i, 0, result, mem)?;
            }
            M::Add | M::Adc | M::Sub | M::Sbb | M::Cmp | M::And | M::Or | M::Xor | M::Test => {
                let bits = Self::width(i, 0)?;
                let a = self.read(i, 0, mem)?;
                let b = self.read(i, 1, mem)?;
                let carry = matches!(i.mnemonic(), M::Adc | M::Sbb) && self.rflags & CF != 0;
                let sub = matches!(i.mnemonic(), M::Sub | M::Sbb | M::Cmp);
                let result = match i.mnemonic() {
                    M::And | M::Test => a & b,
                    M::Or => a | b,
                    M::Xor => a ^ b,
                    _ => {
                        if sub {
                            a.wrapping_sub(b).wrapping_sub(carry as u64)
                        } else {
                            a.wrapping_add(b).wrapping_add(carry as u64)
                        }
                    }
                };
                if matches!(i.mnemonic(), M::And | M::Or | M::Xor | M::Test) {
                    self.flags(result, 0, result, bits, false, false);
                    self.rflags &= !(CF | OF | AF);
                } else {
                    self.flags(a, b, result, bits, sub, carry);
                }
                if !matches!(i.mnemonic(), M::Cmp | M::Test) {
                    self.write(i, 0, result, mem)?;
                }
            }
            M::Inc | M::Dec | M::Neg | M::Not => {
                let a = self.read(i, 0, mem)?;
                let bits = Self::width(i, 0)?;
                let result = match i.mnemonic() {
                    M::Inc => a.wrapping_add(1),
                    M::Dec => a.wrapping_sub(1),
                    M::Neg => 0u64.wrapping_sub(a),
                    _ => !a,
                };
                if i.mnemonic() != M::Not {
                    let old_cf = self.rflags & CF;
                    if i.mnemonic() == M::Neg {
                        self.flags(0, a, result, bits, true, false);
                    } else {
                        self.flags(a, 1, result, bits, i.mnemonic() == M::Dec, false);
                        self.rflags = (self.rflags & !CF) | old_cf;
                    }
                }
                self.write(i, 0, result, mem)?;
            }
            M::Push | M::Call => {
                let value = if i.mnemonic() == M::Call {
                    i.next_ip()
                } else {
                    self.read(i, 0, mem)?
                };
                let target = if i.mnemonic() == M::Call {
                    Some(self.read(i, 0, mem)?)
                } else {
                    None
                };
                let width = if i.stack_pointer_increment() == -2 {
                    2
                } else {
                    8
                };
                let rsp = self.regs[4]
                    .checked_sub(width as u64)
                    .ok_or("AMD64 stack underflow")?;
                mem.write_int(rsp, width, value)?;
                self.regs[4] = rsp;
                if let Some(target) = target {
                    self.rip = target;
                }
            }
            M::Pop => {
                let width = if Self::width(i, 0)? == 16 { 2 } else { 8 };
                let value = mem.read_int(self.regs[4], width)?;
                self.regs[4] = self.regs[4]
                    .checked_add(width as u64)
                    .ok_or("AMD64 stack overflow")?;
                self.write(i, 0, value, mem)?;
            }
            M::Ret => {
                let target = mem.read_int(self.regs[4], 8)?;
                self.regs[4] = self.regs[4]
                    .checked_add(
                        8 + if i.op_count() == 0 {
                            0
                        } else {
                            i.immediate16() as u64
                        },
                    )
                    .ok_or("AMD64 stack overflow")?;
                self.rip = target;
            }
            M::Jmp => self.rip = self.read(i, 0, mem)?,
            M::Clc => self.rflags &= !CF,
            M::Stc => self.rflags |= CF,
            M::Cmc => self.rflags ^= CF,
            M::Cld => self.rflags &= !1024,
            M::Std => self.rflags |= 1024,
            _ => return Err("not implemented in AMD64 bring-up interpreter".into()),
        }
        Ok(())
    }
    /// Windows x64 integer/pointer arguments: RCX,RDX,R8,R9, then RSP+40.
    pub fn arguments(&self, mem: &Memory, count: usize) -> Result<Vec<u64>, String> {
        if count > 16 {
            return Err("AMD64 gateway argument limit".into());
        }
        (0..count)
            .map(|i| {
                if i < 4 {
                    Ok(self.regs[[1, 2, 8, 9][i]])
                } else {
                    mem.read_int(
                        self.regs[4]
                            .checked_add(40 + (i as u64 - 4) * 8)
                            .ok_or("AMD64 argument overflow")?,
                        8,
                    )
                }
            })
            .collect()
    }
    /// Callee pops only return address; caller owns shadow space/stack arguments.
    pub fn return_integer(&mut self, mem: &Memory, value: u64) -> Result<(), String> {
        let target = mem.read_int(self.regs[4], 8)?;
        let rsp = self.regs[4].checked_add(8).ok_or("AMD64 return overflow")?;
        self.regs[0] = value;
        self.regs[4] = rsp;
        self.rip = target;
        Ok(())
    }
}
