//! Win64 currently admits only initialization of a 16-byte SLIST header.
use crate::{win32::catalog::InterlockedApi, x64::memory::Memory};

pub(crate) fn call(mem: &mut Memory, id: InterlockedApi, a: &[u64]) -> Result<u64, String> {
    if id != InterlockedApi::InitializeSListHead {
        unreachable!("unbound Win64 interlocked API");
    }
    if !a[0].is_multiple_of(16) {
        return Err("unaligned Win64 SLIST_HEADER".into());
    }
    mem.write(a[0], &[0; 16])?;
    Ok(0)
}
