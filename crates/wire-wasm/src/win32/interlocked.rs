//! Calls complete within one cooperative guest scheduling interval.
use super::{call::Call, catalog::InterlockedApi as I, slist, ServiceError};
use crate::{
    machine::Mode,
    memory::{Memory, WRITE},
};

pub(super) fn dispatch(id: I, call: &Call, memory: &mut Memory) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    if matches!(
        id,
        I::Increment | I::Decrement | I::Exchange | I::CompareExchange
    ) {
        let old = memory.read_int(a[0], 4)? as u32;
        let value = match id {
            I::Increment => old.wrapping_add(1),
            I::Decrement => old.wrapping_sub(1),
            I::CompareExchange if old != a[2] as u32 => return Ok(old.into()),
            _ => a[1] as u32,
        };
        memory.write_int(a[0], 4, value.into())?;
        return Ok(if matches!(id, I::Increment | I::Decrement) {
            value.into()
        } else {
            old.into()
        });
    }

    let wide = memory.mode() == Mode::X64;
    let alignment = if wide { 16 } else { 8 };
    let width = if wide { 8 } else { 4 };
    if !a[0].is_multiple_of(alignment) {
        return Err(ServiceError::Unavailable("unaligned SLIST_HEADER"));
    }
    if id == I::InitializeSListHead {
        memory.write(a[0], &[0; 16][..alignment as usize])?;
        return Ok(0);
    }
    let mut header = [0; 16];
    memory.read(a[0], &mut header[..alignment as usize])?;
    let packed = u64::from_le_bytes(header[..8].try_into().unwrap());
    let region = u64::from_le_bytes(header[8..].try_into().unwrap());
    let previous = if wide {
        region & !15
    } else {
        packed as u32 as u64
    };
    let depth = if wide {
        packed as u16
    } else {
        (packed >> 32) as u16
    };
    let (next, depth) = match id {
        I::FlushSList => (0, 0),
        I::PopSList => {
            if previous == 0 {
                return Ok(0);
            }
            if !previous.is_multiple_of(alignment) {
                return Err(ServiceError::Unavailable("unaligned SLIST_ENTRY"));
            }
            let next = memory.read_int(previous, width)?;
            if !next.is_multiple_of(alignment) {
                return Err(ServiceError::Unavailable("unaligned SLIST_ENTRY link"));
            }
            (next, depth.wrapping_sub(1))
        }
        I::PushSList => {
            if a[1] == 0 || !a[1].is_multiple_of(alignment) {
                return Err(ServiceError::Unavailable("unaligned or NULL SLIST_ENTRY"));
            }
            memory.check(a[1], width, WRITE)?;
            (a[1], depth.wrapping_add(1))
        }
        _ => unreachable!(),
    };
    // Neither link is published until all stores can complete.
    memory.check(a[0], alignment as usize, WRITE)?;
    if wide {
        header = slist::update64(packed, region, next, depth);
    } else {
        header[..8].copy_from_slice(&slist::update32(packed, next as u32, depth).to_le_bytes());
    }
    if id == I::PushSList {
        memory.write_int(a[1], width, previous)?;
    }
    memory.write(a[0], &header[..alignment as usize])?;
    Ok(previous)
}
