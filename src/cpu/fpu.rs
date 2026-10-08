//! Software x87 stack. Basic arithmetic uses APFloat's extended format;
//! reduced precision keeps the extended exponent range and uses round-to-odd
//! before the final rounding, avoiding double rounding of 64-bit intermediates.
use super::{Cpu, CpuError, AF, CF, EAX, OF, PF, SF, ZF};
use crate::mem::GuestMem;
use rustc_apfloat::ieee::{Double, IeeeFloat, Quad, Semantics, Single, X87DoubleExtended};
use rustc_apfloat::{Float, FloatConvert, Round, Status, StatusAnd};
use std::cmp::Ordering;
use std::sync::LazyLock;

type Ext = X87DoubleExtended;
const C0: u16 = 0x100;
const C1: u16 = 0x200;
const C2: u16 = 0x400;
const C3: u16 = 0x4000;
const INDEFINITE: u128 = 0xffff_c000_0000_0000_0000;
static LN2: LazyLock<Quad> = LazyLock::new(|| {
    Quad::from_str_r(
        "0.693147180559945309417232121458176568075500134360255254120",
        Round::NearestTiesToEven,
    )
    .unwrap()
    .value
});
// Split pi/2 retains >200 bits during fused argument reduction, including
// extended inputs near the x87 2^63 limit. No binary64 conversion is used.
const HALF_PI_HI: u128 = 0x3fff_921f_b544_42d1_8469_898c_c517_01b8;
static HALF_PI_LO: LazyLock<Quad> = LazyLock::new(|| {
    Quad::from_str_r("4.33590506506189051239852201302167598438116167312826517088796786054022068557566411533235e-35",Round::NearestTiesToEven).unwrap().value
});
static LOAD_CONSTANTS: LazyLock<[[Ext; 7]; 4]> = LazyLock::new(|| {
    [
        Round::NearestTiesToEven,
        Round::TowardNegative,
        Round::TowardPositive,
        Round::TowardZero,
    ]
    .map(|round| {
        [
            "1",
            "3.321928094887362347870319429489390175864831393",
            "1.442695040888963407359924681001892137426645954",
            "3.141592653589793238462643383279502884197169399",
            "0.301029995663981195213738894724493026768189881",
            "0.693147180559945309417232121458176568075500134",
            "0",
        ]
        .map(|text| Ext::from_str_r(text, round).unwrap().value)
    })
});

// ceil(2^64 / n), computed at compile time. Each quotient estimate below is
// at most one too large and is corrected with an exact integer product.
static SMALL_RECIPROCALS: [u64; 256] = {
    let mut table = [0; 256];
    let mut n = 2;
    while n < 256 {
        table[n] = (1u128 << 64).div_ceil(n as u128) as u64;
        n += 1;
    }
    table
};

#[inline]
fn divide_small_digit(value: u64, divisor: u64, reciprocal: u64) -> (u64, u64) {
    // Callers supply at most 57 bits, leaving room for the trial product.
    // ceil reciprocal adds <value/2^64 <1 to the true quotient. Therefore
    // floor(product/2^64) is either floor(value/divisor) or that plus one.
    let trial = ((value as u128 * reciprocal as u128) >> 64) as u64;
    let product = trial * divisor;
    if product > value {
        (trial - 1, value + divisor - product)
    } else {
        (trial, value - product)
    }
}

