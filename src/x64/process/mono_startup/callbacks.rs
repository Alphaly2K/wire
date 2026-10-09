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
#[derive(Clone, Copy, Default)]
pub(super) struct Stats {
    pub(super) calls: [usize; 7],
    pub(super) gc_events: u32,
}
struct Owner<'a> {
    memory: RefCell<&'a mut Memory>,
    runtime: RefCell<&'a mut Runtime64>,
    targets: [u64; 7],
    depth: Cell<usize>,
    calls: Cell<[usize; 7]>,
    gc_events: Cell<u32>,
}
impl Drop for Owner<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.memory.get_mut().unmap(STACK_BASE, STACK_BYTES) {
            eprintln!("NATIVE_MONO_CALLBACK_STACK_REJECT: {error}");
            std::process::abort();
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
    if runtime.active_thread() != 1 {
        return Err("native callback owner must be guest main thread".into());
    }
    if OWNER.with(|slot| !slot.get().is_null()) {
        return Err("native callback owner scope already installed".into());
    }
    memory.map(
        STACK_BASE,
        STACK_BYTES,
        super::super::super::memory::READ | super::super::super::memory::WRITE,
    )?;
    let owner = Owner {
        memory: RefCell::new(memory),
        runtime: RefCell::new(runtime),
        targets,
        depth: Cell::new(0),
        calls: Cell::new([0; 7]),
        gc_events: Cell::new(0),
    };
    // Only the TLS pointer erases the lifetime. Scope clears it before this
    // stack owner and its exclusive Memory/Runtime borrows can be released.
    OWNER.with(|slot| slot.set((&owner as *const Owner<'_>).cast::<Owner<'static>>()));
    let scope = Scope;
    let result = operation();
    drop(scope);
    let stats = Stats {
        calls: owner.calls.get(),
        gc_events: owner.gc_events.get(),
    };
    Ok((result, stats))
}
fn execute(owner: &Owner<'_>, slot: usize, args: &[u64]) -> Result<u64, String> {
    let target = *owner
        .targets
        .get(slot)
        .ok_or("callback slot outside table")?;
    if target == 0 || args.len() > 4 {
        return Err("native callback target/arity not admitted".into());
    }
    let depth = owner.depth.get();
    if depth >= 8 {
        return Err("native callback nesting capacity exceeded".into());
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
    let rsp = STACK_BASE + STACK_BYTES - 72 - depth as u64 * 65536;
    owner.memory.borrow_mut().write_int(rsp, 8, RETURN)?;
    let mut cpu = Cpu64::new(target, rsp);
    cpu.gs_base = owner.runtime.borrow().teb();
    for (index, value) in args.iter().enumerate() {
        cpu.regs[[1, 2, 8, 9][index]] = *value;
    }
    while cpu.work < WORK_LIMIT {
        if cpu.rip == RETURN {
            let mut calls = owner.calls.get();
            calls[slot] += 1;
            owner.calls.set(calls);
            return Ok(cpu.regs[0]);
        }
        let binding = owner.runtime.borrow().gateways.binding(&cpu.rip);
        if let Some(binding) = binding {
            if binding.dll.as_ref() == "mono-2.0-bdwgc.dll" {
                return Err(format!(
                    "nested Mono gateway requires typed reentry: {}",
                    binding.original_function
                ));
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
    scalar(4, &[0, size as u64]);
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
}
