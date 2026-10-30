//! Dynamic slots share process allocation markers and per-thread TEB storage.
pub(crate) use super::local_slots::LocalSlots64 as Slots;
use super::{call::Call, catalog::LocalApi as L, context, ServiceError};
use crate::{
    machine::{Mode, State},
    memory::{Memory, WRITE},
    process::Loaded,
};

pub(crate) fn address(mode: Mode, teb: u64, fls: bool, index: usize) -> u64 {
    teb + if fls {
        0x1800
    } else if mode == Mode::X86 {
        0xe10
    } else {
        0x1480
    } + index as u64 * (mode as u64 / 8)
}
pub(crate) fn clear_slots(
    memory: &mut Memory,
    addresses: &[u64],
) -> Result<(), crate::memory::Fault> {
    let ptr = memory.mode() as usize / 8;
    for &at in addresses {
        memory.check(at, ptr, WRITE)?;
    }
    for &at in addresses {
        memory.write_int(at, ptr, 0)?;
    }
    Ok(())
}

pub(super) fn dispatch(
    id: L,
    call: Call,
    state: &mut State,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<(), ServiceError> {
    if id == L::FlsFree {
        return image.free_fls(call, state, memory);
    }
    let a = &call.arguments;
    let fls = matches!(id, L::FlsAlloc | L::FlsGet | L::FlsSet);
    let allocation = matches!(id, L::TlsAlloc | L::FlsAlloc);
    let ptr = memory.mode() as usize / 8;
    let error = context::error_address(state, memory.mode());
    let result = if allocation {
        let vacant = if fls {
            image.locals.vacant_fls()
        } else {
            image.locals.vacant_tls()
        };
        if let Some(index) = vacant {
            let addresses: Vec<_> = image
                .threads
                .tebs()
                .map(|teb| address(memory.mode(), teb, fls, index))
                .collect();
            clear_slots(memory, &addresses)?;
            if fls {
                image.locals.commit_fls(index, Some((0, a[0])));
            } else {
                image.locals.commit_tls(index, Some(0));
            }
            index as u64
        } else {
            memory.write_int(error, 4, 8)?;
            u64::from(u32::MAX)
        }
    } else {
        let index = a[0] as u32 as usize;
        let allocated = if fls {
            image.locals.fls(index).is_some()
        } else {
            image.locals.tls(index).is_some()
        };
        if !allocated {
            memory.write_int(error, 4, 87)?;
            0
        } else {
            let at = address(memory.mode(), image.threads.teb(), fls, index);
            match id {
                L::TlsGet | L::FlsGet => {
                    let value = memory.read_int(at, ptr)?;
                    if id == L::TlsGet {
                        memory.write_int(error, 4, 0)?;
                    }
                    value
                }
                L::TlsSet | L::FlsSet => {
                    memory.write_int(at, ptr, a[1])?;
                    1
                }
                L::TlsFree => {
                    let addresses: Vec<_> = image
                        .threads
                        .tebs()
                        .map(|teb| address(memory.mode(), teb, false, index))
                        .collect();
                    clear_slots(memory, &addresses)?;
                    image.locals.commit_tls(index, None);
                    1
                }
                _ => unreachable!(),
            }
        }
    };
    call.complete(state, result);
    Ok(())
}
