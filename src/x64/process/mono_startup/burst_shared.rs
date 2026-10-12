//! Finite diagnostic SharedStatic bridge. Unity supplies the key identity and
//! engine allocation; its exact byte interval and native IL share one backing.
//! This does not admit Burst compilation, executable pointers or job delegates.
use super::{callbacks, managed_calls, Object};
use crate::x64::memory::shared::{ExecutionLease, HostRegion};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ptr,
    sync::{Mutex, OnceLock},
    thread::{self, ThreadId},
};

pub(super) const NAME: &str = "Unity.Burst.LowLevel.BurstCompilerService::GetOrCreateSharedMemory";
pub(super) const TARGET: u64 = 0x1800694a0;
const CAPACITY: usize = 1024 * 1024;
const COUNT: usize = 256;
static TARGET_PC: OnceLock<u64> = OnceLock::new();
static STORAGE: OnceLock<Mutex<Storage>> = OnceLock::new();
thread_local! {
    static EXECUTION: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}
struct Frame {
    regions: Vec<HostRegion>,
    leases: Vec<ExecutionLease>,
}
struct Entry {
    guest: u64,
    size: usize,
    alignment: usize,
    region: HostRegion,
}
struct Storage {
    owner: ThreadId,
    entries: BTreeMap<[u8; 16], Entry>,
    pending: BTreeMap<[u8; 16], usize>,
    bytes: usize,
    calls: usize,
    reused: usize,
    stopping: bool,
    closed: bool,
}
impl Storage {
    fn new() -> Self {
        Self {
            owner: thread::current().id(),
            entries: BTreeMap::new(),
            pending: BTreeMap::new(),
            bytes: 0,
            calls: 0,
            reused: 0,
            stopping: false,
            closed: false,
        }
    }
    fn reserve(&mut self, key: [u8; 16], size: usize) -> Result<(), String> {
        if self.owner != thread::current().id()
            || self.stopping
            || self.closed
            || self.pending.contains_key(&key)
            || self.entries.contains_key(&key)
            || self.entries.len() + self.pending.len() >= COUNT
            || self
                .bytes
                .checked_add(self.pending.values().sum::<usize>())
                .and_then(|n| n.checked_add(size))
                .is_none_or(|n| n > CAPACITY)
        {
            return Err("SharedStatic owner/key/capacity admission failed".into());
        }
        self.pending.insert(key, size);
        Ok(())
    }
}

