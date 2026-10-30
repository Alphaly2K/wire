use super::*;
use rustc_apfloat::ieee::Quad;

struct Precision;
impl Semantics for Precision {
    const BITS: usize = 127;
    const EXP_BITS: usize = 17;
}
type WideValue = IeeeFloat<Precision>;

fn wide(x: Ext) -> WideValue {
    x.convert_r(Round::NearestTiesToEven, &mut false).value
}
fn quad(x: Ext) -> Quad {
    x.convert_r(Round::NearestTiesToEven, &mut false).value
}
fn expanded(x: Quad) -> WideValue {
    let converted: StatusAnd<WideValue> = x.convert_r(Round::TowardZero, &mut false);
    odd(converted)
}
fn pair(result: Output, pushed: u128) -> Pair {
    Pair {
        result,
        pushed: [pushed as u64, (pushed >> 64) as u64],
    }
}
fn finish_value(fpu: &mut Fpu, value: WideValue, inexact: bool) -> Ext {
    fpu.sw &= !0x200;
    let round = fpu.round();
    let rounded: StatusAnd<Unbounded<Ext>> = value.convert_r(round, &mut false);
    let truncated: StatusAnd<Unbounded<Ext>> = value.convert_r(Round::TowardZero, &mut false);
    let overflow = rounded.value.is_finite_non_zero() && rounded.value.ilogb() > Ext::MAX_EXP;
    let tiny = rounded.value.is_finite_non_zero() && rounded.value.ilogb() < Ext::MIN_EXP;
    let mut status = rounded.status | if inexact { Status::INEXACT } else { Status::OK };
    let mut up = rounded.value.abs() > truncated.value.abs();
    let result = if overflow && fpu.cw & 8 == 0 || tiny && fpu.cw & 16 == 0 {
        status |= if overflow {
            Status::OVERFLOW
        } else {
            Status::UNDERFLOW
        };
        rounded
            .value
            .scalbn(if overflow { -24576 } else { 24576 })
            .convert_r(round, &mut false)
            .value
    } else {
        let result: StatusAnd<Ext> = value.convert_r(round, &mut false);
        let truncated: StatusAnd<Ext> = value.convert_r(Round::TowardZero, &mut false);
        status |= result.status;
        up = result.value.abs() > truncated.value.abs();
        if overflow {
            status |= Status::OVERFLOW | Status::INEXACT;
        }
        if tiny && status.contains(Status::INEXACT) {
            status |= Status::UNDERFLOW;
        }
        if !tiny {
            status &= !Status::UNDERFLOW;
        }
        result.value
    };
    fpu.status(status);
    if status.contains(Status::INEXACT) && up {
        fpu.sw |= 0x200;
    }
    result
}
fn signed_zero(negative: bool) -> u128 {
    if negative {
        1 << 79
    } else {
        0
    }
}

