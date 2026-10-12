//! Fixed compiled ARM64 adapters and scoped same-owner Win64 reentry. No
//! generated trampolines, borrowed guest payloads or executable host mappings.
use super::super::super::runtime::Control;
use super::super::{Cpu64, Memory, Runtime64};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
const STACK_BASE: u64 = 0x6ffd00000000;
const STACK_BYTES: u64 = 1024 * 1024;
const RETURN: u64 = 0x6ffd00100000;
const WORK_LIMIT: u64 = 2_000_000;
// Resource loads, prefab clones and render-pipeline changes can traverse whole
// asset graphs. The actual metadata owner/protocol selects this bounded work
// allowance; ordinary calls retain their small limit.
const ASSET_WORK_LIMIT: u64 = 128_000_000;
#[derive(Clone, Copy)]
pub(super) enum Work {
    Ordinary,
    Asset,
}
impl Work {
    fn limit(self) -> u64 {
        match self {
            Self::Ordinary => WORK_LIMIT,
            Self::Asset => ASSET_WORK_LIMIT,
        }
    }
}
struct BytePolicy {
    reference_class: Option<usize>,
    work: Work,
}
const STACK_SLICE: u64 = STACK_BYTES / 8;
#[derive(Clone, Copy, Default)]
pub(super) struct Stats {
    pub(super) calls: [usize; 7],
    pub(super) gc_events: u32,
    pub(super) mono_reentries: usize,
    pub(super) managed_reentries: usize,
}
struct Owner<'a> {
    memory: RefCell<&'a mut Memory>,
    runtime: RefCell<&'a mut Runtime64>,
    targets: [u64; 7],
    depth: Cell<usize>,
    stack_mapped: Cell<bool>,
    calls: Cell<[usize; 7]>,
    gc_events: Cell<u32>,
    mono_reentries: Cell<usize>,
    managed_reentries: Cell<usize>,
    reference_outputs: Cell<[Option<(u64, usize)>; 8]>,
    metadata: Option<RefCell<super::reentry::Metadata<'a>>>,
}
impl Drop for Owner<'_> {
    fn drop(&mut self) {
        if self.stack_mapped.get() {
            if let Err(error) = self.memory.get_mut().unmap(STACK_BASE, STACK_BYTES) {
                eprintln!("NATIVE_MONO_CALLBACK_STACK_REJECT: {error}");
                std::process::abort();
            }
        }
    }
}
thread_local! { static OWNER: Cell<*const Owner<'static>> = const { Cell::new(ptr::null()) }; }
/// Query only the paused runtime's allocation ledger. The caller already owns
/// its Memory borrow and must independently check permissions/code history.
pub(super) fn live_temporary_slot(address: u64, width: u64) -> Result<bool, String> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Ok(false);
        }
        let owner = unsafe { &*slot.get() };
        let runtime = owner
            .runtime
            .try_borrow()
            .map_err(|_| "temporary reference runtime is already borrowed")?;
        Ok(runtime.live_data_slot(address, width))
    })
}
/// Exact first-word reference output leases of fixed FastPath adapters. No
/// neighboring bytes or general byte payloads become managed reference slots.
pub(super) fn live_reference_output(address: u64) -> Option<super::Object> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return None;
        }
        let owner = unsafe { &*slot.get() };
        owner
            .reference_outputs
            .get()
            .into_iter()
            .flatten()
            .find_map(|(at, class)| (at == address).then_some(class as super::Object))
    })
}
struct Scope;
impl Drop for Scope {
    fn drop(&mut self) {
        OWNER.with(|slot| slot.set(ptr::null()));
    }
}
pub(super) fn with_owner<T>(
    memory: &mut Memory,
    runtime: &mut Runtime64,
    targets: [u64; 7],
    operation: impl FnOnce() -> T,
) -> Result<(T, Stats), String> {
    with_routes(memory, runtime, targets, None, false, operation)
}
// Worker registration C APIs may raise GC profiler events. The worker owns its
// real Mono TLS; only these owned scalar events reenter the paused guest thread.
// No metadata route or worker managed invocation is installed here.
pub(super) fn with_worker_registration_owner<T>(
    memory: &mut Memory,
    runtime: &mut Runtime64,
    targets: [u64; 7],
    operation: impl FnOnce() -> T,
) -> Result<(T, Stats), String> {
    let guest = runtime.active_thread();
    if guest <= 1 || memory.read_int(runtime.teb() + 0x48, 8)? != guest {
        return Err("worker registration callback owner TEB identity mismatch".into());
    }
    let restricted = [0, 0, 0, targets[3], targets[4], 0, 0];
    with_routes(memory, runtime, restricted, None, true, operation)
}
pub(super) fn with_metadata_owner<'a, T>(
    memory: &'a mut Memory,
    runtime: &'a mut Runtime64,
    targets: [u64; 7],
    metadata: super::reentry::Metadata<'a>,
    operation: impl FnOnce() -> T,
) -> Result<(T, Stats), String> {
    with_routes(memory, runtime, targets, Some(metadata), false, operation)
}
fn with_routes<'a, T>(
    memory: &'a mut Memory,
    runtime: &'a mut Runtime64,
    targets: [u64; 7],
    metadata: Option<super::reentry::Metadata<'a>>,
    worker_registration: bool,
    operation: impl FnOnce() -> T,
) -> Result<(T, Stats), String> {
    if runtime.active_thread() != 1 && !worker_registration {
        return Err("native callback owner must be guest main thread".into());
    }
    if OWNER.with(|slot| !slot.get().is_null()) {
        return Err("native callback owner scope already installed".into());
    }
    let owner = Owner {
        memory: RefCell::new(memory),
        runtime: RefCell::new(runtime),
        targets,
        depth: Cell::new(0),
        stack_mapped: Cell::new(false),
        calls: Cell::new([0; 7]),
        gc_events: Cell::new(0),
        mono_reentries: Cell::new(0),
        managed_reentries: Cell::new(0),
        reference_outputs: Cell::new([None; 8]),
        metadata: metadata.map(RefCell::new),
    };
    // Only the TLS pointer erases the lifetime. Scope clears it before this
    // stack owner and its exclusive Memory/Runtime borrows can be released.
    OWNER.with(|slot| slot.set((&owner as *const Owner<'_>).cast::<Owner<'static>>()));
    let scope = Scope;
    let _execution = super::burst_shared::execution()?;
    let temporary = super::unsafe_buffers::TemporaryScope::enter()?;
    if !worker_registration {
        super::unsafe_buffers::drain_finalizers()?;
        super::profiler_recorders::drain_finalizers()?;
        super::logging::drain_finalizers()?;
        super::owned_resources::drain_finalizers()?;
        if owner.metadata.is_some() {
            super::async_operations::drain_finalizers()?;
        }
    }
    let result = operation();
    super::burst_shared::validate_handles()?;
    if !worker_registration {
        super::unsafe_buffers::drain_finalizers()?;
        super::profiler_recorders::drain_finalizers()?;
        super::logging::drain_finalizers()?;
        super::owned_resources::drain_finalizers()?;
        if owner.metadata.is_some() {
            super::async_operations::drain_finalizers()?;
        }
    }
    temporary.verify()?;
    drop(scope);
    let stats = Stats {
        calls: owner.calls.get(),
        gc_events: owner.gc_events.get(),
        mono_reentries: owner.mono_reentries.get(),
        managed_reentries: owner.managed_reentries.get(),
    };
    Ok((result, stats))
}
// Native classlib services borrow the existing guest process only for a bounded
// synchronous operation. No managed/native pointer is passed into this closure.
pub(super) fn has_owner() -> bool {
    OWNER.with(|slot| !slot.get().is_null())
}
pub(super) fn with_service<T>(
    operation: impl FnOnce(&mut Memory, &mut Runtime64) -> Result<T, String>,
) -> Result<T, String> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Err("native service has no live same-thread guest owner".into());
        }
        let owner = unsafe { &*slot.get() };
        let mut memory = owner
            .memory
            .try_borrow_mut()
            .map_err(|_| "native service memory is already borrowed")?;
        let mut runtime = owner
            .runtime
            .try_borrow_mut()
            .map_err(|_| "native service runtime is already borrowed")?;
        operation(&mut memory, &mut runtime)
    })
}
/// Borrow only for native/guest identity conversion; callers release this
/// scope before executing engine code or managed IL that may reenter it.
pub(super) fn with_metadata_service<T>(
    operation: impl FnOnce(&mut super::reentry::Metadata<'_>, &mut Memory) -> Result<T, String>,
) -> Result<T, String> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Err("native identity service has no callback owner".into());
        }
        let owner = unsafe { &*slot.get() };
        let route = owner
            .metadata
            .as_ref()
            .ok_or("native identity service has no metadata route")?;
        let mut route = route
            .try_borrow_mut()
            .map_err(|_| "native identity metadata is already borrowed")?;
        let mut memory = owner
            .memory
            .try_borrow_mut()
            .map_err(|_| "native identity memory is already borrowed")?;
        operation(&mut route, &mut memory)
    })
}
fn execute(owner: &Owner<'_>, slot: usize, args: &[u64]) -> Result<u64, String> {
    let target = *owner
        .targets
        .get(slot)
        .ok_or("callback slot outside table")?;
    if target == 0 || args.len() > 4 {
        return Err("native callback target/arity not admitted".into());
    }
    let value = execute_guest(owner, target, args)?;
    let mut calls = owner.calls.get();
    calls[slot] += 1;
    owner.calls.set(calls);
    Ok(value)
}
fn execute_guest(owner: &Owner<'_>, target: u64, args: &[u64]) -> Result<u64, String> {
    Ok(execute_scalar_guest(owner, target, args, &[])?.0)
}
fn execute_scalar_guest(
    owner: &Owner<'_>,
    target: u64,
    args: &[u64],
    floating: &[(usize, u64)],
) -> Result<(u64, u64), String> {
    execute_scalar_guest_bounded(owner, target, args, floating, WORK_LIMIT)
}
fn execute_scalar_guest_bounded(
    owner: &Owner<'_>,
    target: u64,
    args: &[u64],
    floating: &[(usize, u64)],
    work_limit: u64,
) -> Result<(u64, u64), String> {
    super::burst_shared::validate_handles()?;
    if floating
        .iter()
        .any(|&(index, _)| index >= args.len() || index >= 4)
    {
        return Err("guest floating argument position outside ABI".into());
    }
    if target == 0 || args.len() > 16 {
        return Err("guest callback target/arity not admitted".into());
    }
    let depth = owner.depth.get();
    if depth >= 8 {
        return Err("native callback nesting capacity exceeded".into());
    }
    // Most native metadata leaves never reenter guest code. Allocate the
    // full checked stack only at the first actual callback; keep it scoped
    // to this same owner for GC and nested callbacks, then revoke on Drop.
    if !owner.stack_mapped.get() {
        owner
            .memory
            .try_borrow_mut()
            .map_err(|_| "callback stack memory is already borrowed")?
            .map(
                STACK_BASE,
                STACK_BYTES,
                super::super::super::memory::READ | super::super::super::memory::WRITE,
            )?;
        owner.stack_mapped.set(true);
    }
    owner.depth.set(depth + 1);
    struct Depth<'a>(&'a Cell<usize>, usize);
    impl Drop for Depth<'_> {
        fn drop(&mut self) {
            self.0.set(self.1);
        }
    }
    let _depth = Depth(&owner.depth, depth);
    // Win64 caller reserves 32B shadow space above the return slot.
    // Reserve every positional stack argument as well as shadow/home storage;
    // maintain callee RSP = 8 (mod 16) for signatures up to sixteen arguments.
    let stack_top = STACK_BASE + STACK_BYTES - depth as u64 * STACK_SLICE;
    let call_bytes = (40 + args.len().saturating_sub(4) as u64 * 8).max(72);
    let rsp = stack_top - ((call_bytes + 7) & !15) - 8;
    let teb = owner.runtime.borrow().teb();
    let previous = [
        owner.memory.borrow().read_int(teb + 8, 8)?,
        owner.memory.borrow().read_int(teb + 16, 8)?,
    ];
    struct StackBounds<'a, 'b>(&'a Owner<'b>, u64, [u64; 2]);
    impl Drop for StackBounds<'_, '_> {
        fn drop(&mut self) {
            let mut memory = self.0.memory.borrow_mut();
            if memory
                .write_int(self.1 + 8, 8, self.2[0])
                .and_then(|_| memory.write_int(self.1 + 16, 8, self.2[1]))
                .is_err()
            {
                eprintln!("native callback TEB stack-bound restoration failed");
                std::process::abort();
            }
        }
    }
    let _bounds = StackBounds(owner, teb, previous);
    owner.memory.borrow_mut().write_int(teb + 8, 8, stack_top)?;
    owner
        .memory
        .borrow_mut()
        .write_int(teb + 16, 8, stack_top - STACK_SLICE)?;
    owner.memory.borrow_mut().write_int(rsp, 8, RETURN)?;
    let mut cpu = Cpu64::new(target, rsp);
    cpu.gs_base = teb;
    for (index, value) in args.iter().take(4).enumerate() {
        cpu.regs[[1, 2, 8, 9][index]] = *value;
    }
    for (index, value) in args.iter().skip(4).enumerate() {
        owner
            .memory
            .borrow_mut()
            .write_int(rsp + 40 + index as u64 * 8, 8, *value)?;
    }
    for &(index, bits) in floating {
        cpu.xmm[index][..8].copy_from_slice(&bits.to_le_bytes());
    }
    let mono_start = owner.mono_reentries.get();
    let managed_start = owner.managed_reentries.get();
    // Explicit diagnostic comparison against the transactional x86 interpreter.
    // Normal execution keeps the IR path and its existing bounded work contract.
    let reference = std::env::var_os("WIRE_NATIVE_MONO_CALLBACK_REFERENCE_INTERPRETER")
        .is_some_and(|v| v == "1");
    let diagnostic =
        std::env::var_os("WIRE_NATIVE_MONO_CALLBACK_DIAGNOSTICS").is_some_and(|v| v == "1");
    let mut tail = std::collections::BTreeMap::<u64, usize>::new();
    loop {
        if cpu.rip == RETURN {
            if let Some(route) = owner.metadata.as_ref() {
                route
                    .try_borrow_mut()
                    .map_err(|_| "nested completion metadata is already borrowed")?
                    .sync_guest(&owner.memory.borrow())?;
            }
            return Ok((
                cpu.regs[0],
                u64::from_le_bytes(cpu.xmm[0][..8].try_into().unwrap()),
            ));
        }
        if cpu.work >= work_limit {
            if diagnostic {
                eprintln!("NATIVE_MONO_CALLBACK_LIMIT_TRACE: reference={reference}; last-4096-work-pcs={tail:x?}");
                let memory = owner.memory.borrow();
                for (index, &address) in cpu.regs.iter().enumerate() {
                    let mut header = [0u8; 32];
                    if memory.read(address, &mut header).is_err() {
                        continue;
                    }
                    let pointer = u64::from_le_bytes(header[..8].try_into().unwrap());
                    let mut bytes = [0u8; 1024];
                    let pointee = memory
                        .read(pointer, &mut bytes)
                        .is_ok()
                        .then_some(bytes.as_slice());
                    eprintln!("NATIVE_MONO_CALLBACK_LIMIT_MEMORY: register={index}; address={address:#x}; header={header:02x?}; first-pointer={pointer:#x}; data={pointee:02x?}");
                    for offset in [8, 16, 24] {
                        let pointer =
                            u64::from_le_bytes(header[offset..offset + 8].try_into().unwrap());
                        let mut bytes = [0u8; 64];
                        if memory.read(pointer, &mut bytes).is_ok() {
                            eprintln!("NATIVE_MONO_CALLBACK_LIMIT_POINTEE: register={index}; offset={offset}; pointer={pointer:#x}; data={bytes:02x?}");
                        }
                    }
                }
            }
            return Err(format!(
                "native callback work limit exceeded; target={target:#x}; rip={:#x}; work={}; limit={work_limit}; depth={}; mono-reentries={}; managed-reentries={}; registers={:x?}",
                cpu.rip,
                cpu.work,
                depth + 1,
                owner.mono_reentries.get() - mono_start,
                owner.managed_reentries.get() - managed_start,
                cpu.regs,
            ));
        }
        if diagnostic && cpu.work.saturating_add(4096) >= work_limit {
            *tail.entry(cpu.rip).or_default() += 1;
        }
        let binding = owner.runtime.borrow().gateways.binding(&cpu.rip);
        if let Some(binding) = binding {
            if binding.dll.as_ref() == "mono-2.0-bdwgc.dll" {
                let route = owner.metadata.as_ref().ok_or_else(|| {
                    format!(
                        "nested Mono gateway requires typed reentry: {}",
                        binding.original_function
                    )
                })?;
                if binding.original_function.as_ref() == "mono_runtime_invoke" {
                    let mut prepared = route
                        .try_borrow_mut()
                        .map_err(|_| "nested invoke metadata is already borrowed")?
                        .prepare_invoke(
                            &cpu,
                            &owner.memory.borrow(),
                            &owner.runtime.borrow(),
                            (stack_top - STACK_SLICE, stack_top),
                        )?;
                    // Both temporary RefCell borrows above end before native IL.
                    let native_result = prepared.execute()?;
                    let result = route
                        .try_borrow_mut()
                        .map_err(|_| "nested invoke completion metadata is already borrowed")?
                        .finish_invoke(
                            &prepared,
                            native_result,
                            &mut owner.memory.borrow_mut(),
                            &owner.runtime.borrow(),
                        )?;
                    cpu.return_integer(&owner.memory.borrow(), result)?;
                    owner
                        .managed_reentries
                        .set(owner.managed_reentries.get() + 1);
                } else if binding.original_function.as_ref() == "mono_object_get_virtual_method" {
                    let request = route
                        .try_borrow_mut()
                        .map_err(|_| "nested virtual metadata is already borrowed")?
                        .prepare_virtual(&cpu, &owner.memory.borrow())?;
                    // Resolving generic virtual metadata may allocate. Keep the
                    // pinned receiver but release all guest/registry borrows.
                    let resolved = request.execute()?;
                    let token = route
                        .try_borrow_mut()
                        .map_err(|_| "nested virtual completion metadata is already borrowed")?
                        .finish_virtual(resolved, &mut owner.memory.borrow_mut())?;
                    cpu.return_integer(&owner.memory.borrow(), token)?;
                } else if super::reentry::handles_allocation(&binding.original_function) {
                    let prepared = route
                        .try_borrow_mut()
                        .map_err(|_| "nested allocation metadata is already borrowed")?
                        .prepare_allocation(
                            &binding.original_function,
                            &cpu,
                            &owner.memory.borrow(),
                        )?;
                    let allocation = prepared.execute()?;
                    let token = route
                        .try_borrow_mut()
                        .map_err(|_| "nested allocation completion metadata is already borrowed")?
                        .finish_allocation(allocation, &mut owner.memory.borrow_mut())?;
                    cpu.return_integer(&owner.memory.borrow(), token)?;
                } else {
                    route
                        .try_borrow_mut()
                        .map_err(|_| "nested metadata route is already borrowed")?
                        .dispatch(
                            &binding.original_function,
                            &mut cpu,
                            &mut owner.memory.borrow_mut(),
                            (stack_top - STACK_SLICE, stack_top),
                        )
                        .map_err(|error| {
                            format!(
                                "{error}; function={}; args={:x?}; rsp={:#x}",
                                binding.original_function,
                                [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]],
                                cpu.regs[4]
                            )
                        })?;
                }
                owner.mono_reentries.set(owner.mono_reentries.get() + 1);
                cpu.work += 1;
                continue;
            }
            let control = owner.runtime.borrow_mut().dispatch_bound(
                &binding,
                &mut cpu,
                &mut owner.memory.borrow_mut(),
            )?;
            match control {
                Control::Returned => {}
                Control::Blocked => {
                    let work = super::super::mono_scheduler::pump(
                        &mut owner.memory.borrow_mut(),
                        &mut owner.runtime.borrow_mut(),
                        work_limit - cpu.work,
                    )?;
                    cpu.work += work;
                    continue;
                }
                other => {
                    return Err(format!(
                        "native callback requested a scheduler/loader/control transition not yet admitted; target={target:#x}; rip={:#x}; work={}; function={}!{}; control={other:?}",
                        cpu.rip, cpu.work, binding.dll, binding.original_function,
                    ));
                }
            }
            cpu.work += 1;
        } else if reference {
            cpu.step_reference(&mut owner.memory.borrow_mut())?;
        } else {
            cpu.step_bounded(&mut owner.memory.borrow_mut(), work_limit - cpu.work)?;
        }
    }
}
// Transfer only owned UTF16 and a guest-local span header. The native pinned
// pointer never becomes a guest address. Null and Unity's empty sentinel differ.
pub(super) fn log_span(target: u64, units: Option<&[u16]>) -> Result<(), String> {
    guest_string_span(target, units, &[]).map(|_| ())
}
pub(super) fn guest_string_span(
    target: u64,
    units: Option<&[u16]>,
    trailing: &[u64],
) -> Result<u64, String> {
    guest_positioned_string_span(target, units, &[], trailing)
}
pub(super) fn guest_positioned_string_span(
    target: u64,
    units: Option<&[u16]>,
    leading: &[u64],
    trailing: &[u64],
) -> Result<u64, String> {
    guest_string_span_bounded(target, units, leading, trailing, WORK_LIMIT)
}
pub(super) fn guest_asset_string_span(
    target: u64,
    units: Option<&[u16]>,
    leading: &[u64],
    trailing: &[u64],
) -> Result<u64, String> {
    guest_string_span_bounded(target, units, leading, trailing, ASSET_WORK_LIMIT)
}
fn guest_string_span_bounded(
    target: u64,
    units: Option<&[u16]>,
    leading: &[u64],
    trailing: &[u64],
    work_limit: u64,
) -> Result<u64, String> {
    if leading.len() + trailing.len() > 3 {
        return Err("string span callback arity exceeded".into());
    }
    OWNER.with(|pointer| {
        if pointer.get().is_null() {
            return Err("log span has no live same-thread owner".into());
        }
        let owner = unsafe { &*pointer.get() };
        let units_len = units.map_or(0, <[u16]>::len);
        if units_len > 65536 || owner.depth.get() >= 8 {
            return Err("log span length/depth capacity exceeded".into());
        }
        let address = 0x6ffa00000000 + owner.depth.get() as u64 * 0x21000;
        owner.memory.borrow_mut().map(
            address,
            0x21000,
            super::super::super::memory::READ | super::super::super::memory::WRITE,
        )?;
        struct Buffer<'a, 'b>(&'a Owner<'b>, u64);
        impl Drop for Buffer<'_, '_> {
            fn drop(&mut self) {
                if let Err(error) = self.0.memory.borrow_mut().unmap(self.1, 0x21000) {
                    eprintln!("native logging payload cleanup: {error}");
                    std::process::abort();
                }
            }
        }
        let _buffer = Buffer(owner, address);
        let begin = match units {
            None => 0,
            Some([]) => 1,
            Some(units) => {
                let bytes: Vec<_> = units.iter().flat_map(|c| c.to_le_bytes()).collect();
                owner.memory.borrow_mut().write(address + 16, &bytes)?;
                address + 16
            }
        };
        owner.memory.borrow_mut().write_int(address, 8, begin)?;
        owner
            .memory
            .borrow_mut()
            .write_int(address + 8, 4, units_len as u64)?;
        let mut arguments = leading.to_vec();
        arguments.push(address);
        arguments.extend_from_slice(trailing);
        execute_scalar_guest_bounded(owner, target, &arguments, &[], work_limit).map(|r| r.0)
    })
}
/// Fixed scalar Win64 ABI: integer registers and positional XMM arguments;
/// return both RAX and the exact low XMM0 bits. No native pointer is transferred.
pub(super) fn guest_scalars(
    target: u64,
    args: &[u64],
    floating: &[(usize, u64)],
) -> Result<(u64, u64), String> {
    guest_scalars_with_work(target, args, floating, Work::Ordinary)
}
pub(super) fn guest_scalars_with_work(
    target: u64,
    args: &[u64],
    floating: &[(usize, u64)],
    work: Work,
) -> Result<(u64, u64), String> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Err("scalar engine callback has no owner".into());
        }
        execute_scalar_guest_bounded(
            unsafe { &*slot.get() },
            target,
            args,
            floating,
            work.limit(),
        )
    })
}
pub(super) fn guest_arguments(target: u64, args: &[u64]) -> Result<u64, String> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Err("guest argument callback has no owner".into());
        }
        execute_guest(unsafe { &*slot.get() }, target, args)
    })
}
pub(super) fn guest_asset_arguments(target: u64, args: &[u64]) -> Result<u64, String> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Err("asset callback has no owner".into());
        }
        execute_scalar_guest_bounded(unsafe { &*slot.get() }, target, args, &[], ASSET_WORK_LIMIT)
            .map(|r| r.0)
    })
}
/// UTF16 span arguments share one scoped guest allocation. All positions are
/// Win64 positions, including XMM inputs and the optional 16B output header.
pub(super) type GuestSpanResult = ((u64, u64), Option<[u8; 16]>);
pub(super) fn guest_span_arguments(
    target: u64,
    args: &[u64],
    floating: &[(usize, u64)],
    spans: &[(usize, Option<&[u16]>)],
    output: Option<usize>,
) -> Result<GuestSpanResult, String> {
    OWNER.with(|slot| {
        if slot.get().is_null() || args.len() > 16 || spans.len() > 16 {
            return Err("UTF16 callback owner/arity absent".into());
        }
        let owner = unsafe { &*slot.get() };
        let mut seen = [false; 16];
        let mut offset = 0usize;
        let mut offsets = Vec::new();
        for &(index, units) in spans {
            if index >= args.len() || seen[index] || units.is_some_and(|u| u.len() > 65536) {
                return Err("UTF16 callback span/argument invalid".into());
            }
            seen[index] = true;
            offsets.push(offset);
            offset += 16 + units.map_or(0, |u| u.len() * 2);
            offset = (offset + 15) & !15;
        }
        let output_offset = if let Some(index) = output {
            if index >= args.len() || seen[index] {
                return Err("UTF16 callback output position invalid".into());
            }
            let at = offset;
            offset += 16;
            Some(at)
        } else {
            None
        };
        if floating.iter().enumerate().any(|(i, &(index, _))| {
            index >= args.len()
                || index >= 4
                || seen[index]
                || output == Some(index)
                || floating[..i].iter().any(|&(prior, _)| prior == index)
        }) {
            return Err("UTF16 callback floating position invalid".into());
        }
        let size = (offset.max(1) as u64 + 4095) & !4095;
        if size > 0x110000 || owner.depth.get() >= 8 {
            return Err("UTF16 callback capacity exceeded".into());
        }
        let base = 0x6ff800000000 + owner.depth.get() as u64 * 0x110000;
        owner.memory.borrow_mut().map(
            base,
            size,
            super::super::super::memory::READ | super::super::super::memory::WRITE,
        )?;
        struct Payload<'a, 'b>(&'a Owner<'b>, u64, u64);
        impl Drop for Payload<'_, '_> {
            fn drop(&mut self) {
                if let Err(error) = self.0.memory.borrow_mut().unmap(self.1, self.2) {
                    eprintln!("native UTF16 callback cleanup: {error}");
                    std::process::abort();
                }
            }
        }
        let _payload = Payload(owner, base, size);
        let mut arguments = args.to_vec();
        for (&(index, units), &offset) in spans.iter().zip(&offsets) {
            let address = base + offset as u64;
            let begin = match units {
                None => 0,
                Some([]) => 1,
                Some(units) => {
                    let bytes: Vec<_> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
                    owner.memory.borrow_mut().write(address + 16, &bytes)?;
                    address + 16
                }
            };
            owner.memory.borrow_mut().write_int(address, 8, begin)?;
            owner.memory.borrow_mut().write_int(
                address + 8,
                4,
                units.map_or(0, |u| u.len()) as u64,
            )?;
            arguments[index] = address;
        }
        if let (Some(index), Some(offset)) = (output, output_offset) {
            arguments[index] = base + offset as u64;
        }
        let result = execute_scalar_guest(owner, target, &arguments, floating)?;
        let header = if let Some(offset) = output_offset {
            let mut bytes = [0; 16];
            owner
                .memory
                .borrow()
                .read(base + offset as u64, &mut bytes)?;
            // Output strings must belong to the engine allocator, never to a
            // borrowed input span which is revoked when this scope ends.
            let pointer = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            if (base..base + size).contains(&pointer) {
                return Err("engine output aliases scoped UTF16 input".into());
            }
            Some(bytes)
        } else {
            None
        };
        Ok((result, header))
    })
}
/// Fixed adapters marshal declared byref value bytes into private guest slots.
/// Native addresses never enter the Win64 argument list; output bytes are
/// returned only after the original engine call and all nested reentries finish.
pub(super) type GuestByteResult = ((u64, u64), Vec<Vec<u8>>);
pub(super) fn guest_byte_arguments(
    target: u64,
    args: &[u64],
    spans: &[(usize, &[u8])],
) -> Result<GuestByteResult, String> {
    let spans: Vec<_> = spans
        .iter()
        .map(|&(index, bytes)| (index, bytes, 0))
        .collect();
    guest_byte_arguments_at(target, args, &spans)
}
/// An interior pointer stays inside its declared scoped payload. In particular,
/// Unity's FastPath output points one word beyond the reference it fills.
pub(super) fn guest_byte_arguments_at(
    target: u64,
    args: &[u64],
    spans: &[(usize, &[u8], usize)],
) -> Result<GuestByteResult, String> {
    guest_byte_arguments_impl(
        target,
        args,
        spans,
        BytePolicy {
            reference_class: None,
            work: Work::Ordinary,
        },
        &[],
        &[],
        &[],
    )
}
/// Scalar/value ABI with positional floating inputs and exact byref aliases.
/// Alias sources must be declared payloads; no native address reaches Win64.
pub(super) fn guest_value_arguments(
    target: u64,
    args: &[u64],
    floating: &[(usize, u64)],
    spans: &[(usize, &[u8])],
    aliases: &[(usize, usize)],
) -> Result<GuestByteResult, String> {
    guest_value_span_arguments(target, args, floating, spans, aliases, &[], Work::Ordinary)
}
/// Input strings and byref values share one checked positional payload scope.
pub(super) fn guest_value_span_arguments(
    target: u64,
    args: &[u64],
    floating: &[(usize, u64)],
    spans: &[(usize, &[u8])],
    aliases: &[(usize, usize)],
    utf16: &[(usize, Option<&[u16]>)],
    work: Work,
) -> Result<GuestByteResult, String> {
    let spans: Vec<_> = spans
        .iter()
        .map(|&(index, bytes)| (index, bytes, 0))
        .collect();
    guest_byte_arguments_impl(
        target,
        args,
        &spans,
        BytePolicy {
            reference_class: None,
            work,
        },
        floating,
        aliases,
        utf16,
    )
}
pub(super) fn guest_reference_pair(
    target: u64,
    args: &[u64],
    index: usize,
    class: super::Object,
) -> Result<u64, String> {
    if class.is_null() {
        return Err("FastPath reference output requires a declared class".into());
    }
    let mut pair = [0; 16];
    pair[8..].copy_from_slice(&0x7ac3_d156_29be_048fu64.to_le_bytes());
    let (_, outputs) = guest_byte_arguments_impl(
        target,
        args,
        &[(index, &pair, 8)],
        BytePolicy {
            reference_class: Some(class as usize),
            work: Work::Ordinary,
        },
        &[],
        &[],
        &[],
    )?;
    if outputs[0][8..] != pair[8..] {
        return Err("component FastPath wrote beyond its reference output".into());
    }
    Ok(u64::from_le_bytes(outputs[0][..8].try_into().unwrap()))
}
fn guest_byte_arguments_impl(
    target: u64,
    args: &[u64],
    spans: &[(usize, &[u8], usize)],
    policy: BytePolicy,
    floating: &[(usize, u64)],
    aliases: &[(usize, usize)],
    utf16: &[(usize, Option<&[u16]>)],
) -> Result<GuestByteResult, String> {
    OWNER.with(|slot| {
        if slot.get().is_null() || args.len() > 16 || spans.len() + utf16.len() > 16 {
            return Err("typed byte callback owner/arity absent".into());
        }
        let owner = unsafe { &*slot.get() };
        let mut seen = [false; 16];
        let mut offset = 0usize;
        let mut offsets = Vec::new();
        for &(index, bytes, interior) in spans {
            if index >= args.len()
                || seen[index]
                || bytes.is_empty()
                || bytes.len() > 65536
                || interior >= bytes.len()
            {
                return Err("typed byte callback span/argument invalid".into());
            }
            seen[index] = true;
            offsets.push(offset);
            offset = offset
                .checked_add(bytes.len())
                .and_then(|n| n.checked_add(15))
                .ok_or("typed byte span overflow")?
                & !15;
        }
        let payload_positions = seen;
        for &(index, source) in aliases {
            if index >= args.len()
                || source >= args.len()
                || seen[index]
                || !payload_positions[source]
            {
                return Err("typed byte callback alias is not an exact declared payload".into());
            }
            seen[index] = true;
        }
        let mut string_offsets = Vec::new();
        for &(index, units) in utf16 {
            if index >= args.len() || seen[index] || units.is_some_and(|u| u.len() > 65536) {
                return Err("typed UTF16 input overlaps another payload or exceeds bound".into());
            }
            seen[index] = true;
            string_offsets.push(offset);
            offset = (offset + 16 + units.map_or(0, |u| u.len() * 2) + 15) & !15;
        }
        if floating.iter().enumerate().any(|(i, &(index, _))| {
            index >= args.len()
                || index >= 4
                || seen[index]
                || floating[..i].iter().any(|&(previous, _)| previous == index)
        }) {
            return Err("typed byte callback floating position invalid".into());
        }
        let size = (offset.max(1) as u64 + 4095) & !4095;
        if size > 0x100000 || owner.depth.get() >= 8 {
            return Err("typed byte callback capacity exceeded".into());
        }
        let base = 0x6ff900000000 + owner.depth.get() as u64 * 0x100000;
        owner.memory.borrow_mut().map(
            base,
            size,
            super::super::super::memory::READ | super::super::super::memory::WRITE,
        )?;
        struct Payload<'a, 'b>(&'a Owner<'b>, u64, u64);
        impl Drop for Payload<'_, '_> {
            fn drop(&mut self) {
                if let Err(error) = self.0.memory.borrow_mut().unmap(self.1, self.2) {
                    eprintln!("native typed byte callback cleanup: {error}");
                    std::process::abort();
                }
            }
        }
        let _payload = Payload(owner, base, size);
        struct ReferenceOutput<'a, 'b>(&'a Owner<'b>, usize);
        impl Drop for ReferenceOutput<'_, '_> {
            fn drop(&mut self) {
                let mut outputs = self.0.reference_outputs.get();
                outputs[self.1] = None;
                self.0.reference_outputs.set(outputs);
            }
        }
        let _reference = if let Some(class) = policy.reference_class {
            if spans.len() != 1 || spans[0].1.len() != 16 || spans[0].2 != 8 {
                return Err("reference output requires an exact two-word FastPath payload".into());
            }
            let depth = owner.depth.get();
            let mut outputs = owner.reference_outputs.get();
            if outputs[depth].is_some() {
                return Err("reference output scope already occupied".into());
            }
            outputs[depth] = Some((base, class));
            owner.reference_outputs.set(outputs);
            Some(ReferenceOutput(owner, depth))
        } else {
            None
        };
        let mut args = args.to_vec();
        for (&(index, bytes, interior), &offset) in spans.iter().zip(&offsets) {
            let address = base + offset as u64;
            owner.memory.borrow_mut().write(address, bytes)?;
            args[index] = address + interior as u64;
        }
        for &(index, source) in aliases {
            args[index] = args[source];
        }
        for (&(index, units), &offset) in utf16.iter().zip(&string_offsets) {
            let address = base + offset as u64;
            let begin = match units {
                None => 0,
                Some([]) => 1,
                Some(units) => {
                    let bytes: Vec<_> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
                    owner.memory.borrow_mut().write(address + 16, &bytes)?;
                    address + 16
                }
            };
            owner.memory.borrow_mut().write_int(address, 8, begin)?;
            owner.memory.borrow_mut().write_int(
                address + 8,
                4,
                units.map_or(0, |u| u.len()) as u64,
            )?;
            args[index] = address;
        }
        let result =
            execute_scalar_guest_bounded(owner, target, &args, floating, policy.work.limit())?;
        let mut outputs = Vec::new();
        for (&(_, bytes, _), &offset) in spans.iter().zip(&offsets) {
            let mut out = vec![0; bytes.len()];
            owner.memory.borrow().read(base + offset as u64, &mut out)?;
            outputs.push(out);
        }
        Ok((result, outputs))
    })
}
/// Owned 16B Hash128 in the first Win64 register. Return the copied ref value
/// too, so an engine mutation cannot be silently dropped at this boundary.
pub(super) fn guest_hash128(
    target: u64,
    initial: &[u8; 16],
    trailing: &[u64],
) -> Result<(u64, [u8; 16]), String> {
    if trailing.len() > 3 {
        return Err("Hash128 callback arity exceeded".into());
    }
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Err("Hash128 callback has no same-thread owner".into());
        }
        let owner = unsafe { &*slot.get() };
        let address = 0x6ff900000000 + owner.depth.get() as u64 * 4096;
        owner.memory.borrow_mut().map(
            address,
            4096,
            super::super::super::memory::READ | super::super::super::memory::WRITE,
        )?;
        struct Hash<'a, 'b>(&'a Owner<'b>, u64);
        impl Drop for Hash<'_, '_> {
            fn drop(&mut self) {
                if let Err(error) = self.0.memory.borrow_mut().unmap(self.1, 4096) {
                    eprintln!("native Hash128 payload cleanup: {error}");
                    std::process::abort();
                }
            }
        }
        let _hash = Hash(owner, address);
        owner.memory.borrow_mut().write(address, initial)?;
        let mut args = vec![address];
        args.extend_from_slice(trailing);
        let result = execute_guest(owner, target, &args)?;
        let mut hash = [0; 16];
        owner.memory.borrow().read(address, &mut hash)?;
        Ok((result, hash))
    })
}
/// A locked injected span contains guest addresses only. The native IL pinned
/// input is copied bit-for-bit and revoked on every result, including faults.
pub(super) fn guest_keyframe_span(target: u64, count: usize, bytes: &[u8]) -> Result<u64, String> {
    if count > 4096 || bytes.len() != count * super::animation::KEYFRAME_BYTES {
        return Err("Keyframe span exceeds checked stride/count".into());
    }
    guest_resource_span(target, count, bytes, &[], false)
}
/// Copy UTF16 to a temporary read-only injected span, passing three scalar ABI
/// arguments separately. A non-NULL result must belong to guest memory; the
/// original marker API may return NULL for disabled marker flags.
pub(super) fn guest_marker_span(
    target: u64,
    units: &[u16],
    trailing: &[u64; 3],
) -> Result<u64, String> {
    if units.is_empty() || units.len() > 4096 {
        return Err("marker name span outside bound".into());
    }
    let bytes: Vec<_> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
    guest_resource_span(target, units.len(), &bytes, trailing, true)
}
fn guest_resource_span(
    target: u64,
    count: usize,
    bytes: &[u8],
    trailing: &[u64],
    allow_null: bool,
) -> Result<u64, String> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Err("resource span has no callback owner".into());
        }
        let owner = unsafe { &*slot.get() };
        if owner.depth.get() >= 8 {
            return Err("Keyframe span callback depth exceeded".into());
        }
        let address = 0x6ffb00000000 + owner.depth.get() as u64 * 0x21000;
        let span = (16 + bytes.len() as u64 + 4095) & !4095;
        owner.memory.borrow_mut().map(
            address,
            span,
            super::super::super::memory::READ | super::super::super::memory::WRITE,
        )?;
        struct Buffer<'a, 'b>(&'a Owner<'b>, u64, u64);
        impl Drop for Buffer<'_, '_> {
            fn drop(&mut self) {
                if let Err(error) = self.0.memory.borrow_mut().unmap(self.1, self.2) {
                    eprintln!("native Keyframe span cleanup: {error}");
                    std::process::abort();
                }
            }
        }
        let _buffer = Buffer(owner, address, span);
        let begin = if count == 0 { 0 } else { address + 16 };
        owner.memory.borrow_mut().write_int(address, 8, begin)?;
        owner
            .memory
            .borrow_mut()
            .write_int(address + 8, 4, count as u64)?;
        owner.memory.borrow_mut().write(address + 16, bytes)?;
        owner
            .memory
            .borrow_mut()
            .protect(address, span, super::super::super::memory::READ)?;
        let mut args = vec![address];
        args.extend_from_slice(trailing);
        let value = execute_guest(owner, target, &args)?;
        // The locked marker API returns NULL when flag bit 3 is clear;
        // propagate its actual result. Curve creation must remain non-NULL.
        if value == 0 && allow_null {
            return Ok(0);
        }
        if value == 0
            || !value.is_multiple_of(8)
            || (0x6ffa00000000..STACK_BASE + STACK_BYTES).contains(&value)
        {
            return Err(format!(
                "resource callback returned invalid guest data {value:#x}"
            ));
        }
        let memory = owner.memory.borrow();
        memory.check(value, 8, super::super::super::memory::READ)?;
        if memory
            .check(value, 1, super::super::super::memory::EXECUTE)
            .is_ok()
        {
            return Err("resource creation returned executable guest memory".into());
        }
        Ok(value)
    })
}
pub(super) fn guest_noargs(target: u64) -> Result<(), String> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Err("no-argument guest callback has no owner".into());
        }
        execute_guest(unsafe { &*slot.get() }, target, &[])?;
        Ok(())
    })
}
pub(super) fn reference(slot: usize, args: &[u64]) -> Result<u64, String> {
    OWNER.with(|pointer| {
        let owner = pointer.get();
        if owner.is_null() {
            return Err("reference call has no owner".into());
        }
        let owner = unsafe { &*owner };
        execute(owner, slot, args)
    })
}
pub(super) fn payload(
    slot: usize,
    initial: &[u8; 16],
    trailing: &[u64],
    hidden_return: bool,
) -> Result<[u8; 16], String> {
    OWNER.with(|pointer| {
        if pointer.get().is_null() {
            return Err("native payload callback has no same-thread owner".into());
        }
        if trailing.len() > 3 {
            return Err("native payload callback arity exceeded".into());
        }
        let owner = unsafe { &*pointer.get() };
        let address = 0x6ffc00000000 + owner.depth.get() as u64 * 4096;
        owner.memory.borrow_mut().map(
            address,
            4096,
            super::super::super::memory::READ | super::super::super::memory::WRITE,
        )?;
        struct Payload<'a, 'b>(&'a Owner<'b>, u64);
        impl Drop for Payload<'_, '_> {
            fn drop(&mut self) {
                if let Err(error) = self.0.memory.borrow_mut().unmap(self.1, 4096) {
                    eprintln!("native callback payload cleanup: {error}");
                    std::process::abort();
                }
            }
        }
        let _payload = Payload(owner, address);
        owner.memory.borrow_mut().write(address, initial)?;
        let mut arguments = vec![address];
        arguments.extend_from_slice(trailing);
        let value = execute(owner, slot, &arguments)?;
        if hidden_return && value != address {
            return Err("Win64 payload hidden-sret did not return its owned slot".into());
        }
        let mut bytes = [0; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = owner.memory.borrow().read_int(address + index as u64, 1)? as u8;
        }
        Ok(bytes)
    })
}
fn scalar(slot: usize, args: &[u64]) -> u64 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        OWNER.with(|pointer| {
            let owner = pointer.get();
            if owner.is_null() {
                return Err("native callback has no same-thread execution owner".into());
            }
            let owner = unsafe { &*owner };
            execute(owner, slot, args)
        })
    }));
    match result {
        Ok(Ok(value)) => value,
        other => {
            eprintln!("NATIVE_MONO_CALLBACK_REJECT: slot={slot}; args={args:x?}; result={other:?}; backtrace={}", std::backtrace::Backtrace::force_capture());
            std::process::abort();
        }
    }
}
unsafe extern "C" fn cycle_mode() -> u8 {
    u8::from(scalar(0, &[]) as u8 != 0)
}
unsafe extern "C" fn pause_mode() -> u8 {
    u8::from(scalar(1, &[]) as u8 != 0)
}
pub(super) fn adapter(slot: usize) -> *const c_void {
    [cycle_mode as *const (), pause_mode as *const ()][slot].cast()
}
pub(super) unsafe extern "C" fn profiler_shutdown(prof: *mut c_void) {
    if !prof.is_null() {
        reject_profiler("non-NULL profiler context");
    }
    scalar(2, &[0]);
}
pub(super) unsafe extern "C" fn profiler_gc(prof: *mut c_void, event: i32, generation: i32) {
    if !prof.is_null() || !matches!(event, 0 | 5 | 6 | 7 | 8 | 9 | 10 | 11) || generation < 0 {
        reject_profiler("unverified profiler event/context");
    }
    match super::workers::forward_profiler(super::workers::Profiler::Gc(event, generation)) {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => reject_profiler(&error),
    }
    scalar(3, &[0, event as u32 as u64, generation as u32 as u64]);
    OWNER.with(|pointer| {
        let owner = unsafe { &*pointer.get() };
        owner.gc_events.set(owner.gc_events.get() | (1 << event));
    });
}
pub(super) unsafe extern "C" fn profiler_resize(prof: *mut c_void, size: i64) {
    if !prof.is_null() || size < 0 {
        reject_profiler("unverified profiler resize/context");
    }
    match super::workers::forward_profiler(super::workers::Profiler::Resize(size)) {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => reject_profiler(&error),
    }
    scalar(4, &[0, size as u64]);
}
pub(super) fn worker_profiler(event: super::workers::Profiler) -> Result<(), String> {
    if OWNER.with(|slot| slot.get().is_null()) {
        return Err("queued worker profiler event has no live execution owner".into());
    }
    OWNER.with(|slot| {
        let owner = unsafe { &*slot.get() };
        match event {
            super::workers::Profiler::Gc(event, generation)
                if matches!(event, 0 | 5 | 6 | 7 | 8 | 9 | 10 | 11) && generation >= 0 =>
            {
                execute(
                    owner,
                    3,
                    &[0, event as u32 as u64, generation as u32 as u64],
                )?;
                owner.gc_events.set(owner.gc_events.get() | (1 << event));
            }
            super::workers::Profiler::Resize(size) if size >= 0 => {
                execute(owner, 4, &[0, size as u64])?;
            }
            _ => return Err("queued worker profiler event outside verified contract".into()),
        }
        Ok(())
    })
}
fn reject_profiler(reason: &str) -> ! {
    eprintln!("NATIVE_MONO_PROFILER_REJECT: {reason}");
    std::process::abort()
}
#[cfg(test)]
mod tests {
    use super::super::super::super::memory::{EXECUTE, READ, WRITE};
    #[test]
    fn nested_temporary_heap_slots_exclude_padding_and_freed_allocations() {
        let mut memory = Memory::default();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "temporary-ledger",
        )
        .unwrap();
        let call = |name: &str, args: &[u64], memory: &mut Memory, runtime: &mut Runtime64| {
            let rsp = super::super::super::STACK + 65536 - 72;
            memory.write_int(rsp, 8, 0x1000).unwrap();
            let mut cpu = Cpu64::new(0, rsp);
            for (&index, &word) in [1, 2, 8, 9].iter().zip(args) {
                cpu.regs[index] = word;
            }
            runtime
                .dispatch("kernel32.dll", name, &mut cpu, memory)
                .unwrap();
            cpu.regs[0]
        };
        let heap = call("GetProcessHeap", &[], &mut memory, &mut runtime);
        let block = call("HeapAlloc", &[heap, 8, 24], &mut memory, &mut runtime);
        assert_ne!(block, 0);
        let (checked, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            assert!(live_temporary_slot(block + 16, 8)?);
            assert!(!live_temporary_slot(block + 24, 8)?);
            Ok::<_, String>(())
        })
        .unwrap();
        checked.unwrap();
        assert_eq!(
            call("HeapFree", &[heap, 0, block], &mut memory, &mut runtime),
            1
        );
        let (checked, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            live_temporary_slot(block, 8)
        })
        .unwrap();
        assert!(!checked.unwrap());
    }
    unsafe extern "C" fn allocating_string(
        _: super::super::Object,
        _: *const super::super::c_char,
        _: u32,
    ) -> super::super::Object {
        // A heap expansion callback must execute real guest instructions while
        // the prepared native allocation holds no Rust memory/metadata borrow.
        unsafe { profiler_resize(ptr::null_mut(), 0x20000) };
        4usize as super::super::Object
    }
    unsafe extern "C" fn allocating_object(
        _: super::super::Object,
        _: super::super::Object,
        status: *mut u16,
    ) -> super::super::Object {
        unsafe {
            profiler_resize(ptr::null_mut(), 0x30000);
            *status = 0;
        }
        4usize as super::super::Object
    }
    unsafe extern "C" fn allocating_runtime_type(
        _: super::super::Object,
        _: super::super::Object,
    ) -> super::super::Object {
        unsafe { profiler_resize(ptr::null_mut(), 0x40000) };
        4usize as super::super::Object
    }
    #[test]
    fn prepared_native_allocation_allows_synchronous_heap_resize_guest_reentry() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.write(0x1000, &[0x48, 0x89, 0xd0, 0xc3]).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "allocation-profiler",
        )
        .unwrap();
        let (result, stats) = with_owner(
            &mut memory,
            &mut runtime,
            [0, 0, 0, 0, 0x1000, 0, 0],
            || {
                let allocation = super::super::reentry::PreparedAllocation::String {
                    new: allocating_string,
                    roots: super::super::instances::tests::identity_api(),
                    domain: ptr::null_mut(),
                    bytes: vec![b'a', b'b'],
                    empty: None,
                }
                .execute()?;
                match allocation {
                    super::super::reentry::Allocated::String(root) => {
                        assert_eq!(root.target()?, 4usize as super::super::Object)
                    }
                    _ => return Err("allocation reply kind changed".into()),
                }
                let mut roots = super::super::instances::Registry::default();
                let request = roots.prepare_request(
                    super::super::instances::tests::allocation_api(allocating_object),
                    1usize as super::super::Object,
                    2usize as super::super::Object,
                    None,
                )?;
                let allocation =
                    super::super::reentry::PreparedAllocation::Object(request, None).execute()?;
                match allocation {
                    super::super::reentry::Allocated::Object(allocation, _) => {
                        roots.adopt_allocation(allocation)?;
                    }
                    _ => return Err("object allocation reply kind changed".into()),
                }
                roots.release(super::super::instances::tests::identity_api());
                let allocation = super::super::reentry::PreparedAllocation::RuntimeType {
                    get: allocating_runtime_type,
                    roots: super::super::instances::tests::identity_api(),
                    domain: 1usize as super::super::Object,
                    ty: 2usize as super::super::Object,
                }
                .execute()?;
                match allocation {
                    super::super::reentry::Allocated::RuntimeType(root, ty) => {
                        assert_eq!(root.target()?, 4usize as super::super::Object);
                        assert_eq!(ty, 2usize as super::super::Object);
                    }
                    _ => return Err("RuntimeType allocation reply kind changed".into()),
                }
                Ok::<_, String>(())
            },
        )
        .unwrap();
        result.unwrap();
        assert_eq!(stats.calls[4], 3);
        assert!(memory.read_int(STACK_BASE, 8).is_err());
    }
    use super::*;
    #[test]
    fn native_guest_wait_runs_existing_worker_and_requires_real_address_wake() {
        use super::super::super::{mono_scheduler, Call, Context};
        use std::collections::BTreeMap;
        fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
            code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
            code.extend(value.to_le_bytes());
        }
        for mode in 0..3 {
            let reject_worker_mono = mode == 1;
            let mut memory = Memory::default();
            memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
            memory.map(0x2000, 4096, READ | WRITE).unwrap();
            memory
                .map(super::super::super::STACK, 65536, READ | WRITE)
                .unwrap();
            let mut runtime = Runtime64::new(
                &mut memory,
                0x140000000,
                super::super::super::STACK,
                65536,
                "native-wait",
            )
            .unwrap();
            runtime.enable_scheduler();
            let wait = runtime.register("api-ms-win-core-synch-l1-2-0.dll", "WaitOnAddress");
            let wake = runtime.register("api-ms-win-core-synch-l1-2-0.dll", "WakeByAddressSingle");
            let get_id = runtime.register("kernel32.dll", "GetCurrentThreadId");
            let mono = runtime.register("mono-2.0-bdwgc.dll", "mono_thread_current");
            let mut code = vec![0x48, 0x83, 0xec, 0x28];
            for (register, value) in [
                (1, 0x2000),
                (2, 0x2008),
                (8, 4),
                (9, u32::MAX as u64),
                (0, wait),
            ] {
                mov(&mut code, register, value);
            }
            code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
            memory.write(0x1000, &code).unwrap();
            let mut code = vec![0x48, 0x83, 0xec, 0x28];
            if reject_worker_mono {
                mov(&mut code, 0, mono);
                code.extend([0xff, 0xd0]);
            } else {
                mov(&mut code, 0, get_id);
                code.extend([0xff, 0xd0]);
                mov(&mut code, 2, 0x2010);
                code.extend([0x48, 0x89, 0x02]);
                mov(&mut code, 1, 0x2000);
                code.extend([0xc7, 0x01, 1, 0, 0, 0]);
                mov(&mut code, 0, wake);
                code.extend([0xff, 0xd0]);
            }
            code.extend([0xeb, 0xfe]);
            memory.write(0x1100, &code).unwrap();
            let rsp = super::super::super::STACK + 65536 - 72;
            memory.write_int(rsp, 8, 0x1000).unwrap();
            let mut cpu = Cpu64::new(0, rsp);
            cpu.regs[8] = 0x1100;
            runtime
                .dispatch("kernel32.dll", "CreateThread", &mut cpu, &mut memory)
                .unwrap();
            let spawn = runtime.take_threads().pop().unwrap();
            let worker = spawn.id;
            if mode == 2 {
                // A malformed worker TEB must not strand the selected context
                // or leave Runtime switched away from the native main owner.
                memory.unmap(spawn.cpu.gs_base, 3 * 4096).unwrap();
            }
            let mut contexts = BTreeMap::from([(
                worker,
                Context {
                    rsp: spawn.cpu.regs[4],
                    cpu: spawn.cpu,
                    current: Call {
                        name: "started worker".into(),
                        target: 0x1100,
                        base: 0,
                        notify: false,
                        require_true: false,
                        reason: 0,
                        parameter: 0,
                    },
                    calls: Default::default(),
                    exit_code: None,
                    started: true,
                    callbacks: Vec::new(),
                },
            )]);
            let result = mono_scheduler::with_scheduler(&mut contexts, None, || {
                with_owner(&mut memory, &mut runtime, [0; 7], || {
                    guest_arguments(0x1000, &[])
                })
            })
            .unwrap()
            .unwrap()
            .0;
            assert_eq!(runtime.active_thread(), 1);
            assert!(memory.read_int(STACK_BASE, 8).is_err());
            if mode == 2 {
                assert!(result.is_err());
                assert_eq!(contexts[&worker].cpu.work, 0);
            } else if reject_worker_mono {
                assert!(contexts[&worker].cpu.work > 0);
                assert!(result.unwrap_err().contains("typed worker reentry"));
                assert_eq!(contexts[&worker].cpu.rip, mono);
                assert_eq!(memory.read_int(0x2000, 4).unwrap(), 0);
            } else {
                assert!(contexts[&worker].cpu.work > 0);
                assert_eq!(result.unwrap(), 1);
                assert_eq!(memory.read_int(0x2000, 4).unwrap(), 1);
                assert_eq!(memory.read_int(0x2010, 8).unwrap(), worker);
            }
            assert!(mono_scheduler::pump(&mut memory, &mut runtime, 10)
                .unwrap_err()
                .contains("no cooperative process scheduler"));
        }
    }
    #[test]
    fn callback_work_bound_preserves_exact_return_and_restores_after_exhaustion() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        // mov ecx,2; dec ecx; jne -4; ret -- exactly six work units.
        memory
            .write(0x1000, &[0xb9, 2, 0, 0, 0, 0xff, 0xc9, 0x75, 0xfc, 0xc3])
            .unwrap();
        memory.write(0x1100, &[0xeb, 0xfe]).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "callback-work-bound",
        )
        .unwrap();
        let teb = runtime.teb();
        let bounds = [
            memory.read_int(teb + 8, 8).unwrap(),
            memory.read_int(teb + 16, 8).unwrap(),
        ];
        let (result, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            OWNER.with(|slot| {
                let owner = unsafe { &*slot.get() };
                let error = execute_scalar_guest_bounded(owner, 0x1000, &[], &[], 5).unwrap_err();
                assert!(error.contains("work=5; limit=5; depth=1"));
                assert!(error.contains("rip=0x1009"));
                assert_eq!(owner.depth.get(), 0);
                assert_eq!(owner.memory.borrow().read_int(teb + 8, 8)?, bounds[0]);
                assert_eq!(owner.memory.borrow().read_int(teb + 16, 8)?, bounds[1]);
                execute_scalar_guest_bounded(owner, 0x1000, &[], &[], 6)?;
                let error = execute_scalar_guest_bounded(owner, 0x1100, &[], &[], 11).unwrap_err();
                assert!(error.contains("work=11; limit=11; depth=1"));
                assert!(error.contains("rip=0x1100"));
                assert_eq!(owner.depth.get(), 0);
                Ok::<_, String>(())
            })
        })
        .unwrap();
        result.unwrap();
        assert!(memory.read_int(STACK_BASE, 8).is_err());
    }
    #[test]
    fn resource_work_bound_does_not_extend_ordinary_callbacks() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        // dec ecx; jne -4; ret -- exceeds the scalar budget, then returns.
        memory
            .write(0x1000, &[0xff, 0xc9, 0x75, 0xfc, 0xc3])
            .unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "resource-work-bound",
        )
        .unwrap();
        let (result, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            let iterations = WORK_LIMIT / 2 + 1;
            let before = guest_arguments(0x1000, &[iterations]).unwrap_err();
            assert!(before.contains("work=2000000; limit=2000000"));
            guest_asset_arguments(0x1000, &[iterations])?;
            guest_scalars_with_work(0x1000, &[iterations], &[], Work::Asset)?;
            guest_value_span_arguments(0x1000, &[iterations], &[], &[], &[], &[], Work::Asset)?;
            assert!(guest_value_span_arguments(
                0x1000,
                &[iterations],
                &[],
                &[],
                &[],
                &[],
                Work::Ordinary
            )
            .unwrap_err()
            .contains("limit=2000000"));
            let after = guest_arguments(0x1000, &[iterations]).unwrap_err();
            assert!(after.contains("work=2000000; limit=2000000"));
            Ok::<_, String>(())
        })
        .unwrap();
        result.unwrap();
        assert!(memory.read_int(STACK_BASE, 8).is_err());
    }
    #[test]
    fn utf16_arguments_preserve_two_spans_float_output_and_fault_cleanup() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.map(0x2000, 4096, READ | WRITE).unwrap();
        // Return the first UTF16 unit from each span through a guest output
        // allocation, preserving the actual third Win64 output position.
        // mov rax,[rcx]; movzx eax,word [rax]; mov r9,[rdx];
        // movzx r9d,word [r9]; add eax,r9d; mov [0x2000],rax;
        // mov qword [r8],0x2000; mov dword [r8+8],1; ret
        memory
            .write(
                0x1000,
                &[
                    0x48, 0x8b, 0x01, 0x0f, 0xb7, 0x00, 0x4c, 0x8b, 0x0a, 0x45, 0x0f, 0xb7, 0x09,
                    0x44, 0x01, 0xc8, 0x48, 0xa3, 0, 0x20, 0, 0, 0, 0, 0, 0, 0x49, 0xc7, 0x00, 0,
                    0x20, 0, 0, 0x41, 0xc7, 0x40, 0x08, 1, 0, 0, 0, 0xc3,
                ],
            )
            .unwrap();
        // movaps xmm0,xmm1; ret -- f32 default/result bits, including NaN.
        memory.write(0x1100, &[0x0f, 0x28, 0xc1, 0xc3]).unwrap();
        // An engine output must not borrow an input which is about to retire.
        // mov rax,[rcx]; mov [r8],rax; ret
        memory
            .write(0x1200, &[0x48, 0x8b, 0x01, 0x49, 0x89, 0x00, 0xc3])
            .unwrap();
        // mov rax,[0]; ret -- exercise RAII payload teardown after a fault.
        memory
            .write(0x1300, &[0x48, 0xa1, 0, 0, 0, 0, 0, 0, 0, 0, 0xc3])
            .unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "UTF16-arguments",
        )
        .unwrap();
        let (result, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            let spans = [
                (0, Some([0x3042u16].as_slice())),
                (1, Some([0x3044u16].as_slice())),
            ];
            let result = guest_span_arguments(0x1000, &[0; 3], &[], &spans, Some(2))?;
            assert_eq!(result.0 .0, 0x6086);
            let header = result.1.unwrap();
            assert_eq!(u64::from_le_bytes(header[..8].try_into().unwrap()), 0x2000);
            assert_eq!(i32::from_le_bytes(header[8..12].try_into().unwrap()), 1);
            for bits in [0x80000000u64, 0x7fc01234, 0x3fa00000] {
                assert_eq!(
                    guest_span_arguments(0x1100, &[0; 2], &[(1, bits)], &[(0, None)], None)?
                        .0
                         .1,
                    bits
                );
            }
            assert!(guest_span_arguments(0x1200, &[0; 3], &[], &spans, Some(2)).is_err());
            assert!(guest_span_arguments(0x1300, &[0; 3], &[], &spans, Some(2)).is_err());
            assert!(guest_span_arguments(0x1000, &[0; 3], &[], &spans, Some(0)).is_err());
            assert!(guest_span_arguments(0x1000, &[0; 3], &[(1, 0)], &spans, Some(2)).is_err());
            assert!(with_service(|memory, _| memory.read_int(0x6ff800000000, 8)).is_err());
            Ok::<_, String>(())
        })
        .unwrap();
        result.unwrap();
        assert!(memory.read_int(0x6ff800000000, 8).is_err());
    }
    #[test]
    fn byte_arguments_preserve_positions_and_revoke_private_spans() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        // mov rax,[rcx]; add rax,rdx; add rax,r8; mov [r9],rax; ret
        memory
            .write(
                0x1000,
                &[
                    0x48, 0x8b, 0x01, 0x48, 0x01, 0xd0, 0x4c, 0x01, 0xc0, 0x49, 0x89, 0x01, 0xc3,
                ],
            )
            .unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        // mov [r8-8],rcx; ret -- FastPath fills the preceding word.
        memory
            .write(0x1100, &[0x49, 0x89, 0x48, 0xf8, 0xc3])
            .unwrap();
        memory.write(0x1200, &[0x0f, 0x0b]).unwrap();
        // Preserve XMM1 bits alongside a scalar value and a UTF16 header;
        // the last byref argument deliberately aliases the first value.
        memory
            .write(
                0x1300,
                &[
                    0x66, 0x0f, 0x7e, 0xc8, 0x03, 0x01, 0x41, 0x03, 0x40, 0x08, 0x41, 0x89, 0x01,
                    0xc3,
                ],
            )
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "byte-arguments",
        )
        .unwrap();
        let (result, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            let input = 11u64.to_le_bytes();
            let result = guest_byte_arguments(0x1000, &[0, 2, 3, 0], &[(0, &input), (3, &[0; 8])])?;
            assert_eq!(result.0 .0, 16);
            assert_eq!(result.1[0], input);
            assert_eq!(result.1[1], 16u64.to_le_bytes());
            assert!(with_service(|memory, _| memory.read_int(0x6ff900000000, 8)).is_err());
            assert_eq!(
                guest_reference_pair(0x1100, &[71, 0, 0], 2, 1usize as super::super::Object)?,
                71
            );
            assert!(live_reference_output(0x6ff900000000).is_none());
            assert!(
                guest_reference_pair(0x1200, &[0; 3], 2, 1usize as super::super::Object).is_err()
            );
            assert!(live_reference_output(0x6ff900000000).is_none());
            assert!(with_service(|memory, _| memory.read_int(0x6ff900000000, 8)).is_err());
            assert!(guest_byte_arguments(0x1000, &[0; 4], &[(4, &input)]).is_err());
            assert!(guest_byte_arguments(0x1000, &[0; 4], &[(0, &input), (0, &input)]).is_err());
            let mut pair = [0; 16];
            pair[8..].copy_from_slice(&0x1234u64.to_le_bytes());
            let output = guest_byte_arguments_at(0x1100, &[71, 0, 0], &[(2, &pair, 8)])?.1;
            assert_eq!(&output[0][..8], &71u64.to_le_bytes());
            assert_eq!(&output[0][8..], &pair[8..]);
            for invalid in [16, usize::MAX] {
                assert!(guest_byte_arguments_at(0x1100, &[0; 3], &[(2, &pair, invalid)]).is_err());
            }
            assert!(guest_byte_arguments_at(0x1200, &[0; 3], &[(2, &pair, 8)]).is_err());
            assert_eq!(pair[..8], [0; 8]);
            assert!(with_service(|memory, _| memory.read_int(0x6ff900000000, 8)).is_err());
            let input = 11u32.to_le_bytes();
            let (_, bytes) = guest_value_span_arguments(
                0x1300,
                &[0; 4],
                &[(1, 512)],
                &[(0, &input)],
                &[(3, 0)],
                &[(2, Some(&[0x4e00, 0xd83d, 0xde00]))],
                Work::Ordinary,
            )?;
            assert_eq!(bytes, [526u32.to_le_bytes().to_vec()]);
            assert!(guest_value_span_arguments(
                0x1300,
                &[0; 4],
                &[],
                &[(0, &input)],
                &[(2, 0)],
                &[(2, None)],
                Work::Ordinary
            )
            .is_err());
            assert!(with_service(|memory, _| memory.read_int(0x6ff900000000, 8)).is_err());
            Ok::<_, String>(())
        })
        .unwrap();
        result.unwrap();
        assert!(memory.read_int(0x6ff900000000, 8).is_err());
    }
    #[test]
    fn worker_registration_preserves_guest_identity_and_restricts_callbacks() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        // mov rax, gs:[0x48]; ret -- observe the actual callback guest TEB.
        memory
            .write(0x1000, &[0x65, 0x48, 0x8b, 0x04, 0x25, 0x48, 0, 0, 0, 0xc3])
            .unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "worker",
        )
        .unwrap();
        runtime.enable_scheduler();
        let rsp = super::super::super::STACK + 65536 - 72;
        memory.write_int(rsp, 8, 0x1500).unwrap();
        let mut cpu = Cpu64::new(0, rsp);
        cpu.gs_base = runtime.teb();
        cpu.regs[8] = 0x1000;
        runtime
            .dispatch("kernel32.dll", "CreateThread", &mut cpu, &mut memory)
            .unwrap();
        // Resolve the actual created identity through the existing scheduler.
        let worker = runtime.take_threads().pop().unwrap().id;
        assert_ne!(cpu.regs[0], 0);
        runtime.activate_thread(worker);
        let (actual, stats) =
            with_worker_registration_owner(&mut memory, &mut runtime, [0x1000; 7], || {
                worker_profiler(super::super::workers::Profiler::Gc(0, 0)).unwrap();
                assert!(reference(0, &[]).is_err());
                reference(3, &[0, 0, 0])
            })
            .unwrap();
        assert_eq!(actual.unwrap(), worker);
        assert_eq!(stats.calls[3], 2);
        assert_eq!(stats.gc_events, 1);
        assert_eq!(runtime.active_thread(), worker);
        memory.write_int(runtime.teb() + 0x48, 8, 1).unwrap();
        assert!(
            with_worker_registration_owner(&mut memory, &mut runtime, [0x1000; 7], || ()).is_err()
        );
    }
    #[test]
    fn win64_stack_arguments_follow_shadow_space_and_keep_sixteen_argument_bound() {
        use super::super::super::super::memory::{EXECUTE, READ, WRITE};
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        // Original Win64 instructions read arguments five and eight from
        // the caller's stack above the return slot and 32B home area.
        memory
            .write(
                0x1000,
                &[
                    0x48, 0x8b, 0x44, 0x24, 0x28, 0x48, 0x03, 0x44, 0x24, 0x40, 0xc3,
                ],
            )
            .unwrap();
        // The sixteenth argument also needs reserved caller stack space.
        memory
            .write(
                0x1100,
                &[
                    0x48, 0x8b, 0x44, 0x24, 0x28, 0x48, 0x03, 0x84, 0x24, 0x80, 0, 0, 0, 0xc3,
                ],
            )
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "test",
        )
        .unwrap();
        let (result, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            assert!(guest_arguments(0x1000, &[1; 17]).is_err());
            assert_eq!(
                guest_arguments(
                    0x1100,
                    &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
                )?,
                21
            );
            guest_arguments(0x1000, &[1, 2, 3, 4, 51, 6, 7, 83])
        })
        .unwrap();
        assert_eq!(result.unwrap(), 134);
        assert!(memory.read_int(STACK_BASE, 8).is_err());
    }
    #[test]
    fn marker_span_forwards_scalar_arguments_and_revokes_owned_utf16_payload() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.write(0x1000, &[0x48, 0x89, 0xd0, 0xc3]).unwrap(); // mov rax,rdx; ret
        memory.write(0x1100, &[0x31, 0xc0, 0xc3]).unwrap();
        memory.map(0x2000, 4096, READ | WRITE).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "test",
        )
        .unwrap();
        let (result, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            guest_marker_span(0x1000, &[0x41, 0x3042], &[0x2000, 0, 0])
        })
        .unwrap();
        assert_eq!(result.unwrap(), 0x2000);
        let (null, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            guest_marker_span(0x1100, &[0x41], &[1, 0, 0])
        })
        .unwrap();
        assert_eq!(null.unwrap(), 0);
        assert!(memory.read_int(0x6ffb00000000, 8).is_err());
        assert!(guest_marker_span(0x1000, &[], &[0, 0, 0]).is_err());
    }
    #[test]
    fn leaf_owner_allocates_no_stack_and_lazy_callback_collision_preserves_existing_memory() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.write(0x1000, &[0xb8, 42, 0, 0, 0, 0xc3]).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "test",
        )
        .unwrap();
        let (_, stats) = with_owner(&mut memory, &mut runtime, [0x1000; 7], || {
            with_service(|memory, _| {
                assert!(memory.read_int(STACK_BASE, 8).is_err());
                Ok(())
            })
            .unwrap();
        })
        .unwrap();
        assert_eq!(stats.calls, [0; 7]);
        memory.map(STACK_BASE, 4096, READ | WRITE).unwrap();
        memory.write_int(STACK_BASE, 8, 77).unwrap();
        let (value, _) = with_owner(&mut memory, &mut runtime, [0x1000; 7], || 11).unwrap();
        assert_eq!(value, 11);
        assert_eq!(memory.read_int(STACK_BASE, 8).unwrap(), 77);
        let (failed, _) =
            with_owner(&mut memory, &mut runtime, [0x1000; 7], || reference(0, &[])).unwrap();
        assert!(failed.is_err());
        assert_eq!(memory.read_int(STACK_BASE, 8).unwrap(), 77);
        assert!(memory.read_int(STACK_BASE + STACK_BYTES - 8, 8).is_err());
        assert!(OWNER.with(|slot| slot.get().is_null()));
        memory.unmap(STACK_BASE, 4096).unwrap();
        let (value, stats) =
            with_owner(&mut memory, &mut runtime, [0x1000; 7], || reference(0, &[])).unwrap();
        assert_eq!(value.unwrap(), 42);
        assert_eq!(stats.calls[0], 1);
        assert!(memory.read_int(STACK_BASE, 8).is_err());
    }
    #[test]
    fn hidden_return_and_ref_payloads_are_owned_and_retired_on_success_and_rejection() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        // Authored 16B Win64 hidden-sret: two integer stores and RAX=RCX.
        memory
            .write(
                0x1000,
                &[
                    0x48, 0xb8, 1, 2, 3, 4, 5, 6, 7, 8, 0x48, 0x89, 0x01, 0x48, 0x89, 0x41, 8,
                    0x48, 0x89, 0xc8, 0xc3,
                ],
            )
            .unwrap();
        // Authored ref leaf: overwrite DWORD at +4 from EDX.
        memory.write(0x1100, &[0x89, 0x51, 4, 0xc3]).unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "test",
        )
        .unwrap();
        let (result, stats) = with_owner(
            &mut memory,
            &mut runtime,
            [0, 0, 0, 0, 0, 0x1000, 0x1100],
            || -> Result<[u8; 16], String> {
                let value = payload(5, &[0; 16], &[], true)?;
                payload(6, &value, &[0xdeadbeef], false)
            },
        )
        .unwrap();
        let mut expected = [1, 2, 3, 4, 5, 6, 7, 8, 1, 2, 3, 4, 5, 6, 7, 8];
        expected[4..8].copy_from_slice(&0xdeadbeefu32.to_le_bytes());
        assert_eq!(result.unwrap(), expected);
        assert_eq!(stats.calls[5..], [1, 1]);
        assert!(memory.read_int(0x6ffc00000000, 8).is_err());
        let (result, _) = with_owner(
            &mut memory,
            &mut runtime,
            [0, 0, 0, 0, 0, 0x1100, 0],
            || payload(5, &[0; 16], &[], true),
        )
        .unwrap();
        assert!(result.is_err());
        assert!(memory.read_int(0x6ffc00000000, 8).is_err());
        assert!(memory.read_int(STACK_BASE, 8).is_err());
    }
    #[test]
    fn scalar_callbacks_preserve_positional_xmm_bits_and_independent_integer_return() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        // Authored Win64 movapd xmm0,xmm1; mov rax,r8; ret. This checks
        // positional floating arguments separately from integer ABI slots.
        memory
            .write(0x1000, &[0x66, 0x0f, 0x28, 0xc1, 0x4c, 0x89, 0xc0, 0xc3])
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "scalar-test",
        )
        .unwrap();
        for bits in [0x7ff800000000beefu64, 0x8000000000000000, 0x7fc01234] {
            let (value, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
                guest_scalars(0x1000, &[123, 0, 456], &[(1, bits)])
            })
            .unwrap();
            assert_eq!(value.unwrap(), (456, bits));
            assert!(memory.read_int(STACK_BASE, 8).is_err());
        }
        let (value, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            guest_scalars(0x1000, &[123], &[(1, 7)])
        })
        .unwrap();
        assert!(value.is_err());
        assert!(memory.read_int(STACK_BASE, 8).is_err());
        assert!(guest_scalars(0x1000, &[], &[]).is_err());
    }
    #[test]
    fn log_span_owns_utf16_preserves_sentinels_and_revokes_faulted_payload() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        // Authored Win64: copy span begin/length and two UTF16 units to owned
        // output. Skip data for null/empty; never execute native machine code.
        memory
            .write(
                0x1000,
                &[
                    0x48, 0x8b, 0x01, 0x48, 0xa3, 0x00, 0x11, 0, 0, 0, 0, 0, 0, 0x8b, 0x41, 0x08,
                    0xa3, 0x08, 0x11, 0, 0, 0, 0, 0, 0, 0x85, 0xc0, 0x74, 0x0f, 0x48, 0x8b, 0x01,
                    0x8b, 0x00, 0xa3, 0x0c, 0x11, 0, 0, 0, 0, 0, 0, 0xc3, 0xc3,
                ],
            )
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "test",
        )
        .unwrap();
        let teb = runtime.teb();
        let original_bounds = [
            memory.read_int(teb + 8, 8).unwrap(),
            memory.read_int(teb + 16, 8).unwrap(),
        ];
        // Read gs:[StackLimit] from real guest instructions inside the callback.
        memory
            .write(0x1200, &[0x65, 0x48, 0x8b, 0x04, 0x25, 0x10, 0, 0, 0, 0xc3])
            .unwrap();
        let (value, _) =
            with_owner(&mut memory, &mut runtime, [0x1200; 7], || reference(0, &[])).unwrap();
        assert_eq!(value.unwrap(), STACK_BASE + STACK_BYTES - STACK_SLICE);
        assert_eq!(
            [
                memory.read_int(teb + 8, 8).unwrap(),
                memory.read_int(teb + 16, 8).unwrap()
            ],
            original_bounds
        );
        assert!(log_span(0x1000, Some(&[65, 0x4e2d])).is_err());
        for (units, sentinel) in [
            (None, 0),
            (Some(&[][..]), 1),
            (Some(&[65, 0x4e2d][..]), 0x6ffa00000010),
        ] {
            let (value, _) = with_owner(&mut memory, &mut runtime, [0x1000; 7], || {
                log_span(0x1000, units)
            })
            .unwrap();
            value.unwrap();
            assert_eq!(memory.read_int(0x1100, 8).unwrap(), sentinel);
            assert_eq!(
                memory.read_int(0x1108, 4).unwrap(),
                units.map_or(0, <[u16]>::len) as u64
            );
            if sentinel > 1 {
                assert_eq!(memory.read_int(0x110c, 4).unwrap(), 0x4e2d0041);
            }
            assert!(memory.read_int(0x6ffa00000000, 8).is_err());
        }
        let (value, _) = with_owner(&mut memory, &mut runtime, [0x1000; 7], || {
            log_span(0x3000, Some(&[65, 66]))
        })
        .unwrap();
        assert!(value.is_err());
        assert!(memory.read_int(0x6ffa00000000, 8).is_err());
        assert_eq!(
            [
                memory.read_int(teb + 8, 8).unwrap(),
                memory.read_int(teb + 16, 8).unwrap()
            ],
            original_bounds
        );
    }
    #[test]
    fn nested_guest_metadata_gateway_keeps_identity_and_restores_owner_on_rejection() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "nested-test",
        )
        .unwrap();
        let set = runtime.register("mono-2.0-bdwgc.dll", "mono_class_set_userdata");
        let get = runtime.register("mono-2.0-bdwgc.dll", "mono_class_get_userdata");
        let mut registry = super::super::metadata::Registry::default();
        let mut images = super::super::images::Registry::default();
        let mut native = [0u64; 32];
        let token = registry
            .wrap(
                &mut memory,
                super::super::metadata::Value::Handle(
                    native.as_mut_ptr().cast(),
                    super::super::metadata::Kind::Class,
                ),
            )
            .unwrap();
        let mut code = vec![0x48, 0xb9]; // movabs rcx, existing class token
        code.extend_from_slice(&token.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xba]); // movabs rdx, guest embedding value
        code.extend_from_slice(&0x778899u64.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xb8]);
        code.extend_from_slice(&set.to_le_bytes());
        code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28, 0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28]);
        code.extend_from_slice(&[0x48, 0xb9]);
        code.extend_from_slice(&token.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xb8]);
        code.extend_from_slice(&get.to_le_bytes());
        code.extend_from_slice(&[0xff, 0xe0]); // tail call getter, return to owner sentinel
        memory.write(0x1000, &code).unwrap();
        let (rejected, _) =
            with_owner(&mut memory, &mut runtime, [0x1000; 7], || reference(0, &[])).unwrap();
        assert!(rejected.unwrap_err().contains("requires typed reentry"));
        assert_eq!(memory.read_int(token + 232, 8).unwrap(), 0);
        let (result, stats) = with_metadata_owner(
            &mut memory,
            &mut runtime,
            [0x1000; 7],
            super::super::reentry::Metadata {
                gc: None,
                library: ptr::null_mut(),
                domain: ptr::null_mut(),
                thread_handle: ptr::null_mut(),
                metadata: &mut registry,
                images: &mut images,
                rooted_instance: None,
                objects: None,
            },
            || {
                with_metadata_service(|route, memory| {
                    route.metadata.userdata(memory, token, Some(0x123456))?;
                    assert!(with_metadata_service(|_, _| Ok(())).is_err());
                    Ok(())
                })?;
                reference(0, &[])
            },
        )
        .unwrap();
        assert_eq!(result.unwrap(), 0x778899);
        assert_eq!(stats.mono_reentries, 2);
        assert_eq!(native[29], 0);
        assert!(OWNER.with(|slot| slot.get().is_null()));
        assert!(with_metadata_service(|_, _| Ok(())).is_err());
        assert!(memory.read_int(STACK_BASE, 8).is_err());
        registry.retire(&mut memory).unwrap();
        assert!(memory.read_int(token + 232, 8).is_err());
    }
    #[test]
    fn compiled_adapters_reenter_checked_cpu_without_changing_guest_owner_and_remove_scope() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        // Save RCX in caller shadow space; mov eax,ecx; add eax,7; ret.
        // Authored Win64 test, interpreted only.
        memory
            .write(
                0x1000,
                &[
                    0x48, 0x89, 0x4c, 0x24, 0x20, 0x8b, 0xc1, 0x83, 0xc0, 7, 0xc3,
                ],
            )
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "test",
        )
        .unwrap();
        let (value, calls) =
            with_owner(&mut memory, &mut runtime, [0x1000; 7], || scalar(0, &[35])).unwrap();
        assert_eq!(value, 42);
        assert_eq!(calls.calls, [1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(runtime.active_thread(), 1);
        assert!(OWNER.with(|slot| slot.get().is_null()));
        assert!(memory.read_int(STACK_BASE, 8).is_err());
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = with_owner(&mut memory, &mut runtime, [0x1000; 7], || {
                panic!("authored owner-scope unwind");
            });
        }));
        assert!(unwind.is_err());
        assert!(OWNER.with(|slot| slot.get().is_null()));
        assert!(memory.read_int(STACK_BASE, 8).is_err());
        let (value, stats) =
            with_owner(&mut memory, &mut runtime, [0x1000; 7], || scalar(0, &[35])).unwrap();
        assert_eq!(value, 42);
        assert_eq!(stats.calls[0], 1);
    }
    #[test]
    fn injected_keyframe_span_uses_owned_readonly_bits_and_revokes_on_fault_and_escape() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.map(0x2000, 4096, READ | WRITE).unwrap();
        memory
            .map(super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        // Read the first key's raw 32 bits; copy to persistent guest storage.
        memory
            .write(
                0x1000,
                &[
                    0x48, 0x8b, 0x01, 0x8b, 0x10, 0x48, 0xb8, 0x00, 0x20, 0, 0, 0, 0, 0, 0, 0x89,
                    0x10, 0xc3,
                ],
            )
            .unwrap();
        // Attempt to mutate the readonly injected count.
        memory
            .write(0x1100, &[0xc7, 0x41, 0x08, 0, 0, 0, 0, 0xc3])
            .unwrap();
        // Return temporary payload and callback-stack addresses (escape).
        memory.write(0x1200, &[0x48, 0x8b, 0xc1, 0xc3]).unwrap();
        memory
            .write(0x1300, &[0x48, 0x8d, 0x44, 0x24, 0x20, 0xc3])
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "span-test",
        )
        .unwrap();
        let mut bytes = [0u8; super::super::animation::KEYFRAME_BYTES];
        bytes[..4].copy_from_slice(&0x7fc12345u32.to_le_bytes());
        let (value, _) = with_owner(&mut memory, &mut runtime, [0x1000; 7], || {
            guest_keyframe_span(0x1000, 1, &bytes)
        })
        .unwrap();
        assert_eq!(value.unwrap(), 0x2000);
        assert_eq!(memory.read_int(0x2000, 4).unwrap(), 0x7fc12345);
        for target in [0x1100, 0x1200, 0x1300] {
            let (value, _) = with_owner(&mut memory, &mut runtime, [0x1000; 7], || {
                guest_keyframe_span(target, 1, &bytes)
            })
            .unwrap();
            assert!(value.is_err());
            assert!(memory.read_int(0x6ffb00000000, 8).is_err());
        }
        assert!(guest_keyframe_span(0x1000, 2, &bytes).is_err());
    }
}
