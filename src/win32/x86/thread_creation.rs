//! PE32 thread creation keeps its historical late ID copy-out and partial commits.
use crate::{
    api::{ApiTable, Args},
    heap::GuestHeap,
    mem::GuestMem,
    modules::Modules,
    thread::Sched,
    win32::kernel::objects32::{HandleObj, KernelObjects32},
};

pub(crate) struct Resources<'a> {
    pub handles: &'a mut KernelObjects32,
    pub modules: &'a mut Modules,
    pub heap: &'a mut GuestHeap,
    pub process_heap: u32,
    pub last_error: &'a mut u32,
}

pub(crate) fn call(
    args: &Args,
    mem: &mut GuestMem,
    api: &ApiTable,
    sched: &mut Sched,
    resources: &mut Resources<'_>,
) -> Result<(u32, String), String> {
    if !sched.has_thread_capacity() {
        *resources.last_error = 8;
        return Ok((0, "guest thread resource limit".into()));
    }
    let (entry, param, flags) = (args[2], args[3], args[4]);
    let tid = sched.alloc_tid();
    let exit_stub = api.lookup("wire", "ExitThread").unwrap();
    let handle = resources.handles.alloc(HandleObj::Thread { tid });
    sched.create_thread(mem, entry, param, flags & 4 != 0, tid, handle, exit_stub)?;
    let teb = sched.threads.last().unwrap().teb;
    resources
        .modules
        .setup_tls_thread(mem, resources.heap, resources.process_heap, teb)?;
    if args[5] != 0 {
        mem.write_u32(args[5], tid).map_err(|e| e.to_string())?;
    }
    let note = format!(
        "tid {tid} entry={entry:#x} param={param:#x}{}",
        if flags & 4 != 0 { " (suspended)" } else { "" }
    );
    Ok((handle, note))
}
