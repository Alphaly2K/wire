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
        metadata: metadata.map(RefCell::new),
    };
    // Only the TLS pointer erases the lifetime. Scope clears it before this
    // stack owner and its exclusive Memory/Runtime borrows can be released.
    OWNER.with(|slot| slot.set((&owner as *const Owner<'_>).cast::<Owner<'static>>()));
    let scope = Scope;
    let _execution = super::burst_shared::execution()?;
    if !worker_registration {
        super::unsafe_buffers::drain_finalizers()?;
        super::profiler_recorders::drain_finalizers()?;
    }
    let result = operation();
    super::burst_shared::validate_handles()?;
    if !worker_registration {
        super::unsafe_buffers::drain_finalizers()?;
        super::profiler_recorders::drain_finalizers()?;
    }
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
    super::burst_shared::validate_handles()?;
    if floating
        .iter()
        .any(|&(index, _)| index >= args.len() || index >= 4)
    {
        return Err("guest floating argument position outside ABI".into());
    }
    if target == 0 || args.len() > 8 {
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
    // Leave 64B for shadow/home saves and maintain callee RSP = 8 (mod 16).
    let stack_top = STACK_BASE + STACK_BYTES - depth as u64 * STACK_SLICE;
    let rsp = stack_top - 72;
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
    while cpu.work < WORK_LIMIT {
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
                    let prepared = route
                        .try_borrow_mut()
                        .map_err(|_| "nested invoke metadata is already borrowed")?
                        .prepare_invoke(
                            &cpu,
                            &owner.memory.borrow(),
                            (stack_top - STACK_SLICE, stack_top),
                        )?;
                    // Both temporary RefCell borrows above end before native IL.
                    let native_result = prepared.execute()?;
                    let result = route
                        .try_borrow_mut()
                        .map_err(|_| "nested invoke completion metadata is already borrowed")?
                        .finish_invoke(&prepared, native_result, &mut owner.memory.borrow_mut())?;
                    if prepared.exception_slot != 0 {
                        owner
                            .memory
                            .borrow_mut()
                            .write_int(prepared.exception_slot, 8, 0)?;
                    }
                    cpu.return_integer(&owner.memory.borrow(), result)?;
                    owner
                        .managed_reentries
                        .set(owner.managed_reentries.get() + 1);
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
                                "{error}; args={:x?}; rsp={:#x}",
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
            match control { Control::Returned=>{}, _=>return Err("native callback requested a scheduler/loader/control transition not yet admitted".into()) }
            cpu.work += 1;
        } else {
            cpu.step_bounded(&mut owner.memory.borrow_mut(), WORK_LIMIT - cpu.work)?;
        }
    }
    Err("native callback work limit exceeded".into())
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
        execute_guest(owner, target, &arguments)
    })
}
/// Fixed scalar Win64 ABI: integer registers and positional XMM arguments;
/// return both RAX and the exact low XMM0 bits. No native pointer is transferred.
pub(super) fn guest_scalars(
    target: u64,
    args: &[u64],
    floating: &[(usize, u64)],
) -> Result<(u64, u64), String> {
    OWNER.with(|slot| {
        if slot.get().is_null() {
            return Err("scalar engine callback has no owner".into());
        }
        execute_scalar_guest(unsafe { &*slot.get() }, target, args, floating)
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
/// Fixed adapters marshal declared byref value bytes into private guest slots.
/// Native addresses never enter the Win64 argument list; output bytes are
/// returned only after the original engine call and all nested reentries finish.
pub(super) type GuestByteResult = ((u64, u64), Vec<Vec<u8>>);
pub(super) fn guest_byte_arguments(
    target: u64,
    args: &[u64],
    spans: &[(usize, &[u8])],
) -> Result<GuestByteResult, String> {
    OWNER.with(|slot| {
        if slot.get().is_null() || args.len() > 8 || spans.len() > 8 {
            return Err("typed byte callback owner/arity absent".into());
        }
        let owner = unsafe { &*slot.get() };
        let mut seen = [false; 8];
        let mut offset = 0usize;
        let mut offsets = Vec::new();
        for &(index, bytes) in spans {
            if index >= args.len() || seen[index] || bytes.is_empty() || bytes.len() > 65536 {
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
        let mut args = args.to_vec();
        for (&(index, bytes), &offset) in spans.iter().zip(&offsets) {
            let address = base + offset as u64;
            owner.memory.borrow_mut().write(address, bytes)?;
            args[index] = address;
        }
        let result = execute_scalar_guest(owner, target, &args, &[])?;
        let mut outputs = Vec::new();
        for (&(_, bytes), &offset) in spans.iter().zip(&offsets) {
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
            eprintln!("NATIVE_MONO_CALLBACK_REJECT: {other:?}");
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
    use super::*;
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
            assert!(guest_byte_arguments(0x1000, &[0; 4], &[(4, &input)]).is_err());
            assert!(guest_byte_arguments(0x1000, &[0; 4], &[(0, &input), (0, &input)]).is_err());
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
    fn win64_stack_arguments_follow_shadow_space_and_keep_eight_argument_bound() {
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
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::STACK,
            65536,
            "test",
        )
        .unwrap();
        let (result, _) = with_owner(&mut memory, &mut runtime, [0; 7], || {
            assert!(guest_arguments(0x1000, &[1; 9]).is_err());
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
