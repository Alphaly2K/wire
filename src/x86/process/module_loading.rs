//! PE32 module API and failed-load cleanup execute at the process boundary.
//! Modules owns mapping metadata and transaction logs; CPU IR changes stay here.
use super::{
    module_lifecycle::{initialize, unload},
    module_notifications::invoke_tls,
    GuestCallOutcome,
};
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    cpu::Cpu,
    mem::GuestMem,
    modules::{memory, Failure, LoadResult, Symbol, Transaction},
    thread::Sched,
};
use std::collections::HashSet;

fn rollback(
    state: &mut ApiState,
    mem: &mut GuestMem,
    cpu: &mut Cpu,
    txn: &Transaction,
) -> Result<(), String> {
    state.modules.rollback_references(txn);
    for &handle in txn.created().iter().rev() {
        if let Some(size) =
            state
                .modules
                .rollback_image(mem, &mut state.heap, state.process_heap, handle)?
        {
            cpu.remove_module_ir(handle, size);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch(
    name: &str,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    let Some(crate::win32::catalog::ApiId::Module(id)) =
        crate::win32::catalog::by_name(name).map(|c| c.id)
    else {
        unreachable!();
    };
    call(id, a, cpu, mem, api, state, log, sched, max_instr)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: crate::win32::catalog::ModuleApi,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    use crate::win32::catalog::ModuleApi;
    let mut txn = Transaction::default();
    let mut note = String::new();
    let result = (|| -> LoadResult<u32> {
        match id {
            ModuleApi::LoadLibraryA
            | ModuleApi::LoadLibraryW
            | ModuleApi::LoadLibraryExA
            | ModuleApi::LoadLibraryExW => {
                if id.extended() && (a[1] != 0 || a[2] & !8 != 0) {
                    return Err(Failure::Expected(
                        87,
                        "unsupported LoadLibraryEx flags/file handle".into(),
                    ));
                }
                let path = crate::api::read_str(mem, a[0], id.wide());
                let handle = state
                    .module_linker(api)
                    .link(&path, None, mem, &mut txn, 0)
                    .map_err(|e| match e {
                        Failure::Expected(code, reason) => {
                            Failure::Expected(code, format!("{path:?}: {reason}"))
                        }
                        other => other,
                    })?;
                for thread in &sched.threads {
                    if !matches!(thread.state, crate::thread::ThreadState::Exited(_)) {
                        state
                            .modules
                            .setup_tls_thread(mem, &mut state.heap, state.process_heap, thread.teb)
                            .map_err(Failure::Fatal)?;
                    }
                }
                initialize(
                    handle, cpu, mem, api, state, log, sched, max_instr, &mut txn,
                )?;
                note = format!("loaded {path:?} at {handle:#x}");
                Ok(handle)
            }
            ModuleApi::DisableThreadLibraryCalls => state.modules.disable_thread_calls(a[0]),
            ModuleApi::FreeLibrary => {
                unload(a[0], cpu, mem, api, state, log, sched, max_instr)?;
                Ok(1)
            }
            ModuleApi::GetProcAddress => {
                let symbol = if a[1] >> 16 == 0 {
                    Symbol::Ordinal(a[1] as u16)
                } else {
                    Symbol::Name(mem.read_cstr(a[1], 1024).map_err(memory)?)
                };
                note = format!("module={:#x} export={symbol:?}", a[0]);
                let owner = state.modules.image_present(a[0]).then_some(a[0]);
                let value = state.module_linker(api).resolve(
                    a[0],
                    symbol,
                    owner,
                    mem,
                    &mut txn,
                    &mut HashSet::new(),
                )?;
                for handle in txn.created().to_vec() {
                    for thread in &sched.threads {
                        if !matches!(thread.state, crate::thread::ThreadState::Exited(_)) {
                            state
                                .modules
                                .setup_tls_thread(
                                    mem,
                                    &mut state.heap,
                                    state.process_heap,
                                    thread.teb,
                                )
                                .map_err(Failure::Fatal)?;
                        }
                    }
                    initialize(
                        handle, cpu, mem, api, state, log, sched, max_instr, &mut txn,
                    )?;
                }
                Ok(value)
            }
            ModuleApi::GetModuleHandleA
            | ModuleApi::GetModuleHandleW
            | ModuleApi::GetModuleHandleExA
            | ModuleApi::GetModuleHandleExW => {
                let extended = id.extended();
                let flags = if extended { a[0] } else { 2 };
                if flags & !7 != 0 || flags & 3 == 3 {
                    return Err(Failure::Expected(87, "invalid module handle flags".into()));
                }
                let pointer = if extended { a[1] } else { a[0] };
                let handle = if flags & 4 != 0 {
                    state.modules.address(pointer).or_else(|| {
                        api.stub_index(pointer)
                            .and_then(|i| state.modules.provider_handle(&api.entries[i].dll))
                    })
                } else if pointer == 0 {
                    Some(state.exe_base)
                } else {
                    let path = crate::api::read_str(mem, pointer, id.wide());
                    state.modules.lookup_handle(&path, state.fs.as_ref())
                }
                .ok_or_else(|| Failure::Expected(126, "module is not loaded".into()))?;
                if extended {
                    mem.ptr(a[2], 4, true).map_err(memory)?;
                    if flags & 2 == 0 {
                        state.modules.acquire(&mut txn, handle)?;
                    }
                    if flags & 1 != 0 {
                        state.modules.pin_image(handle);
                    }
                    mem.write_u32(a[2], handle).map_err(memory)?;
                    Ok(1)
                } else {
                    Ok(handle)
                }
            }
            ModuleApi::GetModuleFileNameA | ModuleApi::GetModuleFileNameW => {
                let path = if a[0] == 0 || a[0] == state.exe_base {
                    state.exe_path.clone()
                } else if let Some(path) = state.modules.guest_image_path(a[0]) {
                    path.clone()
                } else if let Some(path) = state.modules.provider_name(a[0]) {
                    format!("C:\\Windows\\System32\\{path}")
                } else {
                    return Err(Failure::Expected(6, "invalid module handle".into()));
                };
                let wide = id.wide();
                let length = if wide {
                    path.encode_utf16().count()
                } else {
                    crate::codepage::encode(&path, 932)
                        .map_err(|e| {
                            Failure::Expected(e, "module filename encoding failed".into())
                        })?
                        .0
                        .len()
                } as u32;
                if a[2] == 0 {
                    state.last_error = 122;
                    return Ok(0);
                }
                let mut bytes = if wide {
                    path.encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    crate::codepage::encode(&path, 932)
                        .map_err(|e| {
                            Failure::Expected(e, "module filename encoding failed".into())
                        })?
                        .0
                };
                let width = if wide { 2 } else { 1 };
                bytes.truncate(a[2].saturating_sub(1) as usize * width);
                bytes.extend(std::iter::repeat_n(0, width));
                mem.write_bytes(a[1], &bytes).map_err(memory)?;
                if length >= a[2] {
                    state.last_error = 122;
                    Ok(a[2])
                } else {
                    Ok(length)
                }
            }
            ModuleApi::GetDllDirectoryA
            | ModuleApi::GetDllDirectoryW
            | ModuleApi::SetDllDirectoryA
            | ModuleApi::SetDllDirectoryW => {
                unreachable!("ABI-only DLL directory call entered PE32 typed adapter")
            }
        }
    })();
    match result {
        Ok(value) => Ok((GuestCallOutcome::Returned(value), note)),
        Err(Failure::Expected(code, message)) => {
            // Undo successful dependency callbacks in reverse init order.
            for &handle in txn.initialized().iter().rev() {
                let entry = state.modules.initialized_entry(handle);
                if let Err(failure) =
                    invoke_tls(handle, 0, cpu, mem, api, state, log, sched, max_instr)
                {
                    state.modules.abandon(&txn);
                    return match failure {
                        Failure::Control(outcome) => {
                            Ok((outcome, "TLS rollback transferred control".into()))
                        }
                        Failure::Fatal(message) | Failure::Expected(_, message) => Err(message),
                    };
                }
                if entry != 0 {
                    match super::call_guest(
                        cpu,
                        mem,
                        api,
                        state,
                        log,
                        sched,
                        entry,
                        &[handle, 0, 0],
                        max_instr,
                    )? {
                        GuestCallOutcome::Returned(_) => {}
                        outcome => {
                            state.modules.abandon(&txn);
                            return Ok((
                                outcome,
                                "DLL rollback callback transferred control".into(),
                            ));
                        }
                    }
                }
            }
            rollback(state, mem, cpu, &txn)?;
            state.last_error = code;
            Ok((GuestCallOutcome::Returned(0), message))
        }
        Err(Failure::Fatal(message)) => {
            rollback(state, mem, cpu, &txn)?;
            Err(message)
        }
        // Guest exception/exit already chose a new architectural context.
        // Preserve mapped code while it may still be executing there.
        Err(Failure::Control(outcome)) => {
            state.modules.abandon(&txn);
            Ok((outcome, "DLL callback transferred control".into()))
        }
    }
}
