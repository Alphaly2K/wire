//! Actual Unity awaitable producers/consumers. Managed IL sees never-reused
//! opaque identities; original engine records and transferred GC handles stay private.
use super::{bindings, callbacks, engine_objects, managed_calls, Object};
use std::{
    collections::BTreeMap,
    ffi::CStr,
    sync::{Mutex, OnceLock},
};
const BASE: u64 = 0x6ff400000000;
const SPECS: [(&CStr, u64); 10] = [
    (c"AttachManagedGCHandleToNativeAwaitable", 0x1800ba320),
    (c"ReleaseNativeAwaitable", 0x1800ba3a0),
    (c"CancelNativeAwaitable", 0x1800ba280),
    (c"IsNativeAwaitableCompleted", 0x1800ba440),
    (c"NextFrameInternal", 0x1800ba460),
    (c"WaitForScondsInternal", 0x1800ba550),
    (c"FixedUpdateInternal", 0x1800ba650),
    (c"EndOfFrameInternal", 0x1800ba720),
    (c"FromAsyncOperationInternal", 0x1800ba2d0),
    (c"WireupNextFrameAndEndOfFrameCallbacks", 0x1800ba800),
];
struct Binding {
    library: usize,
    class: usize,
    targets: [u64; 10],
    target: unsafe extern "C" fn(Object) -> Object,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
#[derive(Default)]
struct Registry {
    issued: usize,
    records: BTreeMap<u64, u64>,
}
static RECORDS: Mutex<Registry> = Mutex::new(Registry {
    issued: 0,
    records: BTreeMap::new(),
});
impl Registry {
    fn insert(&mut self, pointer: u64) -> Result<u64, String> {
        if pointer == 0 {
            return Ok(0);
        }
        if self.issued >= 65536 || self.records.len() >= 4096 {
            return Err("awaitable identity capacity exceeded".into());
        }
        let token = BASE + (self.issued as u64 + 1) * 16;
        self.issued += 1;
        self.records.insert(token, pointer);
        Ok(token)
    }
    fn pointer(&self, token: u64) -> Result<u64, String> {
        self.records
            .get(&token)
            .copied()
            .ok_or("unknown or released native awaitable")
            .map_err(Into::into)
    }
}
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn install(
    library: Object,
    core: Object,
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing awaitable API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, *const libc::c_char) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, i32) -> Object
    );
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let owner = unsafe { find(core, c"UnityEngine".as_ptr(), c"Awaitable".as_ptr()) };
    let corlib = unsafe { corlib() };
    let system = |name: &CStr| unsafe { find(corlib, c"System".as_ptr(), name.as_ptr()) };
    let void = (1, system(c"Void"), false);
    let pointer = (0x18, system(c"IntPtr"), false);
    let unsigned = (0x19, system(c"UIntPtr"), false);
    let integer = (8, system(c"Int32"), false);
    let single = (0xc, system(c"Single"), false);
    let signatures = [
        vec![void, pointer, unsigned],
        vec![void, pointer],
        vec![void, pointer],
        vec![integer, pointer],
        vec![pointer],
        vec![pointer, single],
        vec![pointer],
        vec![pointer],
        vec![pointer, pointer],
        vec![void],
    ];
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; 10];
    for (index, ((member, target), signature)) in SPECS.iter().zip(&signatures).enumerate() {
        api.fixed_typed_icall(
            unsafe { method(owner, member.as_ptr(), (signature.len() - 1) as i32) },
            owner,
            signature,
            images,
        )?;
        targets[index] = registry.exact_target(
            &format!("UnityEngine.Awaitable::{}", member.to_str().unwrap()),
            *target,
        )?;
    }
    BINDING
        .set(Binding {
            library: library as usize,
            class: owner as usize,
            targets,
            target: sym!(
                "mono_gchandle_get_target_v2",
                unsafe extern "C" fn(Object) -> Object
            ),
        })
        .map_err(|_| "awaitable adapters already installed")?;
    for ((member, _), adapter) in SPECS.iter().zip([
        attach as *const (),
        release as *const (),
        cancel as *const (),
        completed as *const (),
        next as *const (),
        seconds as *const (),
        fixed as *const (),
        end as *const (),
        from_operation as *const (),
        wireup as *const (),
    ]) {
        let name = std::ffi::CString::new(format!(
            "UnityEngine.Awaitable::{}",
            member.to_str().unwrap()
        ))
        .unwrap();
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    Ok(())
}
fn checked<T>(work: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_AWAITABLE_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_AWAITABLE_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
fn producer(index: usize, args: &[u64], floating: &[(usize, u64)]) -> usize {
    checked(|| {
        let b = BINDING.get().ok_or("awaitable adapters absent")?;
        let pointer = callbacks::guest_scalars(b.targets[index], args, floating)?.0;
        if pointer != 0 {
            engine_objects::engine_pointer(pointer as usize)?;
        }
        let token = RECORDS
            .lock()
            .map_err(|_| "awaitable registry poisoned")?
            .insert(pointer)?;
        eprintln!("NATIVE_MONO_AWAITABLE_CREATED: member={:?}; original-engine=true; opaque-token={token:#x}", SPECS[index].0);
        Ok(token as usize)
    })
}
fn consume(index: usize, token: usize) -> i32 {
    checked(|| {
        let b = BINDING.get().ok_or("awaitable adapters absent")?;
        let pointer = RECORDS
            .lock()
            .map_err(|_| "awaitable registry poisoned")?
            .pointer(token as u64)?;
        engine_objects::engine_pointer(pointer as usize)?;
        let result = callbacks::guest_arguments(b.targets[index], &[pointer])? as i32;
        if index == 1 {
            RECORDS
                .lock()
                .map_err(|_| "awaitable registry poisoned")?
                .records
                .remove(&(token as u64));
        }
        eprintln!(
            "NATIVE_MONO_AWAITABLE_CONSUMED: member={:?}; original-engine=true; result={result}",
            SPECS[index].0
        );
        Ok(result)
    })
}
unsafe extern "C" fn attach(token: usize, handle: usize) {
    checked(|| {
        let b = BINDING.get().ok_or("awaitable adapters absent")?;
        let pointer = RECORDS
            .lock()
            .map_err(|_| "awaitable registry poisoned")?
            .pointer(token as u64)?;
        engine_objects::engine_pointer(pointer as usize)?;
        if handle == 0 || handle & 1 != 0 {
            return Err("awaitable attachment requires a native strong handle".into());
        }
        let native = unsafe { (b.target)(handle as Object) };
        managed_calls::Api::load(b.library as Object)?.compatible(
            native,
            managed_calls::Shape::Reference {
                class: b.class as Object,
                string: false,
            },
        )?;
        if native.is_null() {
            return Err("awaitable native handle lost target".into());
        }
        let guest_handle = callbacks::with_metadata_service(|route, memory| {
            let token = route.root_reference(native, memory)?;
            let roots = route
                .objects
                .as_mut()
                .ok_or("awaitable attachment has no roots")?;
            let guest =
                roots
                    .handles
                    .1
                    .adopt_native(roots.handles.0, handle as Object, native, token)?;
            roots.handles.1.publish(memory, guest)?;
            Ok(guest)
        })?;
        callbacks::guest_arguments(b.targets[0], &[pointer, guest_handle])?;
        Ok(())
    });
}
unsafe extern "C" fn release(token: usize) {
    consume(1, token);
}
unsafe extern "C" fn cancel(token: usize) {
    consume(2, token);
}
unsafe extern "C" fn completed(token: usize) -> i32 {
    consume(3, token)
}
unsafe extern "C" fn next() -> usize {
    producer(4, &[], &[])
}
unsafe extern "C" fn seconds(seconds: f32) -> usize {
    producer(5, &[0], &[(0, seconds.to_bits() as u64)])
}
unsafe extern "C" fn fixed() -> usize {
    producer(6, &[], &[])
}
unsafe extern "C" fn end() -> usize {
    producer(7, &[], &[])
}
unsafe extern "C" fn from_operation(pointer: usize) -> usize {
    let pointer = checked(|| engine_objects::engine_pointer(pointer));
    producer(8, &[pointer], &[])
}
unsafe extern "C" fn wireup() {
    checked(|| {
        let b = BINDING.get().ok_or("awaitable adapters absent")?;
        callbacks::guest_arguments(b.targets[9], &[])?;
        Ok(())
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn released_awaitable_identity_cannot_be_revived_by_engine_address_reuse() {
        let mut registry = Registry::default();
        assert_eq!(registry.insert(0).unwrap(), 0);
        let first = registry.insert(0x1230).unwrap();
        assert!(registry.pointer(0x1230).is_err());
        registry.records.remove(&first);
        let next = registry.insert(0x1230).unwrap();
        assert_ne!(first, next);
        assert!(registry.pointer(first).is_err());
        assert_eq!(registry.pointer(next).unwrap(), 0x1230);
    }
}
