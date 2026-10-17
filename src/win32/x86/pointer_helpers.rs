use crate::{api::Args, mem::GuestMem, win32::catalog::PointerHelperApi};

pub(crate) fn call(id: PointerHelperApi, a: &Args, mem: &GuestMem) -> Result<u32, u32> {
    use PointerHelperApi::*;
    Ok(match id {
        BadRead | BadWrite => (a[1] != 0 && mem.ptr(a[0], a[1], id == BadWrite).is_err()) as u32,
        BadCode => mem.inspect_code(a[0], &mut [0]).is_err() as u32,
        FlushCache => {
            if a[0] != u32::MAX {
                return Err(6);
            }
            1
        }
        MulDiv => {
            let divisor = a[2] as i32 as i64;
            if divisor == 0 {
                return Ok(u32::MAX);
            }
            let n = (a[0] as i32 as i64) * (a[1] as i32 as i64);
            let negative = (n < 0) != (divisor < 0);
            let quotient = (n.unsigned_abs() + divisor.unsigned_abs() / 2) / divisor.unsigned_abs();
            let result = if negative {
                -(quotient as i64)
            } else {
                quotient as i64
            };
            i32::try_from(result).map_or(u32::MAX, |n| n as u32)
        }
    })
}
