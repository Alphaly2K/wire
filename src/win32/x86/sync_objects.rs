//! PE32 synchronization adapters borrow objects, scheduler, memory and error slot.
use super::object_waits::wake_obj_waiters;
use crate::{
    api::read_str,
    mem::GuestMem,
    thread::Sched,
    win32::{
        catalog::{MutexApi, SyncObjectApi},
        kernel::objects32::{HandleObj, KernelObjects32},
    },
};

pub(crate) fn create_named(
    handles: &mut KernelObjects32,
    last_error: &mut u32,
    name: &str,
    make: impl FnOnce() -> HandleObj,
) -> (u32, bool) {
    let (h, existed) = handles.create_named(name, make);
    if existed {
        *last_error = 183; // ERROR_ALREADY_EXISTS
    }
    (h, existed)
}

pub(crate) fn sync_call(
    id: SyncObjectApi,
    args: &[u32],
    mem: &GuestMem,
    handles: &mut KernelObjects32,
    sched: &mut Sched,
    last_error: &mut u32,
) -> Result<(u32, String), String> {
    use SyncObjectApi::*;
    let mut note = String::new();
    let value = match id {
        CreateEventA | CreateEventW => {
            let nm = read_str(mem, args[3], id.wide());
            let (h, existed) = create_named(handles, last_error, &nm, || HandleObj::Event {
                manual: args[1] != 0,
                signaled: args[2] != 0,
            });
            note = format!(
                "event \"{nm}\" manual={} signaled={}{}",
                args[1] != 0,
                args[2] != 0,
                if existed { " (already exists)" } else { "" }
            );
            h
        }
        OpenEventA | OpenEventW => {
            let nm = read_str(mem, args[2], id.wide());
            match handles.named(&nm) {
                Some(h) => {
                    note = format!("event \"{nm}\"");
                    h
                }
                None => {
                    *last_error = 2; // ERROR_FILE_NOT_FOUND
                    note = format!("event \"{nm}\" not found");
                    0
                }
            }
        }
        SetEvent => match handles.get_mut(&args[0]) {
            Some(HandleObj::Event { manual, signaled }) => {
                let manual = *manual;
                *signaled = true;
                let woken = wake_obj_waiters(sched, args[0], if manual { usize::MAX } else { 1 });
                note = format!("woke {woken} waiter(s)");
                1
            }
            _ => {
                *last_error = 6;
                note = "bad event handle".into();
                0
            }
        },
        ResetEvent => match handles.get_mut(&args[0]) {
            Some(HandleObj::Event { signaled, .. }) => {
                *signaled = false;
                1
            }
            _ => {
                *last_error = 6;
                0
            }
        },
        PulseEvent => match handles.get_mut(&args[0]) {
            Some(HandleObj::Event { manual, signaled }) => {
                let manual = *manual;
                *signaled = false;
                let woken = wake_obj_waiters(sched, args[0], if manual { usize::MAX } else { 1 });
                note = format!("woke {woken} waiter(s)");
                1
            }
            _ => {
                *last_error = 6;
                0
            }
        },
        CreateSemaphoreW | CreateSemaphoreA => {
            let nm = read_str(mem, args[3], id.wide());
            let (initial, max) = (args[1] as i32, args[2] as i32);
            let (h, existed) = create_named(handles, last_error, &nm, || HandleObj::Semaphore {
                count: initial,
                max,
            });
            note = format!(
                "semaphore \"{nm}\" {initial}/{max}{}",
                if existed { " (already exists)" } else { "" }
            );
            h
        }
        ReleaseSemaphore => match handles.get_mut(&args[0]) {
            Some(HandleObj::Semaphore { count, max }) => {
                let prev = *count;
                let n = args[1] as i32;
                *count = (*count + n).min(*max);
                if args[2] != 0 {
                    mem.write_u32(args[2], prev as u32)
                        .map_err(|e| e.to_string())?;
                }
                let woken = wake_obj_waiters(sched, args[0], n.max(0) as usize);
                note = format!("count {prev} -> {}, woke {woken}", *count);
                1
            }
            _ => {
                *last_error = 6;
                note = "bad semaphore handle".into();
                0
            }
        },
        CreateEventExA | CreateEventExW | CreateSemaphoreExA | CreateSemaphoreExW
        | OpenSemaphoreA | OpenSemaphoreW => {
            unreachable!("ABI-only exports use the legacy unimplemented policy")
        }
    };
    Ok((value, note))
}

pub(crate) fn mutex_call(
    id: MutexApi,
    args: &[u32],
    mem: &GuestMem,
    handles: &mut KernelObjects32,
    sched: &mut Sched,
    last_error: &mut u32,
) -> Result<(u32, String), String> {
    use MutexApi::*;
    let mut note = String::new();
    let value = match id {
        CreateA | CreateW => {
            let nm = read_str(mem, args[2], id == CreateW);
            let me = sched.current_tid();
            let own = args[1] != 0;
            let (h, existed) = create_named(handles, last_error, &nm, || HandleObj::Mutex {
                owner: if own { Some(me) } else { None },
                count: if own { 1 } else { 0 },
            });
            note = format!(
                "mutex \"{nm}\"{}",
                if existed { " (already exists)" } else { "" }
            );
            h
        }
        Release => {
            let me = sched.current_tid();
            match handles.get_mut(&args[0]) {
                Some(HandleObj::Mutex { owner, count }) if *owner == Some(me) => {
                    *count -= 1;
                    if *count == 0 {
                        *owner = None;
                        wake_obj_waiters(sched, args[0], 1);
                    }
                    1
                }
                Some(HandleObj::Mutex { .. }) => {
                    *last_error = 288; // ERROR_NOT_OWNER
                    note = "not the owner".into();
                    0
                }
                _ => {
                    *last_error = 6;
                    note = "bad mutex handle".into();
                    0
                }
            }
        }
        CreateExA | CreateExW | OpenA | OpenW => {
            unreachable!("ABI-only exports use the legacy unimplemented policy")
        }
    };
    Ok((value, note))
}
