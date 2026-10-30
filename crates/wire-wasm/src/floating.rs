//! Value-only SSE numerical transactions. Inputs include MXCSR explicitly;
//! outputs do not commit CPU registers, instruction pointers, or counters.
use crate::simd::{Scalar, SimdState};
use rustc_apfloat::{Round, Status, StatusAnd};

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Output {
    pub value: [u64; 2],
    pub mxcsr: u32,
    pub exception: u32,
    pub flags: u64,
}

fn vector(words: [u64; 2]) -> u128 {
    u128::from(words[0]) | u128::from(words[1]) << 64
}
fn lane(value: u128, index: usize, width: usize) -> u64 {
    (value >> (index * width * 8)) as u64 & (u64::MAX >> (64 - width * 8))
}
fn replace(value: &mut u128, index: usize, width: usize, word: u64) {
    let mask = u128::MAX >> (128 - width * 8);
    let shift = index * width * 8;
    *value = (*value & !(mask << shift)) | ((u128::from(word) & mask) << shift);
}
fn input(mxcsr: u32) -> SimdState {
    // Track fresh pre-computation flags while retaining sticky precision for
    // the normal native arithmetic path.
    SimdState {
        mxcsr: mxcsr & !7,
        rflags: 0,
    }
}
fn output(value: u128, mut state: SimdState, exception: bool, mxcsr: u32) -> Output {
    if state.mxcsr & !(mxcsr >> 7) & 7 != 0 {
        // SDM 11.5.3: all packed pre-computation conditions precede every
        // post-computation condition. Existing sticky bits remain unchanged.
        state.mxcsr = mxcsr | (state.mxcsr & 7);
    } else {
        state.mxcsr |= mxcsr & 7;
    }
    Output {
        value: [value as u64, (value >> 64) as u64],
        mxcsr: state.mxcsr,
        exception: u32::from(exception),
        flags: state.rflags,
    }
}

pub fn map<const WIDTH: usize>(
    mxcsr: u32,
    a: [u64; 2],
    b: [u64; 2],
    count: u32,
    mut operation: impl FnMut(&mut SimdState, u64, u64, &mut bool) -> Result<u64, String>,
) -> Output {
    let mut state = input(mxcsr);
    let a = vector(a);
    let b = vector(b);
    let mut value = a;
    let mut exception = count == 0 || count as usize > 16 / WIDTH;
    for index in 0..(count as usize).min(16 / WIDTH) {
        match operation(
            &mut state,
            lane(a, index, WIDTH),
            lane(b, index, WIDTH),
            &mut exception,
        ) {
            Ok(word) => replace(&mut value, index, WIDTH, word),
            Err(_) => exception = true,
        }
    }
    output(value, state, exception, mxcsr)
}

pub fn horizontal<const WIDTH: usize>(
    mxcsr: u32,
    a: [u64; 2],
    b: [u64; 2],
    operation: impl FnMut(&mut SimdState, u64, u64, &mut bool) -> Result<u64, String>,
) -> Output {
    let inputs = [vector(a), vector(b)];
    let mut left = 0;
    let mut right = 0;
    for index in 0..16 / WIDTH {
        let source = inputs[index / (8 / WIDTH)];
        let at = index % (8 / WIDTH) * 2;
        replace(&mut left, index, WIDTH, lane(source, at, WIDTH));
        replace(&mut right, index, WIDTH, lane(source, at + 1, WIDTH));
    }
    map::<WIDTH>(
        mxcsr,
        [left as u64, (left >> 64) as u64],
        [right as u64, (right >> 64) as u64],
        (16 / WIDTH) as u32,
        operation,
    )
}

fn integral<F: Scalar, const WIDTH: usize>(value: F, mode: u32) -> StatusAnd<F> {
    #[cfg(target_arch = "wasm32")]
    if !value.is_nan() {
        // Wasm fixes IEEE rounding and subnormal behavior. NaNs retain the
        // precise signaling/payload path; finite inputs use native operations.
        let bits = value.to_bits() as u64;
        let result = if WIDTH == 4 {
            let value = f32::from_bits(bits as u32);
            u64::from(
                match mode {
                    0 => value.round_ties_even(),
                    1 => value.floor(),
                    2 => value.ceil(),
                    _ => value.trunc(),
                }
                .to_bits(),
            )
        } else {
            let value = f64::from_bits(bits);
            match mode {
                0 => value.round_ties_even(),
                1 => value.floor(),
                2 => value.ceil(),
                _ => value.trunc(),
            }
            .to_bits()
        };
        return (if result == bits {
            Status::OK
        } else {
            Status::INEXACT
        })
        .and(F::from_bits(u128::from(result)));
    }
    value.round_to_integral(match mode {
        0 => Round::NearestTiesToEven,
        1 => Round::TowardNegative,
        2 => Round::TowardPositive,
        _ => Round::TowardZero,
    })
}

pub fn round<F: Scalar, const WIDTH: usize>(
    mxcsr: u32,
    a: [u64; 2],
    b: [u64; 2],
    count: u32,
    control: u32,
) -> Output {
    map::<WIDTH>(mxcsr, a, b, count, |state, _, bits, exception| {
        let mut value = F::from_bits(u128::from(bits));
        // ROUND does not report denormal operands. DAZ still applies.
        if value.is_denormal() && mxcsr & 64 != 0 {
            value = F::ZERO.copy_sign(value);
        }
        let mode = if control & 4 != 0 {
            (mxcsr >> 13) & 3
        } else {
            control & 3
        };
        let mut result = integral::<F, WIDTH>(value, mode);
        if control & 8 != 0 {
            result.status &= !Status::INEXACT;
        }
        state
            .simd_accept::<F>(result, 0, exception)
            .map(|value| value.to_bits() as u64)
    })
}