pub(super) struct Execution {
    depth: usize,
}
impl Drop for Execution {
    fn drop(&mut self) {
        EXECUTION.with(|slot| {
            let mut frames = slot.borrow_mut();
            assert_eq!(frames.len(), self.depth);
            drop(frames.pop());
        });
    }
}
/// A synchronous channel wait freezes this thread's native stack. Release all
/// nested leases before the peer executes, retaining their backing and scope
/// identities. The peer must finish (or pause for a reply) before restoration.
pub(super) struct Suspension {
    depth: usize,
    suspended: Vec<bool>,
}
pub(super) fn suspend() -> Suspension {
    EXECUTION.with(|slot| {
        let mut frames = slot.borrow_mut();
        let suspended = frames
            .iter_mut()
            .map(|frame| {
                let active = !frame.leases.is_empty();
                frame.leases.clear();
                active
            })
            .collect();
        Suspension {
            depth: frames.len(),
            suspended,
        }
    })
}
impl Drop for Suspension {
    fn drop(&mut self) {
        let regions = EXECUTION.with(|slot| {
            let frames = slot.borrow();
            assert_eq!(frames.len(), self.depth);
            frames
                .iter()
                .zip(&self.suspended)
                .map(|(frame, active)| {
                    assert!(frame.leases.is_empty());
                    if *active {
                        frame.regions.clone()
                    } else {
                        Vec::new()
                    }
                })
                .collect::<Vec<_>>()
        });
        let leases = regions
            .iter()
            .map(|regions| {
                regions
                    .iter()
                    .map(HostRegion::execution_lease)
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>();
        let leases = leases.unwrap_or_else(|error| {
            eprintln!("NATIVE_MONO_SHARED_STATIC_RESUME_REJECT: {error}");
            std::process::abort();
        });
        EXECUTION.with(|slot| {
            for (frame, leases) in slot.borrow_mut().iter_mut().zip(leases) {
                frame.leases = leases;
            }
        });
    }
}
/// Hold leases across native IL, not merely during the adapter's byte copy.
/// Mono's actual finalizer profiler brackets acquire the same leases too.
pub(super) fn execution() -> Result<Execution, String> {
    let regions = if let Some(storage) = STORAGE.get() {
        let storage = storage
            .lock()
            .map_err(|_| "SharedStatic registry poisoned")?;
        storage
            .entries
            .values()
            .map(|e| e.region.clone())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    execution_regions(regions)
}
fn execution_regions(regions: Vec<HostRegion>) -> Result<Execution, String> {
    let leases = regions
        .iter()
        .map(HostRegion::execution_lease)
        .collect::<Result<Vec<_>, _>>()?;
    let depth = EXECUTION.with(|slot| {
        let mut frames = slot.borrow_mut();
        frames.push(Frame { regions, leases });
        frames.len()
    });
    Ok(Execution { depth })
}
/// Cleanup waits for finalizers, so it must release the owner's execution
/// leases first. Finalizers retain/lease the backing until their real return.
pub(super) fn prepare_vm_cleanup() -> Result<(), String> {
    if let Some(storage) = STORAGE.get() {
        let mut storage = storage
            .lock()
            .map_err(|_| "SharedStatic registry poisoned")?;
        if storage.owner != thread::current().id() || !storage.pending.is_empty() {
            return Err("SharedStatic shutdown on foreign owner or pending allocation".into());
        }
        storage.stopping = true;
    }
    EXECUTION.with(|slot| {
        for frame in slot.borrow_mut().iter_mut() {
            frame.leases.clear();
        }
    });
    Ok(())
}
pub(super) fn installed() -> bool {
    TARGET_PC.get().is_some()
}
pub(super) fn install(
    library: Object,
    core: Object,
    images: &[Object],
    target: u64,
) -> Result<(), String> {
    if target != TARGET || !images.contains(&core) {
        return Err("SharedStatic target/image mismatch".into());
    }
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing SharedStatic API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const std::ffi::c_char, *const std::ffi::c_char) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const std::ffi::c_char, i32) -> Object
    );
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
    let field = sym!(
        "mono_class_get_field_from_name",
        unsafe extern "C" fn(Object, *const std::ffi::c_char) -> Object
    );
    let offset = sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> u32);
    let field_type = sym!(
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let value_size = sym!(
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let owner = unsafe {
        find(
            core,
            c"Unity.Burst.LowLevel".as_ptr(),
            c"BurstCompilerService".as_ptr(),
        )
    };
    let hash = unsafe { find(core, c"UnityEngine".as_ptr(), c"Hash128".as_ptr()) };
    if owner.is_null() || hash.is_null() {
        return Err("SharedStatic metadata absent".into());
    }
    let method = unsafe { method(owner, c"GetOrCreateSharedMemory".as_ptr(), 3) };
    if method.is_null() || unsafe { token(method) } != 0x0600046a {
        return Err("SharedStatic method/token mismatch".into());
    }
    let corlib = unsafe { corlib() };
    let void = unsafe { find(corlib, c"System".as_ptr(), c"Void".as_ptr()) };
    let uint = unsafe { find(corlib, c"System".as_ptr(), c"UInt32".as_ptr()) };
    managed_calls::Api::load(library)?.fixed_typed_icall(
        method,
        owner,
        &[
            (0xf, void, false),
            (0x11, hash, true),
            (9, uint, false),
            (9, uint, false),
        ],
        images,
    )?;
    let mut alignment = 0;
    if unsafe { value_size(hash, &mut alignment) } != 16 || alignment != 8 {
        return Err("Hash128 size/alignment mismatch".into());
    }
    for (name, expected) in [(c"u64_0", 16), (c"u64_1", 24)] {
        let member = unsafe { field(hash, name.as_ptr()) };
        if member.is_null()
            || unsafe { offset(member) } != expected
            || unsafe { kind(field_type(member)) } != 0xb
        {
            return Err("Hash128 actual field layout mismatch".into());
        }
    }
    STORAGE
        .set(Mutex::new(Storage::new()))
        .map_err(|_| "SharedStatic storage already installed")?;
    TARGET_PC
        .set(target)
        .map_err(|_| "SharedStatic target already installed")?;
    unsafe {
        add(
            c"Unity.Burst.LowLevel.BurstCompilerService::GetOrCreateSharedMemory".as_ptr(),
            shared as *const () as Object,
        );
    }
    Ok(())
}
fn request(size: u32, alignment: u32) -> Result<(usize, usize), String> {
    let alignment = if alignment == 0 {
        4
    } else {
        alignment as usize
    };
    if size != 8 || !alignment.is_power_of_two() || alignment > 4096 {
        return Err(format!("SharedStatic currently requires an 8B engine-handle slot: size={size}; alignment={alignment}"));
    }
    Ok((size as usize, alignment))
}
/// The current caller stores job-reflection engine handles. Reject all native
/// addresses before guest execution/reentry or ordinary-call completion; this
/// is not an admission for arbitrary pointer-bearing SharedStatic payloads.
pub(super) fn validate_handles() -> Result<(), String> {
    let Some(storage) = STORAGE.get() else {
        return Ok(());
    };
    let regions = {
        let storage = storage
            .lock()
            .map_err(|_| "SharedStatic registry poisoned")?;
        storage
            .entries
            .values()
            .map(|entry| entry.region.clone())
            .collect::<Vec<_>>()
    };
    if regions.is_empty() {
        return Ok(());
    }
    callbacks::with_service(|memory, runtime| {
        for region in regions {
            let value = region.with_native(|pointer| unsafe {
                pointer.as_ptr().cast::<u64>().read_unaligned()
            })?;
            if value != 0
                && !(memory.owned_noncode_access(value, 1)
                    && (runtime.live_data_slot(value, 1)
                        || memory.guest_private_data_slot(value, 1)))
            {
                return Err(
                    "SharedStatic value is not NULL or a live original engine data handle".into(),
                );
            }
        }
        Ok(())
    })
}
fn get(key: [u8; 16], size: u32, alignment: u32) -> Result<Object, String> {
    callbacks::with_service(|_, _| Ok(()))?;
    let (size, alignment) = request(size, alignment)?;
    let storage = STORAGE.get().ok_or("SharedStatic storage absent")?;
    let existing = {
        let storage = storage
            .lock()
            .map_err(|_| "SharedStatic registry poisoned")?;
        if storage.owner != thread::current().id() || storage.stopping || storage.closed {
            return Err("SharedStatic called outside its live owner".into());
        }
        storage
            .entries
            .get(&key)
            .map(|e| (e.guest, e.size, e.alignment, e.region.clone()))
    };
    if existing.is_none() {
        storage
            .lock()
            .map_err(|_| "SharedStatic registry poisoned")?
            .reserve(key, size)?;
    }
    struct Pending([u8; 16]);
    impl Drop for Pending {
        fn drop(&mut self) {
            STORAGE
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .pending
                .remove(&self.0);
        }
    }
    let _pending = existing.is_none().then(|| Pending(key));
    // Every request executes the actual engine lookup. A layout mismatch or
    // failed Unity allocation returns its real NULL; never invent an identity.
    let (guest, returned_key) = callbacks::guest_hash128(
        *TARGET_PC.get().ok_or("SharedStatic target absent")?,
        &key,
        &[size as u64, alignment as u64],
    )?;
    if returned_key != key {
        return Err("engine SharedStatic changed the borrowed hash key".into());
    }
    if guest == 0 {
        return Ok(ptr::null_mut());
    }
    let region = if let Some((previous, old_size, old_alignment, region)) = existing {
        if guest != previous || size != old_size || alignment != old_alignment {
            return Err("engine SharedStatic changed an existing key/layout identity".into());
        }
        callbacks::with_service(|memory, _| {
            if !memory.owns_shared_data(guest, &region) {
                return Err("SharedStatic mapping owner changed".into());
            }
            Ok(())
        })?;
        storage
            .lock()
            .map_err(|_| "SharedStatic registry poisoned")?
            .reused += 1;
        region
    } else {
        let mut initial = vec![0; size];
        callbacks::with_service(|memory, runtime| {
            if !guest.is_multiple_of(alignment as u64)
                || !memory.owned_noncode_access(guest, size)
                || !(runtime.live_data_slot(guest, size as u64)
                    || memory.guest_private_data_slot(guest, size))
            {
                return Err(
                    "SharedStatic engine allocation is not exact live aligned noncode bytes".into(),
                );
            }
            memory.read(guest, &mut initial)
        })?;
        let initial_value = u64::from_le_bytes(initial.as_slice().try_into().unwrap());
        if initial_value != 0 {
            callbacks::with_service(|memory, runtime| {
                if !memory.owned_noncode_access(initial_value, 1)
                    || !(runtime.live_data_slot(initial_value, 1)
                        || memory.guest_private_data_slot(initial_value, 1))
                {
                    return Err(
                        "initial SharedStatic value is not a live original engine handle".into(),
                    );
                }
                Ok(())
            })?;
        }
        let region = HostRegion::owned_retained(size, alignment, std::sync::Arc::new(()))?;
        let lease = region.execution_lease()?;
        region.with_native(|pointer| unsafe {
            ptr::copy_nonoverlapping(initial.as_ptr(), pointer.as_ptr(), size);
        })?;
        EXECUTION.with(|slot| -> Result<(), String> {
            let mut slot = slot.borrow_mut();
            let frame = slot
                .last_mut()
                .ok_or("SharedStatic allocation has no native execution lease scope")?;
            frame.regions.push(region.clone());
            frame.leases.push(lease);
            Ok(())
        })?;
        callbacks::with_service(|memory, _| memory.map_shared_overlay(guest, region.clone()))?;
        let mut storage = storage
            .lock()
            .map_err(|_| "SharedStatic registry poisoned")?;
        storage.bytes += size;
        storage.entries.insert(
            key,
            Entry {
                guest,
                size,
                alignment,
                region: region.clone(),
            },
        );
        region
    };
    storage
        .lock()
        .map_err(|_| "SharedStatic registry poisoned")?
        .calls += 1;
    let native = region.with_native(|pointer| pointer.as_ptr().cast())?;
    eprintln!("NATIVE_MONO_SHARED_STATIC_VERIFIED: key={key:02x?}; size={size}; alignment={alignment}; original-engine-allocation={guest:#x}; one-backing=true; native-address-private=true");
    Ok(native)
}
unsafe extern "C" fn shared(key: *const [u8; 16], size: u32, alignment: u32) -> Object {
    match std::panic::catch_unwind(|| -> Result<Object, String> {
        if key.is_null() || !(key as usize).is_multiple_of(8) {
            return Err("SharedStatic hash key is NULL/misaligned".into());
        }
        // The fixed ABI supplies an actual native IL stack Hash128, which is
        // copied before crossing the boundary; no native key pointer escapes.
        get(unsafe { key.read() }, size, alignment)
    }) {
        Ok(Ok(pointer)) => pointer,
        other => {
            eprintln!("NATIVE_MONO_SHARED_STATIC_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
pub(super) fn retire_after_vm_cleanup() -> Result<(), String> {
    let Some(storage) = STORAGE.get() else {
        return Ok(());
    };
    let entries = {
        let storage = storage
            .lock()
            .map_err(|_| "SharedStatic registry poisoned")?;
        if !storage.stopping
            || storage.owner != thread::current().id()
            || !storage.pending.is_empty()
        {
            return Err("SharedStatic retirement before owner VM cleanup".into());
        }
        storage
            .entries
            .values()
            .map(|e| (e.guest, e.region.clone()))
            .collect::<Vec<_>>()
    };
    for (guest, region) in entries {
        callbacks::with_service(|memory, _| memory.restore_shared_overlay(guest, &region))?;
    }
    let mut storage = storage
        .lock()
        .map_err(|_| "SharedStatic registry poisoned")?;
    storage.entries.clear();
    storage.bytes = 0;
    storage.closed = true;
    Ok(())
}
pub(super) fn report() -> String {
    STORAGE.get().map(|storage| {
        let storage = storage.lock().unwrap();
        format!("native-shared-static-calls={}\nnative-shared-static-reused={}\nnative-shared-static-live={}\nnative-shared-static-bytes={}\nnative-shared-static-closed={}\n",
            storage.calls, storage.reused, storage.entries.len(), storage.bytes, storage.closed)
    }).unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_native_scopes_handoff_and_worker_callback_restore_all_leases() {
        use std::sync::mpsc;
        use std::time::Duration;
        let region = HostRegion::owned(8).unwrap();
        let outer = execution_regions(vec![region.clone()]).unwrap();
        let inner = execution_regions(vec![region.clone()]).unwrap();
        let (requests, incoming) = mpsc::sync_channel(1);
        let (replies, outgoing) = mpsc::sync_channel(1);
        let (completed, completion) = mpsc::sync_channel(1);
        let wait = suspend();
        let worker_region = region.clone();
        let worker = thread::spawn(move || {
            let _execution = execution_regions(vec![worker_region.clone()]).unwrap();
            worker_region
                .with_native(|pointer| unsafe {
                    pointer.cast::<u64>().as_ptr().write(41);
                })
                .unwrap();
            {
                let _callback_wait = suspend();
                requests.send(()).unwrap();
                outgoing.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            let value = worker_region
                .with_native(|pointer| unsafe { pointer.cast::<u64>().as_ptr().read() })
                .unwrap();
            assert_eq!(value, 42);
            drop(_execution);
            completed.send(()).unwrap();
        });
        incoming.recv_timeout(Duration::from_secs(2)).unwrap();
        {
            let _callback = execution_regions(vec![region.clone()]).unwrap();
            region
                .with_native(|pointer| unsafe {
                    pointer.cast::<u64>().as_ptr().write(42);
                })
                .unwrap();
        }
        replies.send(()).unwrap();
        completion.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        drop(wait);
        EXECUTION.with(|slot| {
            let frames = slot.borrow();
            assert_eq!(frames.len(), 2);
            assert!(frames.iter().all(|frame| frame.leases.len() == 1));
        });
        drop(inner);
        drop(outer);
        region.revoke().unwrap();
        assert!(execution_regions(vec![region]).is_err());
        EXECUTION.with(|slot| assert!(slot.borrow().is_empty()));
    }
    #[test]
    fn shared_static_request_and_reentrant_capacity_are_bounded() {
        assert_eq!(request(8, 0).unwrap(), (8, 4));
        assert!(request(0, 4).is_err());
        assert!(request(4097, 4).is_err());
        assert!(request(8, 3).is_err());
        let mut storage = Storage::new();
        storage.reserve([1; 16], 4096).unwrap();
        assert!(storage.reserve([1; 16], 4096).is_err());
        storage.stopping = true;
        assert!(storage.reserve([2; 16], 8).is_err());
        assert_eq!(storage.pending.len(), 1);
    }
}
