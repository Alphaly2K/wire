//! Normal finite single-precision arithmetic, with exact inexact detection.
use crate::SimdState;
use iced_x86::Mnemonic as M;

#[inline(always)]
pub(crate) fn single(
    state: &mut SimdState,
    op: M,
    a: u64,
    b: u64,
    exception: &mut bool,
) -> Option<Result<u64, String>> {
    let kind = match op {
        M::Addss | M::Addps => 0,
        M::Subss | M::Subps => 1,
        M::Mulss | M::Mulps => 2,
        _ => return None,
    };
    let a = a as u32;
    let b = b as u32;
    let ea = (a >> 23) & 255;
    let eb = (b >> 23) & 255;
    if state.mxcsr & 0x6000 != 0
        || !(1..255).contains(&ea)
        || !(1..255).contains(&eb)
        || !host_rounds_normally()
    {
        return None;
    }
    let fa = f32::from_bits(a);
    let fb = f32::from_bits(b);
    let value = match kind {
        0 => fa + fb,
        1 => fa - fb,
        _ => fa * fb,
    };
    let bits = value.to_bits();
    if !(1..255).contains(&((bits >> 23) & 255)) {
        return None;
    }
    if state.mxcsr & 0x1020 == 0x1020 {
        return Some(Ok(u64::from(bits)));
    }
    // A product of two 24-bit significands is exact in binary64. Addition
    // and subtraction are exact there when exponents differ by at most 24.
    // Beyond that distance a normal binary32 result cannot be exact, even
    // when binary64 itself discards the smaller operand.
    let inexact = if kind != 2 && ea.abs_diff(eb) > 24 {
        true
    } else {
        let wide = match kind {
            0 => f64::from(fa) + f64::from(fb),
            1 => f64::from(fa) - f64::from(fb),
            _ => f64::from(fa) * f64::from(fb),
        };
        wide != f64::from(value)
    };
    if inexact {
        state.mxcsr |= 0x20;
        if state.mxcsr & 0x1000 == 0 {
            *exception = true;
            return Some(Err("unmasked SIMD floating-point exception (#XM)".into()));
        }
    }
    Some(Ok(u64::from(bits)))
}

#[inline(always)]
fn host_rounds_normally() -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        let control: u64;
        // Read only: preserve the calling thread's floating-point controls.
        unsafe {
            std::arch::asm!("mrs {control}, fpcr", control = out(reg) control,
                options(nomem, nostack, preserves_flags));
        }
        control & ((3 << 22) | (1 << 24) | 0x9f00) == 0
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_apfloat::ieee::Single;
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn host_floating_point_controls_are_checked_without_modification() {
        struct Restore(u64);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    std::arch::asm!("msr fpcr, {0}", in(reg) self.0, options(nomem, nostack, preserves_flags));
                }
            }
        }
        let original: u64;
        unsafe {
            std::arch::asm!("mrs {0}, fpcr", out(reg) original, options(nomem, nostack, preserves_flags));
        }
        let restore = Restore(original);
        for mode in [1 << 22, 1 << 24] {
            let changed = original | mode;
            unsafe {
                std::arch::asm!("msr fpcr, {0}", in(reg) changed, options(nomem, nostack, preserves_flags));
            }
            let mut state = SimdState {
                mxcsr: 0x1f80,
                rflags: 0xad7,
            };
            let mut exception = false;
            assert!(single(&mut state, M::Mulss, 0x3f800000, 0x3f000000, &mut exception).is_none());
            assert_eq!(
                (state.mxcsr, state.rflags, exception),
                (0x1f80, 0xad7, false)
            );
            let after: u64;
            unsafe {
                std::arch::asm!("mrs {0}, fpcr", out(reg) after, options(nomem, nostack, preserves_flags));
            }
            assert_eq!(after, changed);
        }
        drop(restore);
    }

    #[test]
    fn normal_single_fast_path_matches_exact_arithmetic_and_exceptions() {
        let mut seed = 0x71b4a69352c8e10fu64;
        let edge = [
            0x00800000, 0x00800001, 0x3f000000, 0x3f800000, 0x3f800001, 0x7f7fffff, 0x80000000, 0,
            1, 0x7f800000, 0x7fc12345, 0x7f812345, 0x33800000, 0x33000000,
        ];
        let mut exercised = 0;
        for n in 0..100_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let a = if n < edge.len() * edge.len() {
                edge[n / edge.len()]
            } else {
                seed as u32
            };
            let b = if n < edge.len() * edge.len() {
                edge[n % edge.len()]
            } else {
                (seed >> 32) as u32
            };
            for op in [M::Addss, M::Subss, M::Mulss] {
                for control in [0x1f80, 0x1fa0, 0x0f80, 0x9fc0, 0x3f80] {
                    let mut fast = SimdState {
                        mxcsr: control,
                        rflags: 0xad7,
                    };
                    let mut exact = fast;
                    let mut fe = false;
                    let mut ee = false;
                    if let Some(result) = single(&mut fast, op, u64::from(a), u64::from(b), &mut fe)
                    {
                        exercised += 1;
                        let expected =
                            exact.arithmetic::<Single>(op, u64::from(a), u64::from(b), &mut ee);
                        assert_eq!(result, expected, "{op:?} a={a:x} b={b:x} MXCSR={control:x}");
                        assert_eq!(
                            (fast.mxcsr, fast.rflags, fe),
                            (exact.mxcsr, exact.rflags, ee)
                        );
                    }
                }
            }
        }
        if cfg!(target_arch = "aarch64") && host_rounds_normally() {
            assert!(exercised > 100_000);
        }
    }
}
