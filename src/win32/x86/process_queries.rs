//! Process query adapters inspect handles; the gateway owns handle creation.
use crate::{
    api::{Args, HandleObj, KernelObjects32},
    mem::GuestMem,
    thread::Sched,
    win32::catalog::ProcessQueryApi,
};

pub(crate) enum ProcessQueryResult {
    Value(u32),
    Open { access: u32 },
}

pub(crate) fn call(
    id: ProcessQueryApi,
    a: &Args,
    mem: &mut GuestMem,
    handles: &KernelObjects32,
    sched: &Sched,
) -> Result<ProcessQueryResult, u32> {
    use ProcessQueryApi::*;
    if id == Open {
        if a[2] != 4242 {
            return Err(if a[2] == 0 { 87 } else { 5 });
        }
        return Ok(ProcessQueryResult::Open { access: a[0] });
    }
    let value = match id {
        Affinity | SetAffinity | Id | ExitCode => {
            if a[0] != u32::MAX {
                let Some(HandleObj::Process { access }) = handles.get(&a[0]) else {
                    return Err(6);
                };
                let required = if id == SetAffinity { 0x200 } else { 0x1400 };
                if access & required == 0 {
                    return Err(5);
                }
            }
            match id {
                Id => 4242,
                ExitCode => {
                    mem.write_u32(a[1], 259).map_err(|_| 87u32)?;
                    1
                }
                SetAffinity => {
                    if a[1] != 1 {
                        return Err(87);
                    }
                    1
                }
                Affinity => {
                    mem.ptr(a[1], 4, true).map_err(|_| 87u32)?;
                    mem.ptr(a[2], 4, true).map_err(|_| 87u32)?;
                    mem.write_u32(a[1], 1).map_err(|_| 87u32)?;
                    mem.write_u32(a[2], 1).map_err(|_| 87u32)?;
                    1
                }
                _ => unreachable!(),
            }
        }
        SetIdealProcessor => {
            if a[0] != 0xfffffffe && sched.thread_index_by_handle(a[0]).is_none() {
                return Err(6);
            }
            if !matches!(a[1], 0 | u32::MAX) {
                return Err(87);
            }
            0
        }
        ActiveProcessorCount | MaximumProcessorCount => {
            if !matches!(a[0], 0 | 0xffff) {
                return Err(87);
            }
            1
        }
        Open => unreachable!(),
        ActiveGroupCount | MaximumGroupCount => {
            unreachable!("PE32 group-count helpers remain ABI-only")
        }
    };
    Ok(ProcessQueryResult::Value(value))
}
