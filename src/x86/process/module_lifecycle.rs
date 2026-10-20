//! Process-owned PE32 DLL initialization, sidecar IR and unload execution.
//! Modules validates and commits lifecycle state using owned requests.
use super::{
    module_error::{Failure, LoadResult},
    module_notifications::invoke_tls,
    GuestCallOutcome,
};
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    cpu::Cpu,
    mem::GuestMem,
    modules::{Initialization, Transaction},
    thread::Sched,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn initialize(
    handle: u32,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
    txn: &mut Transaction,
) -> LoadResult<()> {
    let Some(Initialization {
        dependencies,
        entry,
        path,
    }) = state.modules.begin_initialization(handle)?
    else {
        return Ok(());
    };
    // Optional install-time sidecar. Its source hash and each instruction's
    // loaded bytes must match; stale/unsupported IR never prevents loading.
    let sidecar = path.with_file_name(format!(
        "{}.wir",
        path.file_name().unwrap().to_string_lossy()
    ));
    if cpu.uses_ir() && sidecar.is_file() {
        let result = (|| -> Result<usize, String> {
            let bytes = std::fs::read(&sidecar).map_err(|e| e.to_string())?;
            let source = std::fs::read(&path).map_err(|e| e.to_string())?;
            let image = crate::ir::Image::parse(&bytes, &source)?.at_base(handle)?;
            cpu.add_module_ir(image, mem)
        })();
        log.note(&format!("module IR {}: {result:?}", path.display()));
    }
    for dependency in dependencies {
        initialize(dependency, cpu, mem, api, state, log, sched, max_instr, txn)?;
    }
    invoke_tls(handle, 1, cpu, mem, api, state, log, sched, max_instr)?;
    if entry != 0 {
        log.note(&format!(
            "DllMain attach module={handle:#x} entry={entry:#x}"
        ));
        match super::call_guest(
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            entry,
            &[handle, 1, 0],
            max_instr,
        )
        .map_err(Failure::Fatal)?
        {
            GuestCallOutcome::Returned(0) => {
                // A failed PROCESS_ATTACH is followed by PROCESS_DETACH while
                // its dependencies and static TLS are still mapped.
                txn.record_initialization(handle);
                return Err(Failure::Expected(1114, "DllMain returned FALSE".into()));
            }
            GuestCallOutcome::Returned(_) => {}
            outcome => return Err(Failure::Control(outcome)),
        }
    }
    state.modules.complete_initialization(handle, txn);
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn unload(
    handle: u32,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> LoadResult<()> {
    let Some(entry) = state.modules.begin_unload(handle)? else {
        return Ok(());
    };
    invoke_tls(handle, 0, cpu, mem, api, state, log, sched, max_instr)?;
    if entry != 0 {
        log.note(&format!(
            "DllMain detach module={handle:#x} entry={entry:#x}"
        ));
        if let outcome @ (GuestCallOutcome::Transferred | GuestCallOutcome::Exited(_)) =
            super::call_guest(
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                entry,
                &[handle, 0, 0],
                max_instr,
            )
            .map_err(Failure::Fatal)?
        {
            return Err(Failure::Control(outcome));
        }
    }
    let retired = state
        .modules
        .finish_unload(mem, &mut state.heap, state.process_heap, handle)?;
    cpu.remove_module_ir(handle, retired.size);
    for dependency in retired.dependencies {
        unload(dependency, cpu, mem, api, state, log, sched, max_instr)?;
    }
    Ok(())
}