fn dot_add<F: Scalar, const WIDTH: usize>(mxcsr: u32, a: u64, b: u64) -> Output {
    map::<WIDTH>(mxcsr, [a, 0], [b, 0], 1, |state, a, b, exception| {
        state.simd_arithmetic::<F>(
            if WIDTH == 4 {
                iced_x86::Mnemonic::Addss
            } else {
                iced_x86::Mnemonic::Addsd
            },
            a,
            b,
            exception,
        )
    })
}

pub fn dot<F: Scalar, const WIDTH: usize>(
    mxcsr: u32,
    a: [u64; 2],
    b: [u64; 2],
    control: u32,
) -> Output {
    let mut index = 0;
    let products = map::<WIDTH>(
        mxcsr,
        a,
        b,
        (16 / WIDTH) as u32,
        |state, a, b, exception| {
            let enabled = control & (1 << (index + 4)) != 0;
            index += 1;
            if enabled {
                state.simd_arithmetic::<F>(
                    if WIDTH == 4 {
                        iced_x86::Mnemonic::Mulss
                    } else {
                        iced_x86::Mnemonic::Mulsd
                    },
                    a,
                    b,
                    exception,
                )
            } else {
                Ok(0)
            }
        },
    );
    if products.exception != 0 {
        return products;
    }
    let values = vector(products.value);
    let mut sum = dot_add::<F, WIDTH>(
        products.mxcsr,
        lane(values, 0, WIDTH),
        lane(values, 1, WIDTH),
    );
    if sum.exception != 0 {
        return sum;
    }
    if WIDTH == 4 {
        // Each addition is a separate exception boundary. The second pair
        // must not affect MXCSR when the first pair has already faulted.
        let high = dot_add::<F, WIDTH>(sum.mxcsr, lane(values, 2, WIDTH), lane(values, 3, WIDTH));
        if high.exception != 0 {
            return high;
        }
        sum = dot_add::<F, WIDTH>(high.mxcsr, sum.value[0], high.value[0]);
        if sum.exception != 0 {
            return sum;
        }
    }
    let mut value = 0;
    for index in 0..16 / WIDTH {
        if control & (1 << index) != 0 {
            replace(&mut value, index, WIDTH, sum.value[0]);
        }
    }
    sum.value = [value as u64, (value >> 64) as u64];
    sum
}

pub fn convert<
    F: Scalar + crate::simd::ConvertTo<D>,
    D: Scalar,
    const SOURCE: usize,
    const DEST: usize,
>(
    mxcsr: u32,
    a: [u64; 2],
    b: [u64; 2],
    count: u32,
) -> Output {
    let mut state = input(mxcsr);
    let mut value = if count == 1 { vector(a) } else { 0 };
    let b = vector(b);
    let mut exception = count == 0 || count as usize > (16 / SOURCE).min(16 / DEST);
    for index in 0..(count as usize).min((16 / SOURCE).min(16 / DEST)) {
        match state.simd_convert::<F, D>(lane(b, index, SOURCE), &mut exception) {
            Ok(word) => replace(&mut value, index, DEST, word),
            Err(_) => exception = true,
        }
    }
    output(value, state, exception, mxcsr)
}

pub fn integer_to_float<F: Scalar, const DEST: usize>(
    mxcsr: u32,
    a: [u64; 2],
    b: [u64; 2],
    count: u32,
    scalar: bool,
) -> Output {
    let mut state = input(mxcsr);
    let mut value = if scalar { vector(a) } else { 0 };
    let b = vector(b);
    let limit = if scalar { 1 } else { 16 / DEST };
    let mut exception = count == 0 || count as usize > limit;
    for index in 0..(count as usize).min(limit) {
        let integer = if scalar {
            b as i64 as i128
        } else {
            lane(b, index, 4) as i32 as i128
        };
        let result = F::from_i128_r(integer, state.simd_round());
        match state.simd_accept::<F>(result, 0, &mut exception) {
            Ok(word) => replace(&mut value, index, DEST, word.to_bits() as u64),
            Err(_) => exception = true,
        }
    }
    output(value, state, exception, mxcsr)
}

pub fn float_to_integer<F: Scalar, const SOURCE: usize, const TRUNCATE: bool>(
    mxcsr: u32,
    b: [u64; 2],
    count: u32,
    bits: u32,
    scalar: bool,
) -> Output {
    let mut state = input(mxcsr);
    let mut value = 0;
    let b = vector(b);
    let limit = if scalar { 1 } else { (16 / SOURCE).min(4) };
    let mut exception =
        count == 0 || count as usize > limit || !matches!(bits, 32 | 64) || !scalar && bits != 32;
    if matches!(bits, 32 | 64) {
        let round = if TRUNCATE {
            Round::TowardZero
        } else {
            state.simd_round()
        };
        for index in 0..(count as usize).min(limit) {
            match state.simd_to_integer::<F>(lane(b, index, SOURCE), bits, round, &mut exception) {
                Ok(word) => replace(
                    &mut value,
                    index,
                    if scalar { bits as usize / 8 } else { 4 },
                    word,
                ),
                Err(_) => exception = true,
            }
        }
    }
    output(value, state, exception, mxcsr)
}