/// The series divides binary128 terms by integers <=255. Divide their exact
/// significand using u128 quotient/remainder, then round once to nearest-even.
/// This produces the same bits as APFloat, without its general float division.
/// Subnormal/underflow and non-finite inputs retain the general implementation.
#[inline]
fn quad_divide_small(x: Quad, n: u32) -> Quad {
    let bits = x.to_bits();
    let mut exponent = ((bits >> 112) & 0x7fff) as u32;
    let mut shift = if n == 0 {
        0
    } else {
        32 - (n - 1).leading_zeros()
    };
    if !(1..=255).contains(&n) || exponent <= shift || exponent == 0x7fff {
        return (x / Quad::from_u128(n as u128).value).value;
    }
    if n.is_power_of_two() {
        return Quad::from_bits(bits - ((shift as u128) << 112));
    }
    const FRACTION: u128 = (1 << 112) - 1;
    let significand = (bits & FRACTION) | (1 << 112);
    let numerator = significand << shift; // at most 121 bits
    let mut divisor = n as u128;
    // Base-2^32 long division with exact reciprocal-corrected digits. A
    // remainder is <256, so its combination with the next digit fits in
    // 40 bits. The first dividend fits in 57 bits (121-bit numerator).
    let d = n as u64;
    let reciprocal = SMALL_RECIPROCALS[n as usize];
    let high = (numerator >> 64) as u64;
    let (q_high, r_high) = divide_small_digit(high, d, reciprocal);
    let middle = (r_high << 32) | ((numerator as u64) >> 32);
    let (q_middle, r_middle) = divide_small_digit(middle, d, reciprocal);
    let low = (r_middle << 32) | (numerator as u64 & 0xffff_ffff);
    let (q_low, r_low) = divide_small_digit(low, d, reciprocal);
    let mut quotient = ((q_high as u128) << 64) | ((q_middle as u128) << 32) | q_low as u128;
    let mut remainder = r_low as u128;
    if quotient >= 1 << 113 {
        // Normalize one bit without losing its contribution to the remainder.
        remainder += (quotient & 1) * divisor;
        divisor *= 2;
        quotient >>= 1;
        shift -= 1;
    }
    exponent -= shift;
    if remainder * 2 > divisor || (remainder * 2 == divisor && quotient & 1 != 0) {
        quotient += 1;
        if quotient == 1 << 113 {
            quotient >>= 1;
            exponent += 1;
        }
    }
    Quad::from_bits((bits & (1 << 127)) | ((exponent as u128) << 112) | (quotient & FRACTION))
}
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fpu {
    pub cw: u16,
    pub sw: u16,
    /// Physical register order; register payloads remain intact when freed.
    pub regs: [u128; 8],
    pub tags: u16,
    pub ip: u64,
    pub dp: u64,
    pub opcode: u16,
}
impl Default for Fpu {
    fn default() -> Self {
        Self {
            cw: 0x037f,
            sw: 0,
            regs: [0; 8],
            tags: 0xffff,
            ip: 0,
            dp: 0,
            opcode: 0,
        }
    }
}
impl Fpu {
    pub fn top(&self) -> usize {
        ((self.sw >> 11) & 7) as usize
    }
    pub(crate) fn set_top(&mut self, n: usize) {
        self.sw = (self.sw & !0x3800) | ((n as u16 & 7) << 11);
    }
    pub(crate) fn index(&self, i: usize) -> usize {
        (self.top() + i) & 7
    }
    fn tag(&self, i: usize) -> u16 {
        (self.tags >> (2 * i)) & 3
    }
    pub(crate) fn set_tag(&mut self, i: usize, tag: u16) {
        self.tags = (self.tags & !(3 << (2 * i))) | (tag << (2 * i));
    }
    pub(crate) fn exception(&mut self, bits: u16) {
        self.sw |= bits;
        if bits & !self.cw & 0x3f != 0 {
            self.sw |= 0x8080;
        }
    }
    pub(crate) fn status(&mut self, s: Status) {
        self.exception((s.bits() as u16 & 1) | ((s.bits() as u16 & 0x1e) << 1));
    }
    pub(crate) fn operands(&mut self, values: &[Ext]) {
        self.sw &= !C1;
        if values.iter().any(|x| x.is_denormal()) {
            self.exception(2);
        }
    }
    pub fn st_bits(&self, i: usize) -> u128 {
        self.regs[self.index(i)]
    }
    pub fn set_st_bits(&mut self, i: usize, bits: u128) {
        let n = self.index(i);
        let v = Ext::from_bits(bits);
        self.regs[n] = bits;
        self.set_tag(
            n,
            if v.is_zero() {
                1
            } else if v.is_nan() || v.is_infinite() || v.is_denormal() {
                2
            } else {
                0
            },
        );
    }
    pub(crate) fn get(&mut self, i: usize) -> Ext {
        if self.tag(self.index(i)) == 3 {
            self.sw &= !C1;
            self.exception(0x41);
            Ext::from_bits(INDEFINITE)
        } else {
            Ext::from_bits(self.st_bits(i))
        }
    }
    pub(crate) fn put(&mut self, i: usize, value: Ext) {
        self.set_st_bits(i, value.to_bits());
    }
    pub(crate) fn push(&mut self, value: Ext) {
        let n = (self.top() + 7) & 7;
        let value = if self.tag(n) != 3 {
            self.sw |= C1;
            self.exception(0x41);
            Ext::from_bits(INDEFINITE)
        } else {
            self.sw &= !C1;
            value
        };
        self.set_top(n);
        self.put(0, value);
    }
    pub(crate) fn pop(&mut self) {
        self.set_tag(self.top(), 3);
        self.set_top(self.top() + 1);
    }
    pub(crate) fn round(&self) -> Round {
        match (self.cw >> 10) & 3 {
            0 => Round::NearestTiesToEven,
            1 => Round::TowardNegative,
            2 => Round::TowardPositive,
            _ => Round::TowardZero,
        }
    }
    pub(crate) fn load_real_bits(&mut self, bits: u128, size: usize) -> Ext {
        if size == 10 {
            return Ext::from_bits(bits);
        }
        let (denormal, converted): (bool, StatusAnd<Ext>) = if size == 4 {
            let x = Single::from_bits(bits);
            (
                x.is_denormal(),
                x.convert_r(Round::NearestTiesToEven, &mut false),
            )
        } else {
            let x = Double::from_bits(bits);
            (
                x.is_denormal(),
                x.convert_r(Round::NearestTiesToEven, &mut false),
            )
        };
        if denormal {
            self.exception(2);
        }
        self.status(converted.status);
        converted.value
    }
    pub(crate) fn store_real_bits(&mut self, size: usize) -> u128 {
        let x = self.get(0);
        self.operands(&[x]);
        let (status, value, rounded_up) = match size {
            4 => {
                let r: StatusAnd<Single> = x.convert_r(self.round(), &mut false);
                let t: StatusAnd<Single> = x.convert_r(Round::TowardZero, &mut false);
                (r.status, r.value.to_bits(), r.value.abs() > t.value.abs())
            }
            8 => {
                let r: StatusAnd<Double> = x.convert_r(self.round(), &mut false);
                let t: StatusAnd<Double> = x.convert_r(Round::TowardZero, &mut false);
                (r.status, r.value.to_bits(), r.value.abs() > t.value.abs())
            }
            _ => (Status::OK, x.to_bits(), false),
        };
        self.status(status);
        if rounded_up {
            self.sw |= C1;
        }
        value
    }
    pub(crate) fn store_int_bits(&mut self, size: usize, truncate: bool) -> u128 {
        let x = self.get(0);
        self.operands(&[x]);
        let round = if truncate {
            Round::TowardZero
        } else {
            self.round()
        };
        let r = x.to_i128_r(size * 8, round, &mut false);
        self.status(r.status);
        let value = if r.status.contains(Status::INVALID_OP) {
            1u128 << (size * 8 - 1)
        } else {
            r.value as u128
        };
        if r.status.contains(Status::INEXACT) && Ext::from_i128(r.value).value.abs() > x.abs() {
            self.sw |= C1;
        }
        value
    }
    pub(crate) fn load_constant(&mut self, index: usize) {
        self.push(LOAD_CONSTANTS[((self.cw >> 10) & 3) as usize][index]);
    }
    pub(crate) fn accept(&mut self, r: StatusAnd<Ext>, truncated: Ext) -> Ext {
        self.status(r.status);
        if r.status.contains(Status::INEXACT) && r.value.abs() > truncated.abs() {
            self.sw |= C1;
        }
        r.value
    }
    fn reduced<T: Float + FloatConvert<Ext>>(r: StatusAnd<Ext>, round: Round) -> StatusAnd<Ext>
    where
        Ext: FloatConvert<T>,
    {
        // Preserve the direction of the discarded tail in the least significant
        // bit. The target has at least 11 fewer bits than the intermediate.
        let mut odd = r.value;
        if r.status.contains(Status::INEXACT) && odd.is_finite() {
            odd = Ext::from_bits(odd.to_bits() | 1);
        }
        let rounded: StatusAnd<T> = odd.convert_r(round, &mut false);
        let extended: StatusAnd<Ext> = rounded
            .value
            .convert_r(Round::NearestTiesToEven, &mut false);
        (r.status | rounded.status).and(extended.value)
    }
    pub(crate) fn binary(&mut self, a: Ext, b: Ext, kind: usize) -> Ext {
        self.operands(&[a, b]);
        let op = |round| match kind {
            0 => a.add_r(b, round),
            1 => a.mul_r(b, round),
            4 => a.sub_r(b, round),
            5 => b.sub_r(a, round),
            6 => a.div_r(b, round),
            7 => b.div_r(a, round),
            _ => unreachable!(),
        };
        let trunc = op(Round::TowardZero);
        let mut result = match (self.cw >> 8) & 3 {
            0 => Self::reduced::<IeeeFloat<P24>>(trunc, self.round()),
            2 => Self::reduced::<IeeeFloat<P53>>(trunc, self.round()),
            _ => op(self.round()),
        };
        // Exact cancellation has a rounding-dependent zero sign. Conversion
        // cannot recover that sign from a toward-zero intermediate.
        if trunc.value.is_zero() && trunc.status == Status::OK {
            result.value = op(self.round()).value;
        }
        if result.status.contains(Status::INVALID_OP) && !a.is_nan() && !b.is_nan() {
            result.value = Ext::from_bits(INDEFINITE);
        }
        self.accept(result, trunc.value)
    }
    pub(crate) fn compare(&mut self, a: Ext, b: Ext, quiet: bool) -> u16 {
        self.operands(&[a, b]);
        if a.is_signaling() || b.is_signaling() || (!quiet && (a.is_nan() || b.is_nan())) {
            self.exception(1);
        }
        match a.partial_cmp(&b) {
            None => C0 | C2 | C3,
            Some(Ordering::Less) => C0,
            Some(Ordering::Equal) => C3,
            _ => 0,
        }
    }
    pub(crate) fn compare_sw(&mut self, a: Ext, b: Ext, quiet: bool) {
        let flags = self.compare(a, b, quiet);
        self.sw = (self.sw & !(C0 | C2 | C3)) | flags;
    }
    pub(crate) fn sqrt(&mut self, x: Ext) -> Ext {
        self.operands(&[x]);
        if x.is_nan() {
            return self.accept(x.add_r(Ext::ZERO, self.round()), x);
        }
        if x.is_zero() {
            return x;
        }
        if x.is_negative() {
            self.exception(1);
            return Ext::from_bits(INDEFINITE);
        }
        if x.is_infinite() {
            return x;
        }
        let bits = x.to_bits();
        let mut sig = bits as u64;
        let mut exp = ((bits >> 64) & 0x7fff) as i32 - 16383;
        if exp == -16383 {
            exp += 1;
        }
        let shift = sig.leading_zeros();
        sig <<= shift;
        exp -= shift as i32;
        let radicand = (sig as u128) << (63 + (exp & 1));
        let root = radicand.isqrt();
        let remainder = radicand - root * root;
        let mut result = Ext::from_bits((((exp.div_euclid(2) + 16383) as u128) << 64) | root);
        let truncated = result;
        let status = if remainder == 0 {
            Status::OK
        } else {
            Status::INEXACT
        };
        if (self.cw >> 8) & 3 == 0 {
            result = Self::reduced::<IeeeFloat<P24>>(status.and(result), self.round()).value;
        } else if (self.cw >> 8) & 3 == 2 {
            result = Self::reduced::<IeeeFloat<P53>>(status.and(result), self.round()).value;
        } else if remainder != 0
            && (self.round() == Round::TowardPositive
                || (self.round() == Round::NearestTiesToEven && remainder > root))
        {
            result = result.next_up().value;
        }
        // Reduced precision can be inexact even for an exact 64-bit root.
        let status = if result != truncated {
            status | Status::INEXACT
        } else {
            status
        };
        self.accept(status.and(result), truncated)
    }
    fn to_quad(x: Ext) -> Quad {
        x.convert_r(Round::NearestTiesToEven, &mut false).value
    }
    fn narrow_quad(&mut self, x: Quad) -> Ext {
        let r: StatusAnd<Ext> = x.convert_r(self.round(), &mut false);
        self.status(r.status);
        r.value
    }
    fn ln2() -> Quad {
        *LN2
    }
    fn log2(x: Quad) -> Quad {
        let mut exponent = 0;
        let mut mantissa = x.frexp_r(&mut exponent, Round::NearestTiesToEven);
        // Keep the mantissa close to one. Normalizing exclusively to [1,2)
        // would cancel almost all significant bits for inputs just below one.
        if mantissa < Quad::from_bits(0x3ffe_8000_0000_0000_0000_0000_0000_0000) {
            mantissa = mantissa.scalbn(1);
            exponent -= 1;
        }
        let one = Quad::from_u128(1).value;
        let z = ((mantissa - one).value / (mantissa + one).value).value;
        let logarithm = Self::atanh_log2(z);
        (logarithm + Quad::from_i128(exponent as i128).value).value
    }
    fn atanh_log2(z: Quad) -> Quad {
        let z2 = (z * z).value;
        let mut power = z;
        let mut sum = z;
        for n in 1..128 {
            power = (power * z2).value;
            let next = (sum + quad_divide_small(power, 2 * n + 1)).value;
            if next == sum {
                break;
            }
            sum = next;
        }
        (sum.scalbn(1) / Self::ln2()).value
    }
    pub(crate) fn log_multiply(&mut self, plus_one: bool) {
        let x = self.get(0);
        let y = self.get(1);
        self.operands(&[x, y]);
        if x.is_nan() || y.is_nan() {
            let r = x.add_r(y, self.round());
            let value = self.accept(r, r.value);
            self.put(1, value);
            self.pop();
            return;
        }
        let mut q = Self::to_quad(x);
        if plus_one {
            q = (q + Quad::from_u128(1).value).value;
        }
        let result = if q.is_negative() && !q.is_zero() {
            self.exception(1);
            Ext::from_bits(INDEFINITE)
        } else if q.is_zero() {
            let r = y.mul_r(-Ext::INFINITY, self.round());
            self.exception(4);
            self.accept(r, r.value)
        } else if q.is_nan() || y.is_nan() || q.is_infinite() {
            let logarithm = if q.is_infinite() {
                Ext::INFINITY
            } else {
                Ext::NAN
            };
            let r = y.mul_r(logarithm, self.round());
            self.accept(r, r.value)
        } else {
            let logarithm = if plus_one && x.abs() <= Ext::from_bits(0x3ffe_8000_0000_0000_0000) {
                let x = Self::to_quad(x);
                let z = (x / (x + Quad::from_u128(2).value).value).value;
                Self::atanh_log2(z)
            } else {
                Self::log2(q)
            };
            let q = (logarithm * Self::to_quad(y)).value;
            self.narrow_quad(q)
        };
        self.put(1, result);
        self.pop();
    }
    pub(crate) fn exp2_minus_one(&mut self) {
        let x = self.get(0);
        self.operands(&[x]);
        if x.is_nan() {
            let r = x.add_r(Ext::ZERO, self.round());
            let v = self.accept(r, r.value);
            self.put(0, v);
            return;
        }
        let one = Ext::from_u128(1).value;
        if x.abs() > one {
            return;
        } // outside the ISA-defined domain
        if x.is_zero() {
            return;
        }
        if x == one {
            self.put(0, one);
            return;
        }
        if x == -one {
            self.put(0, Ext::from_bits(0xbffe_8000_0000_0000_0000));
            return;
        }
        let z = (Self::to_quad(x) * Self::ln2()).value;
        let mut term = z;
        let mut sum = z;
        for n in 2..128 {
            term = quad_divide_small((term * z).value, n);
            let next = (sum + term).value;
            if next == sum {
                break;
            }
            sum = next;
        }
        let v = self.narrow_quad(sum);
        self.exception(0x20);
        self.put(0, v);
    }
    pub(crate) fn trig(&mut self, op: u8) {
        let x = self.get(0);
        self.operands(&[x]);
        // Invalid/denormal operand exceptions precede any stack mutation.
        if self.sw & !self.cw & 3 != 0 {
            return;
        }
        let limit = Ext::from_bits(((16383u128 + 63) << 64) | (1 << 63));
        if x.is_finite() && x.abs() >= limit {
            self.sw |= C2;
            return;
        }
        self.sw &= !C2;
        if op == 0xfb && self.tag((self.top() + 7) & 7) != 3 {
            self.sw |= C1;
            self.exception(0x41);
            if self.cw & 1 != 0 {
                self.put(0, Ext::from_bits(INDEFINITE));
                self.push(Ext::from_bits(INDEFINITE));
            }
            return;
        }
        let (sin, cos) = if x.is_nan() {
            let r = x.add_r(Ext::ZERO, self.round());
            let v = self.accept(r, x);
            (v, v)
        } else if x.is_infinite() {
            self.exception(1);
            let v = Ext::from_bits(INDEFINITE);
            (v, v)
        } else if x.is_zero() {
            (x, Ext::from_u128(1).value)
        } else if x.abs() < Ext::from_bits(((16383u128 - 33) << 64) | (1 << 63)) {
            // sin(x) lies immediately toward zero from x; cos(x) immediately
            // below one. Preserve the discarded tail even when binary128
            // cannot represent x^3 or distinguish 1-x^2/2 from one.
            let toward_zero = matches!(self.round(), Round::TowardZero)
                || (x.is_negative() && self.round() == Round::TowardPositive)
                || (!x.is_negative() && self.round() == Round::TowardNegative);
            let sin = if toward_zero {
                if x.is_negative() {
                    x.next_up().value
                } else {
                    x.next_down().value
                }
            } else {
                x
            };
            let one = Ext::from_u128(1).value;
            let cos = if matches!(self.round(), Round::TowardZero | Round::TowardNegative) {
                one.next_down().value
            } else {
                one
            };
            self.exception(0x20);
            if op != 0xff && (sin.is_denormal() || sin.is_zero()) {
                self.exception(0x10);
            }
            if (op == 0xfe && !toward_zero) || (op != 0xfe && cos == one) {
                self.sw |= C1;
            }
            (sin, cos)
        } else {
            let high = Quad::from_bits(HALF_PI_HI);
            let q = Self::to_quad(x);
            let quadrant = (q / high)
                .value
                .round_to_integral(Round::NearestTiesToEven)
                .value;
            let n = quadrant.to_i128_r(64, Round::TowardZero, &mut false).value;
            let r = (-quadrant)
                .mul_add_r(high, q, Round::NearestTiesToEven)
                .value;
            let r = (-quadrant)
                .mul_add_r(*HALF_PI_LO, r, Round::NearestTiesToEven)
                .value;
            let r2 = -(r * r).value;
            let mut st = r;
            let mut ct = Quad::from_u128(1).value;
            let mut sin = st;
            let mut cos = ct;
            for i in 1..64 {
                st = quad_divide_small(quad_divide_small((st * r2).value, 2 * i), 2 * i + 1);
                ct = quad_divide_small(quad_divide_small((ct * r2).value, 2 * i - 1), 2 * i);
                let s = (sin + st).value;
                let c = (cos + ct).value;
                if s == sin && c == cos {
                    break;
                }
                sin = s;
                cos = c;
            }
            let (s, c) = match n.rem_euclid(4) {
                0 => (sin, cos),
                1 => (cos, -sin),
                2 => (-sin, -cos),
                _ => (-cos, sin),
            };
            let mut narrow = |q: Quad| {
                let r: StatusAnd<Ext> = q.convert_r(self.round(), &mut false);
                let t: StatusAnd<Ext> = q.convert_r(Round::TowardZero, &mut false);
                self.sw &= !C1;
                self.accept(r, t.value)
            };
            let s = if op != 0xff { narrow(s) } else { Ext::ZERO };
            let c = if op != 0xfe { narrow(c) } else { Ext::ZERO };
            self.exception(0x20);
            (s, c)
        };
        if self.sw & !self.cw & 1 != 0 {
            return;
        }
        match op {
            0xfb => {
                self.put(0, sin);
                let rounded_up = self.sw & C1;
                self.push(cos);
                self.sw |= rounded_up;
            }
            0xfe => self.put(0, sin),
            0xff => self.put(0, cos),
            _ => unreachable!(),
        }
    }
    pub(crate) fn crt_ftol(&mut self) -> u64 {
        let x = self.get(0);
        self.operands(&[x]);
        let r = x.to_i128_r(64, Round::TowardZero, &mut false);
        self.status(r.status);
        let value = if r.status.contains(Status::INVALID_OP) {
            1u64 << 63
        } else {
            r.value as u64
        };
        self.pop();
        value
    }
    pub(crate) fn crt_floor(&mut self, bits: u64) {
        let x = Double::from_bits(bits as u128);
        let result = x.round_to_integral(Round::TowardNegative);
        self.status(result.status);
        let value: StatusAnd<Ext> = result.value.convert_r(Round::NearestTiesToEven, &mut false);
        self.push(value.value);
    }
    pub(crate) fn crt_ldexp(&mut self, bits: u64, exponent: i32) -> bool {
        let x = Double::from_bits(bits as u128);
        let q: StatusAnd<Quad> = x.convert_r(Round::NearestTiesToEven, &mut false);
        // Clamping extreme exponents preserves every binary64 result and its
        // directed overflow/underflow while staying inside binary128 range.
        let r: StatusAnd<Double> = q
            .value
            .scalbn(exponent.clamp(-4096, 4096))
            .convert_r(self.round(), &mut false);
        self.status(r.status);
        let value: StatusAnd<Ext> = r.value.convert_r(Round::NearestTiesToEven, &mut false);
        self.push(value.value);
        r.status.intersects(Status::OVERFLOW | Status::UNDERFLOW)
    }
    pub(crate) fn crt_pow(&mut self, arguments: Option<(u64, u64)>) -> Option<u32> {
        let (x, y) = if let Some((x, y)) = arguments {
            let cv = |bits: u64| {
                let v: StatusAnd<Ext> =
                    Double::from_bits(bits as u128).convert_r(Round::NearestTiesToEven, &mut false);
                v.value
            };
            (cv(x), cv(y))
        } else {
            let y = self.get(0);
            let x = self.get(1);
            self.pop();
            self.pop();
            (x, y)
        };
        self.operands(&[x, y]);
        let one = Ext::from_u128(1).value;
        let integral = y.is_finite() && y.round_to_integral(Round::TowardZero).value == y;
        let odd = integral
            && y.abs() < Ext::from_bits(((16383u128 + 64) << 64) | (1 << 63))
            && y.to_i128_r(128, Round::TowardZero, &mut false).value & 1 != 0;
        let negative = x.is_negative() && odd;
        let mut error = None;
        let mut result = if y.is_zero() || x == one {
            one
        } else if x.is_nan() || y.is_nan() {
            self.accept(x.add_r(y, self.round()), x)
        } else if y.is_infinite() {
            if x.abs() == one {
                one
            } else if (x.abs() > one) != y.is_negative() {
                Ext::INFINITY
            } else {
                Ext::ZERO
            }
        } else if x.is_infinite() {
            if y.is_negative() {
                Ext::ZERO
            } else {
                Ext::INFINITY
            }
        } else if x.is_zero() {
            if y.is_negative() {
                self.exception(4);
                error = Some(34);
                Ext::INFINITY
            } else {
                Ext::ZERO
            }
        } else if x.is_negative() && !integral {
            self.exception(1);
            error = Some(33);
            Ext::from_bits(INDEFINITE)
        } else {
            let q = (Self::log2(Self::to_quad(x.abs())) * Self::to_quad(y)).value;
            let q = if q > Quad::from_i128(4096).value {
                Quad::from_i128(4096).value
            } else if q < Quad::from_i128(-4096).value {
                Quad::from_i128(-4096).value
            } else {
                q
            };
            {
                let int = q.round_to_integral(Round::TowardNegative).value;
                let n = int.to_i128_r(32, Round::TowardZero, &mut false).value as i32;
                let frac = (q - int).value;
                let z = (frac * Self::ln2()).value;
                let mut sum = Quad::from_u128(1).value;
                let mut term = sum;
                for i in 1..128 {
                    term = quad_divide_small((term * z).value, i);
                    let next = (sum + term).value;
                    if next == sum {
                        break;
                    }
                    sum = next;
                }
                let value = if negative {
                    -sum.scalbn(n)
                } else {
                    sum.scalbn(n)
                };
                // CRT pow returns binary64. Its internal reduction remains
                // binary128, and the C intrinsic inputs retain x87 precision.
                let rounded: StatusAnd<Double> = value.convert_r(self.round(), &mut false);
                self.status(rounded.status);
                if !frac.is_zero() {
                    self.exception(0x20);
                }
                if rounded
                    .status
                    .intersects(Status::OVERFLOW | Status::UNDERFLOW)
                {
                    error = Some(34);
                }
                let extended: StatusAnd<Ext> = rounded
                    .value
                    .convert_r(Round::NearestTiesToEven, &mut false);
                extended.value
            }
        };
        if negative && !result.is_negative() && !result.is_nan() {
            result = -result;
        }
        self.push(result);
        error
    }
}

