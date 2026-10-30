//! Wide integer operations used by concrete Wasm imports. Inputs and results
//! are values; these functions do not inspect instructions or CPU state.

const CRC32C: [u32; 256] = {
    let mut values = [0; 256];
    let mut index = 0;
    while index < values.len() {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0x82f63b78 } else { 0 };
            bit += 1;
        }
        values[index] = crc;
        index += 1;
    }
    values
};

/// Accumulates the low BYTES bytes in little-endian order, without complement.
pub fn crc32<const BYTES: usize>(mut crc: u32, value: u64) -> u32 {
    assert!(BYTES <= 8);
    for byte in 0..BYTES {
        crc = (crc >> 8) ^ CRC32C[((crc ^ (value >> (byte * 8)) as u32) & 255) as usize];
    }
    crc
}

pub fn mul_high(a: u64, b: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) >> 64) as u64
}

pub fn imul_high(a: u64, b: u64) -> u64 {
    (((a as i64 as i128) * (b as i64 as i128)) >> 64) as u64
}

pub fn div64(low: u64, high: u64, divisor: u64) -> Option<[u64; 2]> {
    if high >= divisor {
        return None;
    }
    let dividend = u128::from(high) << 64 | u128::from(low);
    Some([
        (dividend / u128::from(divisor)) as u64,
        (dividend % u128::from(divisor)) as u64,
    ])
}

pub fn idiv64(low: u64, high: u64, divisor: u64) -> Option<[u64; 2]> {
    let dividend = (u128::from(high) << 64 | u128::from(low)) as i128;
    let divisor = divisor as i64 as i128;
    let quotient = i64::try_from(dividend.checked_div(divisor)?).ok()?;
    Some([quotient as u64, (dividend % divisor) as u64])
}
