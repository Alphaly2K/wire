//! Signed Windows MulDiv rounds ties away from zero and returns -1 on failure.
pub(crate) fn mul_div(a: u32, b: u32, divisor: u32) -> u32 {
    let divisor = divisor as i32 as i64;
    if divisor == 0 {
        return u32::MAX;
    }
    let product = (a as i32 as i64) * (b as i32 as i64);
    let negative = (product < 0) != (divisor < 0);
    let quotient = (product.unsigned_abs() + divisor.unsigned_abs() / 2) / divisor.unsigned_abs();
    let result = if negative {
        -(quotient as i64)
    } else {
        quotient as i64
    };
    i32::try_from(result).map_or(u32::MAX, |n| n as u32)
}
