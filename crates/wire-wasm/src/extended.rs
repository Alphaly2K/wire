//! Concrete FP80 numerical functions. CPU registers, stack movement, memory and
//! deferred exception delivery remain in generated code.
use crate::x87::Fpu;
use rustc_apfloat::ieee::{Double, IeeeFloat, Semantics, Single, X87DoubleExtended as Ext};
use rustc_apfloat::{Float, FloatConvert, Round, Status, StatusAnd};
use std::marker::PhantomData;
mod transcendental;
pub use transcendental::{atan, cos, exp2m1, log_multiply, sin, sincos, tan};

pub const INDEFINITE: u128 = 0xffff_c000_0000_0000_0000;
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Output {
    pub value: [u64; 2],
    pub sw: u32,
    pub commit: u32,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Pair {
    pub result: Output,
    pub pushed: [u64; 2],
}

fn input(cw: u32, sw: u32) -> Fpu {
    Fpu {
        cw: cw as u16,
        sw: sw as u16 & !0x23f,
        ..Fpu::default()
    }
}
fn output(fpu: Fpu, old: u32, value: u128, commit: bool) -> Output {
    Output {
        value: [value as u64, (value >> 64) as u64],
        sw: u32::from(fpu.sw) | (old & 0x3f),
        commit: u32::from(commit),
    }
}
fn blocked(fpu: &Fpu, flags: u16) -> bool {
    fpu.sw & !fpu.cw & flags != 0
}
fn unsupported(value: u128) -> bool {
    (value >> 64) & 0x7fff != 0 && value & (1 << 63) == 0
}
fn denormal(value: u128) -> bool {
    value & (0x7fff << 64) == 0 && value as u64 != 0
}
fn canonical(value: u128) -> Ext {
    Ext::from_bits(if value & (0x7fff << 64) == 0 && value & (1 << 63) != 0 {
        value | (1 << 64)
    } else {
        value
    })
}
fn invalid(fpu: &mut Fpu, old: u32, value: u128) -> Output {
    fpu.exception(1);
    output(*fpu, old, value, !blocked(fpu, 1))
}
fn propagate_nan(fpu: &mut Fpu, old: u32, a: Ext, b: Ext) -> Output {
    if a.is_signaling() || b.is_signaling() {
        fpu.exception(1);
    }
    let result = if !a.is_nan() {
        b
    } else if !b.is_nan() {
        a
    } else if a.is_signaling() != b.is_signaling() {
        if a.is_signaling() {
            b
        } else {
            a
        }
    } else if a.to_bits() as u64 >= b.to_bits() as u64 {
        a
    } else {
        b
    };
    output(*fpu, old, result.to_bits() | (1 << 62), !blocked(fpu, 1))
}

pub fn load_real<F: Float + FloatConvert<Ext>>(cw: u32, sw: u32, bits: u128) -> Output {
    let mut fpu = input(cw, sw);
    let source = F::from_bits(bits);
    let converted: StatusAnd<Ext> = source.convert_r(Round::NearestTiesToEven, &mut false);
    fpu.status(converted.status);
    if source.is_denormal() {
        fpu.exception(2);
    }
    // FLD pushes denormal inputs even when DM is clear (SDM FLD description).
    output(fpu, sw, converted.value.to_bits(), !blocked(&fpu, 1))
}
pub fn load_integer(cw: u32, sw: u32, value: i64) -> Output {
    output(
        input(cw, sw),
        sw,
        Ext::from_i128(value as i128).value.to_bits(),
        true,
    )
}
pub fn load_bcd(cw: u32, sw: u32, bits: u128) -> Output {
    let mut magnitude = 0u64;
    let mut place = 1u64;
    for digit in 0..18 {
        magnitude += ((bits >> (digit * 4)) as u64 & 15) * place;
        place *= 10;
    }
    let value = Ext::from_u128(u128::from(magnitude)).value;
    let value = if bits & (1 << 79) != 0 { -value } else { value };
    output(input(cw, sw), sw, value.to_bits(), true)
}
pub fn store_bcd(cw: u32, sw: u32, bits: u128) -> Output {
    let mut fpu = input(cw, sw);
    if unsupported(bits) {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    let x = canonical(bits);
    let converted = x.to_i128_r(64, fpu.round(), &mut false);
    if converted.status.contains(Status::INVALID_OP)
        || converted.value.unsigned_abs() > 999_999_999_999_999_999
    {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    fpu.status(converted.status);
    if converted.status.contains(Status::INEXACT)
        && Ext::from_i128(converted.value).value.abs() > x.abs()
    {
        fpu.sw |= 0x200;
    }
    let mut magnitude = converted.value.unsigned_abs();
    let mut value = if x.is_negative() { 1u128 << 79 } else { 0 };
    for digit in 0..18 {
        value |= (magnitude % 10) << (digit * 4);
        magnitude /= 10;
    }
    output(fpu, sw, value, true)
}

struct Wide<F>(PhantomData<F>);
impl<F: Float> Semantics for Wide<F> {
    const BITS: usize = F::PRECISION + 17;
    const EXP_BITS: usize = 17;
}
type Unbounded<F> = IeeeFloat<Wide<F>>;
struct P24;
impl Semantics for P24 {
    const BITS: usize = 39;
    const EXP_BITS: usize = 15;
}
struct P53;
impl Semantics for P53 {
    const BITS: usize = 68;
    const EXP_BITS: usize = 15;
}
type Narrow24 = IeeeFloat<P24>;
type Narrow53 = IeeeFloat<P53>;

fn odd<F: Float>(r: StatusAnd<F>) -> F {
    if r.status.contains(Status::INEXACT) && r.value.is_finite() {
        F::from_bits(r.value.to_bits() | 1)
    } else {
        r.value
    }
}
fn precision<F>(r: StatusAnd<Unbounded<Ext>>, round: Round) -> StatusAnd<Unbounded<Ext>>
where
    Unbounded<Ext>: FloatConvert<F>,
    F: Float + FloatConvert<Unbounded<Ext>>,
{
    let narrowed: StatusAnd<F> = odd(r).convert_r(round, &mut false);
    let expanded: StatusAnd<Unbounded<Ext>> = narrowed
        .value
        .convert_r(Round::NearestTiesToEven, &mut false);
    (r.status | narrowed.status).and(expanded.value)
}
fn operation<F: Float, const KIND: usize>(a: F, b: F, round: Round) -> StatusAnd<F> {
    match KIND {
        0 => a.add_r(b, round),
        1 => a.mul_r(b, round),
        4 => a.sub_r(b, round),
        6 => a.div_r(b, round),
        _ => unreachable!(),
    }
}
fn narrow(value: Unbounded<Ext>, pc: u16, round: Round) -> StatusAnd<Ext> {
    match pc {
        0 => {
            let r: StatusAnd<Narrow24> = value.convert_r(round, &mut false);
            r.status.and(
                r.value
                    .convert_r(Round::NearestTiesToEven, &mut false)
                    .value,
            )
        }
        2 => {
            let r: StatusAnd<Narrow53> = value.convert_r(round, &mut false);
            r.status.and(
                r.value
                    .convert_r(Round::NearestTiesToEven, &mut false)
                    .value,
            )
        }
        _ => value.convert_r(round, &mut false),
    }
}
fn range<const KIND: usize>(fpu: &mut Fpu, a: Ext, b: Ext) -> Ext {
    let a: Unbounded<Ext> = a.convert_r(Round::NearestTiesToEven, &mut false).value;
    let b: Unbounded<Ext> = b.convert_r(Round::NearestTiesToEven, &mut false).value;
    let trunc = operation::<_, KIND>(a, b, Round::TowardZero);
    let round = fpu.round();
    let (wide, toward) = match (fpu.cw >> 8) & 3 {
        0 => (
            precision::<Unbounded<Single>>(trunc, round),
            precision::<Unbounded<Single>>(trunc, Round::TowardZero),
        ),
        2 => (
            precision::<Unbounded<Double>>(trunc, round),
            precision::<Unbounded<Double>>(trunc, Round::TowardZero),
        ),
        _ => (operation::<_, KIND>(a, b, round), trunc),
    };
    let finite = wide.value.is_finite_non_zero();
    let overflow = finite && wide.value.ilogb() > Ext::MAX_EXP;
    let tiny = finite && wide.value.ilogb() < Ext::MIN_EXP;
    let unmasked = overflow && fpu.cw & 8 == 0 || tiny && fpu.cw & 16 == 0;
    let mut status = wide.status;
    let mut up = wide.value.abs() > toward.value.abs();
    let result = if unmasked {
        status |= if overflow {
            Status::OVERFLOW
        } else {
            Status::UNDERFLOW
        };
        wide.value
            .scalbn(if overflow { -24576 } else { 24576 })
            .convert_r(round, &mut false)
            .value
    } else {
        let pc = (fpu.cw >> 8) & 3;
        let converted = narrow(
            if matches!(pc, 1 | 3) && !tiny {
                wide.value
            } else {
                odd(trunc)
            },
            pc,
            round,
        );
        let truncated = narrow(trunc.value, pc, Round::TowardZero);
        up = converted.value.abs() > truncated.value.abs();
        status |= converted.status;
        if overflow {
            status |= Status::OVERFLOW | Status::INEXACT;
        }
        if !tiny {
            status &= !Status::UNDERFLOW;
        }
        converted.value
    };
    fpu.status(status);
    if status.contains(Status::INEXACT) && up {
        fpu.sw |= 0x200;
    }
    result
}

pub fn binary<const KIND: usize>(cw: u32, sw: u32, av: u128, bv: u128) -> Output {
    binary_inner::<KIND>(cw, sw, av, bv, false)
}
fn binary_inner<const KIND: usize>(
    cw: u32,
    sw: u32,
    av: u128,
    bv: u128,
    memory_denormal: bool,
) -> Output {
    let mut fpu = input(cw, sw);
    if unsupported(av) || unsupported(bv) {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    let a = canonical(av);
    let b = canonical(bv);
    if a.is_nan() || b.is_nan() {
        return propagate_nan(&mut fpu, sw, a, b);
    }
    let invalid_operation = match KIND {
        0 => a.is_infinite() && b.is_infinite() && a.is_negative() != b.is_negative(),
        1 => a.is_infinite() && b.is_zero() || b.is_infinite() && a.is_zero(),
        4 => a.is_infinite() && b.is_infinite() && a.is_negative() == b.is_negative(),
        6 => a.is_zero() && b.is_zero() || a.is_infinite() && b.is_infinite(),
        _ => unreachable!(),
    };
    if invalid_operation {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    if KIND == 6 && b.is_zero() && a.is_finite_non_zero() {
        fpu.exception(4);
        let sign = if a.is_negative() != b.is_negative() {
            -Ext::ZERO
        } else {
            Ext::ZERO
        };
        return output(
            fpu,
            sw,
            Ext::INFINITY.copy_sign(sign).to_bits(),
            !blocked(&fpu, 4),
        );
    }
    if denormal(av) || denormal(bv) || memory_denormal {
        fpu.exception(2);
    }
    if blocked(&fpu, 2) {
        return output(fpu, sw, av, false);
    }
    let value = fpu.binary(a, b, KIND);
    // Ordinary finite operations reuse the existing FP80 numerical core. The
    // boundary path retains a wide exponent for precise PC rounding and scaling.
    let boundary = a.is_finite()
        && b.is_finite()
        && (value.is_denormal()
            || value.is_zero() && fpu.sw & 0x20 != 0
            || fpu.sw & 0x18 != 0
            || value.is_finite_non_zero()
                && (value.ilogb() == Ext::MAX_EXP || value.ilogb() == Ext::MIN_EXP));
    if boundary {
        fpu = input(cw, sw);
        if denormal(av) || denormal(bv) || memory_denormal {
            fpu.exception(2);
        }
        let value = range::<KIND>(&mut fpu, a, b);
        output(fpu, sw, value.to_bits(), true)
    } else {
        output(fpu, sw, value.to_bits(), true)
    }
}

fn store_real<F>(cw: u32, sw: u32, bits: u128) -> Output
where
    Ext: FloatConvert<F> + FloatConvert<Unbounded<F>>,
    F: Float + FloatConvert<Ext>,
{
    let mut fpu = input(cw, sw);
    let source = if unsupported(bits) {
        fpu.exception(1);
        Ext::from_bits(INDEFINITE)
    } else {
        canonical(bits)
    };
    let mut result: StatusAnd<F> = source.convert_r(fpu.round(), &mut false);
    let toward: StatusAnd<F> = source.convert_r(Round::TowardZero, &mut false);
    let wide: StatusAnd<Unbounded<F>> = source.convert_r(fpu.round(), &mut false);
    if wide.value.is_finite_non_zero() {
        let overflow = wide.value.ilogb() > F::MAX_EXP;
        let tiny = wide.value.ilogb() < F::MIN_EXP;
        if overflow {
            result.status |= Status::OVERFLOW | Status::INEXACT;
        }
        if tiny && (fpu.cw & 16 == 0 || result.status.contains(Status::INEXACT)) {
            result.status |= Status::UNDERFLOW;
        }
        if !tiny {
            result.status &= !Status::UNDERFLOW;
        }
    }
    fpu.status(result.status);
    let no_store = blocked(&fpu, 1 | 8 | 16);
    if blocked(&fpu, 8 | 16) {
        fpu.sw &= !0x220;
    } else if result.status.contains(Status::INEXACT) && result.value.abs() > toward.value.abs() {
        fpu.sw |= 0x200;
    }
    output(fpu, sw, result.value.to_bits(), !no_store)
}
pub fn store_single(cw: u32, sw: u32, bits: u128) -> Output {
    store_real::<Single>(cw, sw, bits)
}
pub fn store_double(cw: u32, sw: u32, bits: u128) -> Output {
    store_real::<Double>(cw, sw, bits)
}
pub fn store_integer<const BITS: usize, const TRUNCATE: bool>(
    cw: u32,
    sw: u32,
    bits: u128,
) -> Output {
    let mut fpu = input(cw, sw);
    if unsupported(bits) {
        return invalid(&mut fpu, sw, 1u128 << (BITS - 1));
    }
    let x = canonical(bits);
    let mut exact = false;
    let result = x.to_i128_r(
        BITS,
        if TRUNCATE {
            Round::TowardZero
        } else {
            fpu.round()
        },
        &mut exact,
    );
    fpu.status(result.status);
    let value = if result.status.contains(Status::INVALID_OP) {
        1u128 << (BITS - 1)
    } else {
        result.value as u128
    };
    if !TRUNCATE
        && result.status.contains(Status::INEXACT)
        && Ext::from_i128(result.value).value.abs() > x.abs()
    {
        fpu.sw |= 0x200;
    }
    output(fpu, sw, value, !blocked(&fpu, 1))
}

fn memory_value<const SIZE: usize, const INTEGER: bool>(bits: u64) -> (u128, bool) {
    if INTEGER {
        let value = if SIZE == 2 {
            bits as i16 as i128
        } else {
            bits as i32 as i128
        };
        return (Ext::from_i128(value).value.to_bits(), false);
    }
    let (sign, exponent, fraction, shift, nan) = if SIZE == 4 {
        (
            u128::from(bits >> 31),
            (bits >> 23) & 255,
            bits & 0x7fffff,
            40,
            (bits >> 23) & 255 == 255 && bits & 0x7fffff != 0,
        )
    } else {
        (
            u128::from(bits >> 63),
            (bits >> 52) & 2047,
            bits & 0xfffffffffffff,
            11,
            (bits >> 52) & 2047 == 2047 && bits & 0xfffffffffffff != 0,
        )
    };
    if nan {
        return (
            (sign << 79) | (0x7fff << 64) | (1 << 63) | (u128::from(fraction) << shift),
            false,
        );
    }
    let value: Ext = if SIZE == 4 {
        Single::from_bits(u128::from(bits as u32))
            .convert_r(Round::NearestTiesToEven, &mut false)
            .value
    } else {
        Double::from_bits(u128::from(bits))
            .convert_r(Round::NearestTiesToEven, &mut false)
            .value
    };
    (value.to_bits(), exponent == 0 && fraction != 0)
}
pub fn binary_memory<
    const KIND: usize,
    const SIZE: usize,
    const INTEGER: bool,
    const REVERSE: bool,
>(
    cw: u32,
    sw: u32,
    a: u128,
    bits: u64,
) -> Output {
    let (b, denormal) = memory_value::<SIZE, INTEGER>(bits);
    if REVERSE {
        binary_inner::<KIND>(cw, sw, b, a, denormal)
    } else {
        binary_inner::<KIND>(cw, sw, a, b, denormal)
    }
}
pub fn constant<const INDEX: usize>(cw: u32, sw: u32) -> Output {
    let mut fpu = input(cw, sw);
    fpu.tags = 0xffff;
    fpu.load_constant(INDEX);
    let value = fpu.st_bits(0);
    fpu.sw = (fpu.sw & !0x3800) | (sw as u16 & 0x3800);
    output(fpu, sw, value, true)
}
pub fn sqrt(cw: u32, sw: u32, bits: u128) -> Output {
    let mut fpu = input(cw, sw);
    let x = canonical(bits);
    if unsupported(bits) || x.is_negative() && !x.is_zero() && !x.is_nan() {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    if x.is_nan() {
        if x.is_signaling() {
            fpu.exception(1);
        }
        return output(fpu, sw, x.to_bits() | (1 << 62), !blocked(&fpu, 1));
    }
    if denormal(bits) {
        fpu.exception(2);
    }
    if blocked(&fpu, 2) {
        return output(fpu, sw, bits, false);
    }
    let result = fpu.sqrt(x);
    output(fpu, sw, result.to_bits(), true)
}
pub fn round_integral(cw: u32, sw: u32, bits: u128) -> Output {
    let mut fpu = input(cw, sw);
    if unsupported(bits) {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    let x = canonical(bits);
    if x.is_nan() {
        if x.is_signaling() {
            fpu.exception(1);
        }
        return output(fpu, sw, x.to_bits() | (1 << 62), !blocked(&fpu, 1));
    }
    if denormal(bits) {
        fpu.exception(2);
    }
    if blocked(&fpu, 2) {
        return output(fpu, sw, bits, false);
    }
    let result = fpu.accept(
        x.round_to_integral(fpu.round()),
        x.round_to_integral(Round::TowardZero).value,
    );
    output(fpu, sw, result.to_bits(), true)
}
pub fn remainder<const NEAREST: bool>(cw: u32, sw: u32, av: u128, bv: u128) -> Output {
    let mut fpu = input(cw, sw);
    fpu.sw &= !0x400;
    if unsupported(av) || unsupported(bv) {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    let a = canonical(av);
    let b = canonical(bv);
    if a.is_nan() || b.is_nan() {
        return propagate_nan(&mut fpu, sw, a, b);
    }
    if a.is_infinite() || b.is_zero() {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    if denormal(av) || denormal(bv) {
        fpu.exception(2);
    }
    if blocked(&fpu, 2) {
        return output(fpu, sw, av, false);
    }
    let (mut result, quotient, partial) = if a.is_zero() || b.is_infinite() {
        (a, 0u128, false)
    } else {
        let sig_a = a.to_bits() as u64;
        let sig_a = u128::from(sig_a << sig_a.leading_zeros());
        let sig_b = b.to_bits() as u64;
        let sig_b = u128::from(sig_b << sig_b.leading_zeros());
        let exponent = a.ilogb();
        let difference = exponent - b.ilogb();
        let partial = difference >= 64;
        let (residual, quotient, unit, flip) = if difference < 0 {
            if !NEAREST || difference < -1 || sig_a <= sig_b {
                (sig_a, 0, exponent - 63, false)
            } else {
                (2 * sig_b - sig_a, 1, exponent - 63, true)
            }
        } else {
            // N=32 bounds one partial reduction. Both the dividend and the
            // full quotient remain exact integers within 128 bits.
            let shift = if partial { 32 } else { difference };
            let numerator = sig_a << shift;
            let mut quotient = numerator / sig_b;
            let mut residual = numerator - quotient * sig_b;
            let flip = NEAREST
                && !partial
                && (2 * residual > sig_b || 2 * residual == sig_b && quotient & 1 != 0);
            if flip {
                quotient += 1;
                residual = sig_b - residual;
            }
            (residual, quotient, exponent - shift - 63, flip)
        };
        let value = Ext::from_u128(residual)
            .value
            .scalbn(unit)
            .copy_sign(if flip { -a } else { a });
        (value, quotient, partial)
    };
    if partial {
        fpu.sw |= 0x400;
    } else {
        let q = quotient as u16;
        fpu.sw = (fpu.sw & !0x4700) | ((q & 4) << 6) | ((q & 2) << 13) | ((q & 1) << 9);
    }
    if result.is_denormal() && fpu.cw & 16 == 0 {
        fpu.exception(16);
        result = result.scalbn(24576);
    }
    output(fpu, sw, result.to_bits(), true)
}

pub fn scale(cw: u32, sw: u32, av: u128, bv: u128) -> Output {
    let mut fpu = input(cw, sw);
    if unsupported(av) || unsupported(bv) {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    let a = canonical(av);
    let b = canonical(bv);
    if a.is_nan() || b.is_nan() {
        return propagate_nan(&mut fpu, sw, a, b);
    }
    if b.is_infinite() && (a.is_zero() && !b.is_negative() || a.is_infinite() && b.is_negative()) {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    if denormal(av) || denormal(bv) {
        fpu.exception(2);
    }
    if blocked(&fpu, 2) {
        return output(fpu, sw, av, false);
    }
    if b.is_infinite() {
        let value = if b.is_negative() {
            Ext::ZERO
        } else {
            Ext::INFINITY
        };
        return output(fpu, sw, value.copy_sign(a).to_bits(), true);
    }
    if a.is_zero() || a.is_infinite() {
        return output(fpu, sw, a.to_bits(), true);
    }
    let amount = b
        .to_i128_r(32, Round::TowardZero, &mut false)
        .value
        .clamp(-65536, 65536) as i32;
    let mut exponent = a.ilogb() + amount;
    let sig = a.to_bits() as u64;
    let sig = sig << sig.leading_zeros();
    let sign = a.to_bits() & (1 << 79);
    let overflow = exponent > Ext::MAX_EXP;
    let tiny = exponent < Ext::MIN_EXP;
    if overflow && fpu.cw & 8 == 0 || tiny && fpu.cw & 16 == 0 {
        fpu.exception(if overflow { 8 } else { 16 });
        exponent += if overflow { -24576 } else { 24576 };
        if !(Ext::MIN_EXP..=Ext::MAX_EXP).contains(&exponent) {
            let value = if overflow { Ext::INFINITY } else { Ext::ZERO };
            return output(fpu, sw, value.copy_sign(a).to_bits(), true);
        }
    } else if overflow {
        fpu.exception(0x28);
        let infinity = matches!(fpu.round(), Round::NearestTiesToEven)
            || fpu.round() == Round::TowardPositive && !a.is_negative()
            || fpu.round() == Round::TowardNegative && a.is_negative();
        if infinity {
            fpu.sw |= 0x200;
        }
        let value = if infinity {
            Ext::INFINITY
        } else {
            Ext::largest()
        };
        return output(fpu, sw, value.copy_sign(a).to_bits(), true);
    } else if tiny {
        let shift = (Ext::MIN_EXP - exponent) as u32;
        let (truncated, lost, nearest) = if shift < 64 {
            let tail = sig & ((1u64 << shift) - 1);
            let half = 1u64 << (shift - 1);
            (
                sig >> shift,
                tail != 0,
                tail > half || tail == half && (sig >> shift) & 1 != 0,
            )
        } else {
            (0, true, shift == 64 && sig > 1 << 63)
        };
        let up = lost
            && match fpu.round() {
                Round::NearestTiesToEven => nearest,
                Round::TowardNegative => a.is_negative(),
                Round::TowardPositive => !a.is_negative(),
                _ => false,
            };
        let rounded = truncated + u64::from(up);
        if lost {
            fpu.exception(0x30);
        }
        if up {
            fpu.sw |= 0x200;
        }
        let value = sign | u128::from(rounded) | if rounded & (1 << 63) != 0 { 1 << 64 } else { 0 };
        return output(fpu, sw, value, true);
    }
    output(
        fpu,
        sw,
        sign | (((exponent + 16383) as u128) << 64) | u128::from(sig),
        true,
    )
}

pub fn extract(cw: u32, sw: u32, bits: u128) -> Pair {
    let mut fpu = input(cw, sw);
    let a = canonical(bits);
    let (exponent, significand, commit) = if unsupported(bits) || a.is_nan() {
        let value = if unsupported(bits) {
            INDEFINITE
        } else {
            a.to_bits() | (1 << 62)
        };
        if unsupported(bits) || a.is_signaling() {
            fpu.exception(1);
        }
        (value, value, !blocked(&fpu, 1))
    } else if a.is_infinite() {
        (Ext::INFINITY.to_bits(), a.to_bits(), true)
    } else if a.is_zero() {
        fpu.exception(4);
        ((-Ext::INFINITY).to_bits(), a.to_bits(), !blocked(&fpu, 4))
    } else {
        if denormal(bits) {
            fpu.exception(2);
        }
        let exponent = a.ilogb();
        (
            Ext::from_i128(i128::from(exponent)).value.to_bits(),
            a.scalbn(-exponent).to_bits(),
            !blocked(&fpu, 2),
        )
    };
    Pair {
        result: output(fpu, sw, exponent, commit),
        pushed: [significand as u64, (significand >> 64) as u64],
    }
}
fn compare_inner<const QUIET: bool, const DENORMAL: bool>(
    cw: u32,
    sw: u32,
    av: u128,
    bv: u128,
    memory_denormal: bool,
) -> Output {
    let mut fpu = input(cw, sw);
    let a = canonical(av);
    let b = canonical(bv);
    if unsupported(av)
        || unsupported(bv)
        || a.is_signaling()
        || b.is_signaling()
        || !QUIET && (a.is_nan() || b.is_nan())
    {
        fpu.exception(1);
    } else if DENORMAL
        && !a.is_nan()
        && !b.is_nan()
        && (denormal(av) || denormal(bv) || memory_denormal)
    {
        fpu.exception(2);
    }
    let commit = !blocked(&fpu, 3);
    if commit {
        let flags = if unsupported(av) || unsupported(bv) {
            0x4500
        } else {
            match a.partial_cmp(&b) {
                None => 0x4500,
                Some(std::cmp::Ordering::Less) => 0x100,
                Some(std::cmp::Ordering::Equal) => 0x4000,
                _ => 0,
            }
        };
        fpu.sw = (fpu.sw & !0x4500) | flags;
    }
    output(fpu, sw, 0, commit)
}
pub fn compare<const QUIET: bool>(cw: u32, sw: u32, a: u128, b: u128) -> Output {
    compare_inner::<QUIET, true>(cw, sw, a, b, false)
}
pub fn compare_flags<const QUIET: bool>(cw: u32, sw: u32, a: u128, b: u128) -> Output {
    let mut result = compare_inner::<QUIET, false>(cw, sw, a, b, false);
    result.value = [u64::from((result.sw >> 8) & 0x45), 0];
    result.sw = (result.sw & !0x4500) | (sw & 0x4500);
    result
}
pub fn compare_memory<const SIZE: usize, const INTEGER: bool>(
    cw: u32,
    sw: u32,
    a: u128,
    b: u64,
) -> Output {
    let (b, de) = memory_value::<SIZE, INTEGER>(b);
    compare_inner::<false, true>(cw, sw, a, b, de)
}