impl Cpu {
    pub(super) fn fpu_wait(&self, eip: u32) -> Result<(), CpuError> {
        if self.fpu.sw & !self.fpu.cw & 0x3f != 0 {
            Err(CpuError::FloatingPoint {
                eip,
                status: self.fpu.sw,
            })
        } else {
            Ok(())
        }
    }
    fn fpu_compare(&mut self, a: Ext, b: Ext, quiet: bool, flags: bool) {
        if flags {
            let sw = self.fpu.compare(a, b, quiet);
            self.eflags &= !(OF | SF | AF | ZF | PF | CF);
            if sw & C0 != 0 {
                self.eflags |= CF;
            }
            if sw & C2 != 0 {
                self.eflags |= PF;
            }
            if sw & C3 != 0 {
                self.eflags |= ZF;
            }
        } else {
            self.fpu.compare_sw(a, b, quiet);
        }
    }
    fn fpu_load_real(&mut self, mem: &GuestMem, a: u32, size: u32) -> Result<Ext, CpuError> {
        let mut bytes = [0; 16];
        mem.read_bytes(a, &mut bytes[..size as usize])?;
        Ok(self
            .fpu
            .load_real_bits(u128::from_le_bytes(bytes), size as usize))
    }
    fn fpu_store_real(&mut self, mem: &GuestMem, a: u32, size: u32) -> Result<(), CpuError> {
        let value = self.fpu.store_real_bits(size as usize);
        mem.write_bytes(a, &value.to_le_bytes()[..size as usize])?;
        Ok(())
    }
    fn fpu_store_int(
        &mut self,
        mem: &GuestMem,
        a: u32,
        size: u32,
        truncate: bool,
    ) -> Result<(), CpuError> {
        let value = self.fpu.store_int_bits(size as usize, truncate);
        mem.write_bytes(a, &value.to_le_bytes()[..size as usize])?;
        Ok(())
    }
    pub(super) fn fpu_op(
        &mut self,
        mem: &mut GuestMem,
        seg: u32,
        bits: u8,
        op: u8,
        eip0: u32,
    ) -> Result<(), CpuError> {
        let b = mem.read_u8(self.eip)?;
        let nonwaiting = matches!((op, b), (0xdb, 0xe2 | 0xe3) | (0xdf, 0xe0))
            || (b < 0xc0 && matches!((op, (b >> 3) & 7), (0xd9, 6 | 7) | (0xdd, 6 | 7)));
        if !nonwaiting {
            self.fpu_wait(eip0)?;
        }
        let environment =
            b < 0xc0 && matches!((op, (b >> 3) & 7), (0xd9, 4..=7) | (0xdd, 4 | 6 | 7));
        if !nonwaiting && !environment {
            self.fpu.ip = eip0 as u64;
            self.fpu.opcode = ((op as u16 & 7) << 8) | b as u16;
        }
        if b >= 0xc0 {
            self.eip = self.eip.wrapping_add(1);
            let i = (b & 7) as usize;
            let group = ((b >> 3) & 7) as usize;
            match (op, b) {
                (0xd8, _) | (0xdc, _) | (0xde, 0xc0..=0xcf | 0xe0..=0xff) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(i);
                    if group == 2 || group == 3 {
                        self.fpu_compare(a, v, false, false);
                        if group == 3 {
                            self.fpu.pop();
                        }
                    } else {
                        let (left, right, kind, dest) = if op == 0xd8 {
                            (a, v, group, 0)
                        } else {
                            (v, a, if group >= 4 { group ^ 1 } else { group }, i)
                        };
                        let r = self.fpu.binary(left, right, kind);
                        self.fpu.put(dest, r);
                        if op == 0xde {
                            self.fpu.pop();
                        }
                    }
                }
                (0xd9, 0xc0..=0xc7) => {
                    let x = self.fpu.get(i);
                    self.fpu.push(x);
                }
                (0xd9, 0xc8..=0xcf) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(i);
                    self.fpu.put(0, v);
                    self.fpu.put(i, a);
                    self.fpu.sw &= !C1;
                }
                (0xd9, 0xd0) => {}
                (0xd9, 0xe0 | 0xe1) => {
                    let x = self.fpu.get(0);
                    self.fpu.put(0, if b == 0xe0 { -x } else { x.abs() });
                    self.fpu.sw &= !C1;
                }
                (0xd9, 0xe4) => {
                    let x = self.fpu.get(0);
                    self.fpu_compare(x, Ext::ZERO, false, false);
                }
                (0xd9, 0xe5) => {
                    let n = self.fpu.index(0);
                    let x = Ext::from_bits(self.fpu.regs[n]);
                    let code = if self.fpu.tag(n) == 3 {
                        C0 | C3
                    } else if x.is_nan() {
                        C0
                    } else if x.is_infinite() {
                        C0 | C2
                    } else if x.is_zero() {
                        C3
                    } else if x.is_denormal() {
                        C2 | C3
                    } else {
                        C2
                    };
                    self.fpu.sw = (self.fpu.sw & !(C0 | C1 | C2 | C3))
                        | code
                        | if x.is_negative() { C1 } else { 0 };
                }
                (0xd9, 0xe8..=0xee) => {
                    self.fpu.load_constant((b - 0xe8) as usize);
                }
                (0xd9, 0xf0) => self.fpu.exp2_minus_one(),
                (0xd9, 0xf1 | 0xf9) => self.fpu.log_multiply(b == 0xf9),
                (0xd9, 0xf6 | 0xf7) => {
                    let top = self.fpu.top();
                    self.fpu.set_top(if b == 0xf6 { top + 7 } else { top + 1 });
                    self.fpu.sw &= !C1;
                }
                (0xd9, 0xfa) => {
                    let x = self.fpu.get(0);
                    let r = self.fpu.sqrt(x);
                    self.fpu.put(0, r);
                }
                (0xd9, 0xfb | 0xfe | 0xff) => self.fpu.trig(b),
                (0xd9, 0xfc) => {
                    let x = self.fpu.get(0);
                    self.fpu.operands(&[x]);
                    let r = x.round_to_integral(self.fpu.round());
                    let t = x.round_to_integral(Round::TowardZero).value;
                    let r = self.fpu.accept(r, t);
                    self.fpu.put(0, r);
                }
                (0xd9, 0xfd) => {
                    let x = self.fpu.get(0);
                    let y = self.fpu.get(1);
                    self.fpu.operands(&[x, y]);
                    let n = y
                        .to_i128_r(32, Round::TowardZero, &mut false)
                        .value
                        .clamp(-32768, 32768) as i32;
                    let r = x.scalbn_r(n, self.fpu.round());
                    if r.is_infinite() && x.is_finite() {
                        self.fpu.exception(0x28);
                    }
                    if (r.is_denormal() || r.is_zero()) && !x.is_zero() {
                        self.fpu.exception(0x30);
                    }
                    self.fpu.put(0, r);
                }
                (0xda, 0xe9) | (0xde, 0xd9) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(1);
                    self.fpu_compare(a, v, op == 0xda, false);
                    self.fpu.pop();
                    self.fpu.pop();
                }
                (0xda | 0xdb, 0xc0..=0xdf) => {
                    let condition = match group {
                        0 => self.flag(CF),
                        1 => self.flag(ZF),
                        2 => self.flag(CF) || self.flag(ZF),
                        _ => self.flag(PF),
                    };
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(i);
                    if condition ^ (op == 0xdb) {
                        self.fpu.put(0, v);
                    } else {
                        self.fpu.put(0, a);
                    }
                }
                (0xdb, 0xe2) => self.fpu.sw &= !0x80ff,
                (0xdb, 0xe3) => self.fpu = Fpu::default(),
                (0xdb | 0xdf, 0xe8..=0xf7) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(i);
                    self.fpu_compare(a, v, b < 0xf0, true);
                    if op == 0xdf {
                        self.fpu.pop();
                    }
                }
                (0xdd, 0xc0..=0xc7) | (0xdf, 0xc0..=0xc7) => {
                    self.fpu.set_tag(self.fpu.index(i), 3);
                    if op == 0xdf {
                        self.fpu.pop();
                    }
                }
                (0xdd, 0xd0..=0xdf) => {
                    let x = self.fpu.get(0);
                    self.fpu.put(i, x);
                    self.fpu.sw &= !C1;
                    if group == 3 {
                        self.fpu.pop();
                    }
                }
                (0xdd, 0xe0..=0xef) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(i);
                    self.fpu_compare(a, v, true, false);
                    if group == 5 {
                        self.fpu.pop();
                    }
                }
                (0xdf, 0xe0) => self.regs[EAX] = (self.regs[EAX] & !0xffff) | self.fpu.sw as u32,
                _ => {
                    return Err(self.unsupported(
                        mem,
                        eip0,
                        &format!("x87 reg op {op:02x} {b:02x}"),
                    ))
                }
            }
        } else {
            let rm = self.modrm(mem)?;
            let a = rm.addr.unwrap().wrapping_add(seg);
            let g = rm.reg;
            if !matches!((op, g), (0xd9, 4..=7) | (0xdd, 4 | 6 | 7)) {
                self.fpu.dp = a as u64;
            }
            match (op, g) {
                (0xd8 | 0xdc | 0xda | 0xde, _) => {
                    let v = match op {
                        0xd8 => self.fpu_load_real(mem, a, 4)?,
                        0xdc => self.fpu_load_real(mem, a, 8)?,
                        0xda => Ext::from_i128(mem.read_u32(a)? as i32 as i128).value,
                        _ => Ext::from_i128(mem.read_u16(a)? as i16 as i128).value,
                    };
                    let x = self.fpu.get(0);
                    if g == 2 || g == 3 {
                        self.fpu_compare(x, v, false, false);
                        if g == 3 {
                            self.fpu.pop();
                        }
                    } else {
                        let r = self.fpu.binary(x, v, g);
                        self.fpu.put(0, r);
                    }
                }
                (0xd9 | 0xdd, 0) | (0xdb, 5) => {
                    let size = match op {
                        0xd9 => 4,
                        0xdd => 8,
                        _ => 10,
                    };
                    let x = self.fpu_load_real(mem, a, size)?;
                    self.fpu.push(x);
                }
                (0xd9 | 0xdd, 2 | 3) | (0xdb, 7) => {
                    let size = match op {
                        0xd9 => 4,
                        0xdd => 8,
                        _ => 10,
                    };
                    self.fpu_store_real(mem, a, size)?;
                    if g == 3 || g == 7 {
                        self.fpu.pop();
                    }
                }
                (0xdb | 0xdf, 0) | (0xdf, 5) => {
                    let x = match (op, g) {
                        (0xdb, _) => mem.read_u32(a)? as i32 as i128,
                        (0xdf, 5) => mem.read_u64(a)? as i64 as i128,
                        _ => mem.read_u16(a)? as i16 as i128,
                    };
                    self.fpu.push(Ext::from_i128(x).value);
                }
                (0xdb | 0xdf, 1..=3) | (0xdd, 1) | (0xdf, 7) => {
                    let size = match (op, g) {
                        (0xdd, _) | (0xdf, 7) => 8,
                        (0xdb, _) => 4,
                        _ => 2,
                    };
                    self.fpu_store_int(mem, a, size, g == 1)?;
                    if g != 2 {
                        self.fpu.pop();
                    }
                }
                (0xd9, 5) => {
                    self.fpu.cw = mem.read_u16(a)?;
                    if self.fpu.sw & !self.fpu.cw & 0x3f != 0 {
                        self.fpu.sw |= 0x8080;
                    }
                }
                (0xd9, 7) => mem.write_u16(a, self.fpu.cw)?,
                (0xdd, 7) => mem.write_u16(a, self.fpu.sw)?,
                (0xd9, 4 | 6) | (0xdd, 4 | 6) => {
                    self.fpu_environment(mem, a, bits, g == 4)?;
                    let size = if bits == 16 { 14 } else { 28 };
                    if op == 0xdd {
                        for i in 0..8 {
                            let addr = a + size + i as u32 * 10;
                            if g == 4 {
                                let mut bytes = [0; 16];
                                mem.read_bytes(addr, &mut bytes[..10])?;
                                let n = self.fpu.index(i);
                                self.fpu.regs[n] = u128::from_le_bytes(bytes);
                            } else {
                                mem.write_bytes(addr, &self.fpu.st_bits(i).to_le_bytes()[..10])?;
                            }
                        }
                        if g == 6 {
                            self.fpu = Fpu::default();
                        }
                    } else if g == 6 {
                        self.fpu.cw |= 0x3f;
                    }
                }
                _ => return Err(self.unsupported(mem, eip0, &format!("x87 mem op {op:02x} /{g}"))),
            }
        }
        Ok(())
    }
    fn fpu_environment(
        &mut self,
        mem: &GuestMem,
        a: u32,
        bits: u8,
        load: bool,
    ) -> Result<(), CpuError> {
        if bits == 16 {
            if load {
                self.fpu.cw = mem.read_u16(a)?;
                self.fpu.sw = mem.read_u16(a + 2)?;
                self.fpu.tags = mem.read_u16(a + 4)?;
                self.fpu.ip = mem.read_u16(a + 6)? as u64;
                self.fpu.dp = mem.read_u16(a + 10)? as u64;
            } else {
                for (off, value) in [
                    (0, self.fpu.cw),
                    (2, self.fpu.sw),
                    (4, self.fpu.tags),
                    (6, self.fpu.ip as u16),
                    (8, 0),
                    (10, self.fpu.dp as u16),
                    (12, 0),
                ] {
                    mem.write_u16(a + off, value)?;
                }
            }
        } else if load {
            self.fpu.cw = mem.read_u16(a)?;
            self.fpu.sw = mem.read_u16(a + 4)?;
            self.fpu.tags = mem.read_u16(a + 8)?;
            self.fpu.ip = mem.read_u32(a + 12)? as u64;
            self.fpu.opcode = mem.read_u16(a + 18)?;
            self.fpu.dp = mem.read_u32(a + 20)? as u64;
        } else {
            mem.write_u32(a, self.fpu.cw as u32 | 0xffff0000)?;
            mem.write_u32(a + 4, self.fpu.sw as u32 | 0xffff0000)?;
            mem.write_u32(a + 8, self.fpu.tags as u32 | 0xffff0000)?;
            mem.write_u32(a + 12, self.fpu.ip as u32)?;
            mem.write_u32(a + 16, (self.fpu.opcode as u32) << 16)?;
            mem.write_u32(a + 20, self.fpu.dp as u32)?;
            mem.write_u32(a + 24, 0xffff0000)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reciprocal_digits_match_integer_quotients_and_remainders() {
        for n in 2..=255u64 {
            for quotient in [0, 1, (1 << 32) / n, (1 << 40) / n, ((1 << 57) - 1) / n - 1] {
                for remainder in 0..n {
                    check(quotient * n + remainder, n);
                }
            }
        }
        let mut seed = 0x9137_eada_3745_729bu64;
        for _ in 0..4096 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            check(seed & ((1 << 57) - 1), (seed % 254) + 2);
        }
        fn check(value: u64, n: u64) {
            assert_eq!(
                divide_small_digit(value, n, SMALL_RECIPROCALS[n as usize]),
                (value / n, value % n),
                "{value} / {n}"
            );
        }
    }

    #[test]
    fn small_integer_quad_division_matches_apfloat_bits() {
        let mut seed = 0x3ac5_79df_1234_5678_9abc_def0_2468_1357u128;
        for exponent in [0, 1, 2, 7, 8, 9, 0x3fff, 0x7ffe, 0x7fff] {
            for fraction in [0, 1, 1 << 111, (1 << 112) - 1] {
                for sign in [0, 1 << 127] {
                    let bits = sign | (exponent << 112) | fraction;
                    check(bits, 1..=255);
                }
            }
        }
        for _ in 0..4096 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            check(seed, std::iter::once((seed as u32 % 255) + 1));
        }
        // Adjacent near-one significands exercise normalization and rounding.
        for fraction in 0..16 {
            check((0x3fffu128 << 112) | fraction, 1..=255);
        }
        fn check(bits: u128, divisors: impl IntoIterator<Item = u32>) {
            let x = Quad::from_bits(bits);
            for n in divisors {
                let expected = (x / Quad::from_u128(n as u128).value).value.to_bits();
                assert_eq!(
                    quad_divide_small(x, n).to_bits(),
                    expected,
                    "{bits:032x} / {n}"
                );
            }
        }
    }
}
