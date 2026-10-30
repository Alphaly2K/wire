//! Shared software SSE arithmetic state. No destination or instruction state is
//! changed until the caller commits; unmasked exceptions accrue MXCSR only.
use iced_x86::Mnemonic as M;
use rustc_apfloat::ieee::{IeeeFloat, Semantics};
use rustc_apfloat::{Float, FloatConvert, Round, Status, StatusAnd};
use std::marker::PhantomData;

// The SSE precision with an exponent range wide enough for every finite input
// operation. Intel detects tiny results before denormal rounding, and unmasked
// range exceptions retain this precision's inexact status.
pub(crate) struct Wide<F>(PhantomData<F>);
impl<F: Float> Semantics for Wide<F> {
    const BITS: usize = F::PRECISION + 15;
    const EXP_BITS: usize = 15;
}
type Unbounded<F> = IeeeFloat<Wide<F>>;
const CF: u64 = 1;
const PF: u64 = 4;
const ZF: u64 = 64;
const ARITH: u64 = CF | PF | 16 | ZF | 128 | 2048;
#[derive(Clone, Copy)]
pub struct SimdState {
    pub mxcsr: u32,
    pub rflags: u64,
}
impl SimdState {
    #[inline(always)]
    pub(crate) fn round(&self) -> Round {
        match (self.mxcsr >> 13) & 3 {
            0 => Round::NearestTiesToEven,
            1 => Round::TowardNegative,
            2 => Round::TowardPositive,
            _ => Round::TowardZero,
        }
    }
    #[inline(always)]
    pub(crate) fn status(status: Status) -> u32 {
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
    #[inline(always)]
    pub(crate) fn accept<F: Float>(
        &mut self,
        result: StatusAnd<F>,
        mut flags: u32,
        simd_exception: &mut bool,
    ) -> Result<F, String> {
        flags |= Self::status(result.status);
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
    #[inline(always)]
    pub(crate) fn input<F: Float>(&self, value: F, flags: &mut u32) -> F {
        if !value.is_denormal() {
            value
        } else if self.mxcsr & (1 << 6) != 0 {
            F::ZERO.copy_sign(value)
        } else {
            *flags |= 1 << 1;
            value
        }
    }
    #[inline(always)]
    pub(crate) fn convert_integer<F: Float>(
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
        let flags = Self::status(result.status);
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
    #[inline(always)]
    pub(crate) fn arithmetic<F: Float + FloatConvert<Unbounded<F>>>(
        &mut self,
        mnemonic: M,
        a: u64,
        b: u64,
        simd_exception: &mut bool,
    ) -> Result<u64, String> {
        let mut flags = 0;
        let a = self.input(F::from_bits(a as u128), &mut flags);
        let b = self.input(F::from_bits(b as u128), &mut flags);
        if matches!(
            mnemonic,
            M::Minss | M::Minsd | M::Minps | M::Minpd | M::Maxss | M::Maxsd | M::Maxps | M::Maxpd
        ) {
            // Legacy SSE selects operand 2 for unordered or equal inputs, including
            // opposite signed zeros. NaN payload/signaling bits are passed through.
            if a.is_nan() || b.is_nan() {
                flags = 1;
            }
            self.accept(Status::OK.and(F::ZERO), flags, simd_exception)?;
            let min = matches!(mnemonic, M::Minss | M::Minsd | M::Minps | M::Minpd);
            let selected = if (min && a < b) || (!min && a > b) {
                a
            } else {
                b
            };
            return Ok(selected.to_bits() as u64);
        }
        let round = self.round();
        let zero_divisor =
            matches!(mnemonic, M::Divss | M::Divsd | M::Divps | M::Divpd) && b.is_zero();
        if flags & 2 != 0
            && self.mxcsr & (1 << 8) == 0
            && !a.is_nan()
            && !b.is_nan()
            && !zero_divisor
        {
            self.accept(Status::OK.and(F::ZERO), flags, simd_exception)?;
        }
        let mut result = match mnemonic {
            M::Addss | M::Addsd | M::Addps | M::Addpd => a.add_r(b, round),
            M::Subss | M::Subsd | M::Subps | M::Subpd => a.sub_r(b, round),
            M::Mulss | M::Mulsd | M::Mulps | M::Mulpd => a.mul_r(b, round),
            _ => a.div_r(b, round),
        };
        if (result.value.abs() <= F::smallest_normalized()
            || result.value.abs() == F::largest()
            || result.status.intersects(Status::OVERFLOW))
            && a.is_finite()
            && b.is_finite()
            && !(result.value.is_zero() && !result.status.intersects(Status::INEXACT))
        {
            let a: Unbounded<F> = a.convert_r(round, &mut false).value;
            let b: Unbounded<F> = b.convert_r(round, &mut false).value;
            let wide = match mnemonic {
                M::Addss | M::Addsd | M::Addps | M::Addpd => a.add_r(b, round),
                M::Subss | M::Subsd | M::Subps | M::Subpd => a.sub_r(b, round),
                M::Mulss | M::Mulsd | M::Mulps | M::Mulpd => a.mul_r(b, round),
                _ => a.div_r(b, round),
            };
            self.range::<F>(wide, &mut result);
        }
        if a.is_nan() || b.is_nan() {
            // Intel SDM table 4-8: SSE selects the first NaN, quieting SNaNs.
            let selected = if a.is_nan() { a } else { b };
            result.value = selected.add_r(F::ZERO, round).value;
            flags &= !(1 << 1); // NaN handling precedes the denormal operand exception.
        } else if result.status.contains(Status::INVALID_OP) {
            // Intel's masked invalid-operation indefinite is a negative quiet NaN.
            result.value = F::NAN.copy_sign(-F::ZERO);
        } else if result.status.contains(Status::DIV_BY_ZERO) {
            flags &= !(1 << 1);
        }
        Ok(self.accept(result, flags, simd_exception)?.to_bits() as u64)
    }
    #[inline(always)]
    pub(crate) fn predicate<F: Float>(
        &mut self,
        a: u64,
        b: u64,
        predicate: u8,
        simd_exception: &mut bool,
    ) -> Result<bool, String> {
        let mut flags = 0;
        let a = self.input(F::from_bits(a as u128), &mut flags);
        let b = self.input(F::from_bits(b as u128), &mut flags);
        let unordered = a.is_nan() || b.is_nan();
        let p = predicate & 7;
        if unordered {
            flags = if a.is_signaling() || b.is_signaling() || matches!(p, 1 | 2 | 5 | 6) {
                1
            } else {
                0
            };
        }
        self.accept(Status::OK.and(F::ZERO), flags, simd_exception)?;
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
    #[inline(always)]
    pub(crate) fn compare<F: Float>(
        &mut self,
        mnemonic: M,
        a: u64,
        b: u64,
        simd_exception: &mut bool,
    ) -> Result<(), String> {
        let mut flags = 0;
        let a = self.input(F::from_bits(a as u128), &mut flags);
        let b = self.input(F::from_bits(b as u128), &mut flags);
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
        self.accept(Status::OK.and(F::ZERO), flags, simd_exception)?;
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
    #[inline(always)]
    pub(crate) fn convert<F: Float + FloatConvert<D> + FloatConvert<Unbounded<D>>, D: Float>(
        &mut self,
        bits: u64,
        simd_exception: &mut bool,
    ) -> Result<u64, String> {
        let mut flags = 0;
        let source = self.input(F::from_bits(bits as u128), &mut flags);
        if flags & 2 != 0 && self.mxcsr & (1 << 8) == 0 {
            self.accept(Status::OK.and(D::ZERO), flags, simd_exception)?;
        }
        let mut result: StatusAnd<D> = source.convert_r(self.round(), &mut false);
        if (result.value.abs() <= D::smallest_normalized()
            || result.value.abs() == D::largest()
            || result.status.intersects(Status::OVERFLOW))
            && source.is_finite()
            && !source.is_zero()
        {
            let wide: StatusAnd<Unbounded<D>> = source.convert_r(self.round(), &mut false);
            self.range::<D>(wide, &mut result);
        }
        Ok(self.accept(result, flags, simd_exception)?.to_bits() as u64)
    }

    fn range<F: Float>(&self, wide: StatusAnd<Unbounded<F>>, result: &mut StatusAnd<F>) {
        if !wide.value.is_finite_non_zero() {
            return;
        }
        let overflow = wide.value.ilogb() > F::MAX_EXP;
        let tiny = wide.value.ilogb() < F::MIN_EXP;
        if overflow {
            result.status |= Status::OVERFLOW;
        }
        if tiny && (self.mxcsr & (1 << 11) == 0 || result.status.intersects(Status::INEXACT)) {
            result.status |= Status::UNDERFLOW;
        }
        if (overflow && self.mxcsr & (1 << 10) == 0) || (tiny && self.mxcsr & (1 << 11) == 0) {
            result.status = (result.status & !Status::INEXACT) | (wide.status & Status::INEXACT);
        }
    }
    #[inline(always)]
    pub(crate) fn sqrt<F: Float>(
        &mut self,
        bits: u64,
        fraction: u32,
        bias: i32,
        simd_exception: &mut bool,
    ) -> Result<u64, String> {
        let mut flags = 0;
        let value = self.input(F::from_bits(bits as u128), &mut flags);
        let result = if value.is_nan() {
            value.add_r(F::ZERO, self.round())
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
                && (self.round() == Round::TowardPositive
                    || (self.round() == Round::NearestTiesToEven && remainder > root));
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
        Ok(self.accept(result, flags, simd_exception)?.to_bits() as u64)
    }
}