pub fn exp2m1(cw: u32, sw: u32, bits: u128) -> Output {
    let mut fpu = input(cw, sw);
    if unsupported(bits) {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    let x = canonical(bits);
    if x.is_nan() {
        return propagate_nan(&mut fpu, sw, x, Ext::ZERO);
    }
    if denormal(bits) {
        fpu.exception(2);
    }
    if blocked(&fpu, 2) {
        return output(fpu, sw, bits, false);
    }
    let one = Ext::from_u128(1).value;
    if x.is_zero() || x.abs() > one {
        return output(fpu, sw, x.to_bits(), true);
    }
    if x == one || x == -one {
        return output(
            fpu,
            sw,
            if x.is_negative() {
                0xbffe_8000_0000_0000_0000
            } else {
                one.to_bits()
            },
            true,
        );
    }
    let value = if x.ilogb() < -120 {
        (wide(x) * expanded(Fpu::ln2())).value
    } else {
        expanded(Fpu::exp2m1_quad(quad(x)))
    };
    let result = finish_value(&mut fpu, value, true);
    output(fpu, sw, result.to_bits(), true)
}

pub fn log_multiply<const PLUS_ONE: bool>(cw: u32, sw: u32, xv: u128, yv: u128) -> Output {
    let mut fpu = input(cw, sw);
    if unsupported(xv) || unsupported(yv) {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    let x = canonical(xv);
    let y = canonical(yv);
    if x.is_nan() || y.is_nan() {
        return propagate_nan(&mut fpu, sw, x, y);
    }
    let one = Ext::from_u128(1).value;
    let domain = if PLUS_ONE {
        x < -one
    } else {
        x.is_negative() && !x.is_zero()
    };
    let pole = if PLUS_ONE { x == -one } else { x.is_zero() };
    let log_zero = if PLUS_ONE { x.is_zero() } else { x == one };
    if domain
        || pole && y.is_zero()
        || log_zero && y.is_infinite()
        || x.is_infinite() && y.is_zero()
    {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    if pole && y.is_finite_non_zero() {
        fpu.exception(4);
        return output(
            fpu,
            sw,
            (-Ext::INFINITY).copy_sign(-y).to_bits(),
            !blocked(&fpu, 4),
        );
    }
    if denormal(xv) || denormal(yv) {
        fpu.exception(2);
    }
    if blocked(&fpu, 2) {
        return output(fpu, sw, yv, false);
    }
    let negative_log = if PLUS_ONE { x.is_negative() } else { x < one };
    let negative = y.is_negative() != negative_log;
    if y.is_infinite() || x.is_infinite() || pole {
        let value = if negative {
            -Ext::INFINITY
        } else {
            Ext::INFINITY
        };
        return output(fpu, sw, value.to_bits(), true);
    }
    if y.is_zero() || log_zero {
        return output(fpu, sw, signed_zero(negative), true);
    }
    let qx = quad(x);
    let mut exact = false;
    let logarithm = if PLUS_ONE && x.ilogb() < -120 {
        (wide(x) / expanded(Fpu::ln2())).value
    } else {
        let argument = if PLUS_ONE {
            qx + Quad::from_u128(1).value
        } else {
            Status::OK.and(qx)
        };
        exact = argument.status == Status::OK
            && argument.value == Quad::from_u128(1).value.scalbn(argument.value.ilogb());
        let logarithm = if PLUS_ONE && x.abs() <= Ext::from_bits(0x3ffe_8000_0000_0000_0000) {
            let z = (qx / (qx + Quad::from_u128(2).value).value).value;
            Fpu::atanh_log2(z)
        } else {
            Fpu::log2(argument.value)
        };
        expanded(logarithm)
    };
    let value = (logarithm * wide(y)).value;
    let result = finish_value(&mut fpu, value, !exact);
    output(fpu, sw, result.to_bits(), true)
}

fn tiny_trig(fpu: &mut Fpu, x: Ext, tangent: bool) -> Ext {
    let a: Unbounded<Ext> = x
        .abs()
        .convert_r(Round::NearestTiesToEven, &mut false)
        .value;
    let away = fpu.round() == Round::TowardPositive && !x.is_negative()
        || fpu.round() == Round::TowardNegative && x.is_negative();
    let inward = fpu.round() == Round::TowardZero
        || fpu.round() == Round::TowardPositive && x.is_negative()
        || fpu.round() == Round::TowardNegative && !x.is_negative();
    let rounded = if tangent && away {
        a.next_up().value
    } else if !tangent && inward {
        a.next_down().value
    } else {
        a
    };
    let tiny = rounded.ilogb() < Ext::MIN_EXP;
    let mut status = Status::INEXACT;
    let up;
    let result = if tiny && fpu.cw & 16 == 0 {
        status |= Status::UNDERFLOW;
        up = if tangent { away } else { !inward };
        rounded
            .scalbn(24576)
            .convert_r(fpu.round(), &mut false)
            .value
    } else {
        let result = if tangent && away {
            x.abs().next_up().value
        } else if !tangent && inward {
            x.abs().next_down().value
        } else {
            x.abs()
        };
        up = if tangent { away } else { !inward };
        if tiny {
            status |= Status::UNDERFLOW;
        }
        result.copy_sign(x)
    };
    fpu.status(status);
    if up {
        fpu.sw |= 0x200;
    }
    result.copy_sign(x)
}

fn trig<const KIND: u8>(cw: u32, sw: u32, bits: u128) -> Pair {
    let mut fpu = input(cw, sw);
    fpu.sw &= !0x400;
    let one = Ext::from_u128(1).value;
    if unsupported(bits) || canonical(bits).is_infinite() {
        return pair(
            invalid(&mut fpu, sw, INDEFINITE),
            if KIND == 3 { one.to_bits() } else { INDEFINITE },
        );
    }
    let x = canonical(bits);
    if x.is_nan() {
        let result = propagate_nan(&mut fpu, sw, x, Ext::ZERO);
        let pushed = if KIND == 3 {
            one.to_bits()
        } else {
            u128::from(result.value[0]) | (u128::from(result.value[1]) << 64)
        };
        return pair(result, pushed);
    }
    if denormal(bits) {
        fpu.exception(2);
    }
    if blocked(&fpu, 2) {
        return pair(output(fpu, sw, bits, false), 0);
    }
    if x.is_finite_non_zero() && x.ilogb() >= 63 {
        fpu.sw |= 0x400;
        return pair(output(fpu, sw, bits, false), 0);
    }
    if x.is_zero() {
        return pair(
            output(
                fpu,
                sw,
                if KIND == 1 {
                    one.to_bits()
                } else {
                    x.to_bits()
                },
                true,
            ),
            one.to_bits(),
        );
    }
    let (value, pushed) = if x.ilogb() < -33 {
        let s = if KIND != 1 {
            tiny_trig(&mut fpu, x, KIND == 3)
        } else {
            Ext::ZERO
        };
        let c = if KIND == 1 || KIND == 2 {
            fpu.sw &= !0x200;
            fpu.exception(32);
            if matches!(fpu.round(), Round::TowardNegative | Round::TowardZero) {
                one.next_down().value
            } else {
                fpu.sw |= 0x200;
                one
            }
        } else {
            one
        };
        (if KIND == 1 { c } else { s }, c)
    } else {
        let (s, c) = Fpu::sincos_quad(quad(x));
        if KIND == 3 {
            let value = (expanded(s) / expanded(c)).value;
            (finish_value(&mut fpu, value, true), one)
        } else {
            let sin = if KIND != 1 {
                finish_value(&mut fpu, expanded(s), true)
            } else {
                Ext::ZERO
            };
            let cos = if KIND != 0 {
                finish_value(&mut fpu, expanded(c), true)
            } else {
                Ext::ZERO
            };
            (if KIND == 1 { cos } else { sin }, cos)
        }
    };
    pair(output(fpu, sw, value.to_bits(), true), pushed.to_bits())
}
pub fn sin(cw: u32, sw: u32, bits: u128) -> Output {
    trig::<0>(cw, sw, bits).result
}
pub fn cos(cw: u32, sw: u32, bits: u128) -> Output {
    trig::<1>(cw, sw, bits).result
}
pub fn sincos(cw: u32, sw: u32, bits: u128) -> Pair {
    trig::<2>(cw, sw, bits)
}
pub fn tan(cw: u32, sw: u32, bits: u128) -> Pair {
    trig::<3>(cw, sw, bits)
}

fn half_pi() -> WideValue {
    expanded(Quad::from_bits(crate::x87::HALF_PI_HI))
}
fn atan_series(x: WideValue) -> WideValue {
    let squared = -(x * x).value;
    let mut power = x;
    let mut sum = x;
    for n in 1..128 {
        power = (power * squared).value;
        let term = (power / WideValue::from_u128(2 * n + 1).value).value;
        let next = (sum + term).value;
        if next == sum {
            break;
        }
        sum = next;
    }
    sum
}
pub fn atan(cw: u32, sw: u32, xv: u128, yv: u128) -> Output {
    let mut fpu = input(cw, sw);
    if unsupported(xv) || unsupported(yv) {
        return invalid(&mut fpu, sw, INDEFINITE);
    }
    let x = canonical(xv);
    let y = canonical(yv);
    if x.is_nan() || y.is_nan() {
        return propagate_nan(&mut fpu, sw, x, y);
    }
    if denormal(xv) || denormal(yv) {
        fpu.exception(2);
    }
    if blocked(&fpu, 2) {
        return output(fpu, sw, yv, false);
    }
    if (y.is_zero() || x.is_infinite() && !y.is_infinite()) && !x.is_negative() {
        return output(fpu, sw, signed_zero(y.is_negative()), true);
    }
    let angle = if y.is_zero() || x.is_infinite() && !y.is_infinite() {
        half_pi().scalbn(1)
    } else if x.is_zero() || y.is_infinite() && !x.is_infinite() {
        half_pi()
    } else if x.is_infinite() && y.is_infinite() {
        let fourth = half_pi().scalbn(-1);
        if x.is_negative() {
            (half_pi() + fourth).value
        } else {
            fourth
        }
    } else {
        let swapped = y.abs() > x.abs();
        let (numerator, denominator) = if swapped {
            (x.abs(), y.abs())
        } else {
            (y.abs(), x.abs())
        };
        let ratio = wide(numerator).div_r(wide(denominator), Round::TowardZero);
        let r = odd(ratio);
        let one = WideValue::from_u128(1).value;
        let mut value = if r > one.scalbn(-1) {
            let z = ((r - one).value / (r + one).value).value;
            (half_pi().scalbn(-1) + atan_series(z)).value
        } else {
            let mut value = atan_series(r);
            if value == r && ratio.status == Status::OK {
                value = value.next_down().value;
            }
            value
        };
        if swapped {
            value = (half_pi() - value).value;
        }
        if x.is_negative() {
            value = (half_pi().scalbn(1) - value).value;
        }
        value
    };
    let angle = if y.is_negative() { -angle } else { angle };
    let result = finish_value(&mut fpu, angle, true);
    output(fpu, sw, result.to_bits(), true)
}
