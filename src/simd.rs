//! Shared SIMD helpers. Exact scalar arithmetic has an isolated codegen boundary.
pub(crate) use wire_simd::{ConvertTo, Scalar, SimdState};

// Select lane width once at the cold vector dispatch boundary. Every copy
// below has a compile-time length; no lane builds a padded temporary buffer.
#[inline(always)]
pub(crate) fn packed<const OP: u8>(a: [u8; 16], b: [u8; 16], width: usize) -> [u8; 16] {
    macro_rules! lanes {
        ($unsigned:ty, $signed:ty, $bytes:literal) => {{
            let mut value = [0; 16];
            for offset in (0..16).step_by($bytes) {
                let a = <$unsigned>::from_le_bytes(a[offset..offset + $bytes].try_into().unwrap());
                let b = <$unsigned>::from_le_bytes(b[offset..offset + $bytes].try_into().unwrap());
                let result = match OP {
                    0 => a.wrapping_add(b),
                    1 => a.wrapping_sub(b),
                    2 => {
                        if a == b {
                            <$unsigned>::MAX
                        } else {
                            0
                        }
                    }
                    3 => {
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
