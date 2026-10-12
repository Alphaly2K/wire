//! Original Unity player-loop arrays, including managed Type/delegate roots.
//! Two exact engine address fields carry Windows image addresses as data only.
//! They cannot be numeric GPU payloads or general native pointer arguments.
use super::{bindings, callbacks, managed_calls, Object};
use std::{
    cell::Cell,
    collections::BTreeSet,
    sync::{Mutex, OnceLock},
};
struct Binding {
    library: usize,
    array: usize,
    targets: [u64; 3],
    fields: [(usize, usize, usize, bool); 4],
}
static BINDING: OnceLock<Binding> = OnceLock::new();
struct Addresses {
    bounds: Option<(u64, u64)>,
    issued: [BTreeSet<u64>; 2],
}
static ADDRESSES: Mutex<Addresses> = Mutex::new(Addresses {
    bounds: None,
    issued: [BTreeSet::new(), BTreeSet::new()],
});
thread_local! { static PRODUCING: Cell<bool> = const { Cell::new(false) }; }
pub(super) fn address_slot(class: Object, field: Object, kind: u32, offset: usize) -> Option<bool> {
    if kind != 0x18 {
        return None;
    }
    BINDING
        .get()?
        .fields
        .iter()
        .find_map(|&(owner, declared, at, condition)| {
            (owner == class as usize && declared == field as usize && offset == at)
                .then_some(condition)
        })
}
impl Addresses {
    fn check(&mut self, address: u64, condition: bool, producing: bool) -> Result<(), String> {
        if address == 0 || self.issued[usize::from(condition)].contains(&address) {
            return Ok(());
        }
        let (start, end) = self.bounds.ok_or("player-loop engine image is not owned")?;
        if !producing
            || address < start
            || address >= end
            || self.issued.iter().map(BTreeSet::len).sum::<usize>() >= 4096
        {
            return Err("player-loop address was not produced by the original engine".into());
        }
        self.issued[usize::from(condition)].insert(address);
        Ok(())
    }
}
pub(super) fn check_address(address: u64, condition: bool) -> Result<(), String> {
    if address == 0 {
        return Ok(());
    }
    let producing = PRODUCING.with(Cell::get);
    ADDRESSES
        .lock()
        .map_err(|_| "player-loop address registry poisoned")?
        .check(address, condition, producing)
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
                return Err(concat!("missing player-loop API: ", $name).into());
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
    let field = sym!(
        "mono_class_get_field_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char) -> Object
    );
    let offset = sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> i32);
    let parent = sym!(
        "mono_field_get_parent",
        unsafe extern "C" fn(Object) -> Object
    );
    let ty = sym!(
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let byref = sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32);
    let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
    let size = sym!(
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let array = sym!(
        "mono_array_class_get",
        unsafe extern "C" fn(Object, u32) -> Object
    );
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let owner = unsafe {
        find(
            core,
            c"UnityEngine.LowLevel".as_ptr(),
            c"PlayerLoop".as_ptr(),
        )
    };
    let internal = unsafe {
        find(
            core,
            c"UnityEngine.LowLevel".as_ptr(),
            c"PlayerLoopSystemInternal".as_ptr(),
        )
    };
    let system = unsafe {
        find(
            core,
            c"UnityEngine.LowLevel".as_ptr(),
            c"PlayerLoopSystem".as_ptr(),
        )
    };
    let void = unsafe { find(corlib(), c"System".as_ptr(), c"Void".as_ptr()) };
    let mut fields = [(0, 0, 0, false); 4];
    for (index, (class, at)) in [(internal, 16), (system, 24)].into_iter().enumerate() {
        let mut alignment = 0;
        if class.is_null() || unsafe { size(class, &mut alignment) } != 40 || alignment != 8 {
            return Err("player-loop value size/alignment changed".into());
        }
        for (column, (name, condition)) in
            [(c"updateFunction", false), (c"loopConditionFunction", true)]
                .into_iter()
                .enumerate()
        {
            let field = unsafe { field(class, name.as_ptr()) };
            let ty = if field.is_null() {
                std::ptr::null_mut()
            } else {
                unsafe { ty(field) }
            };
            let expected = at + column * 8;
            if field.is_null()
                || ty.is_null()
                || unsafe { parent(field) } != class
                || unsafe { offset(field) } != (16 + expected) as i32
                || unsafe { kind(ty) } != 0x18
                || unsafe { byref(ty) } != 0
                || unsafe { flags(field) } & (0x10 | 0x40) != 0
            {
                return Err("player-loop exact engine address declaration changed".into());
            }
            fields[index * 2 + column] = (class as usize, field as usize, expected, condition);
        }
    }
    let array = unsafe { array(internal, 1) };
    let api = managed_calls::Api::load(library)?;
    let specs = [
        (c"GetDefaultPlayerLoopInternal", 0x1800d4c50),
        (c"GetCurrentPlayerLoopInternal", 0x1800d4c80),
        (c"SetPlayerLoopInternal", 0x1800d4d50),
    ];
    let mut targets = [0; 3];
    for (index, (member, target)) in specs.iter().enumerate() {
        let signature = if index == 2 {
            vec![(1, void, false), (0x1d, array, false)]
        } else {
            vec![(0x1d, array, false)]
        };
        api.fixed_typed_icall(
            unsafe { method(owner, member.as_ptr(), (signature.len() - 1) as i32) },
            owner,
            &signature,
            images,
        )?;
        targets[index] = registry.exact_target(
            &format!(
                "UnityEngine.LowLevel.PlayerLoop::{}",
                member.to_str().unwrap()
            ),
            *target,
        )?;
    }
    BINDING
        .set(Binding {
            library: library as usize,
            array: array as usize,
            fields,
            targets,
        })
        .map_err(|_| "player-loop adapters already installed")?;
    for ((member, _), adapter) in specs.iter().zip([
        default_loop as *const (),
        current_loop as *const (),
        set_loop as *const (),
    ]) {
        let name = std::ffi::CString::new(format!(
            "UnityEngine.LowLevel.PlayerLoop::{}",
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
            eprintln!("NATIVE_MONO_PLAYER_LOOP_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_PLAYER_LOOP_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
fn get(index: usize) -> Object {
    checked(|| {
        let b = BINDING.get().ok_or("player-loop adapters absent")?;
        let bounds = callbacks::with_service(|_, runtime| {
            runtime
                .module_image_bounds("unityplayer.dll")
                .ok_or_else(|| "original UnityPlayer image absent".to_owned())
        })?;
        {
            let mut addresses = ADDRESSES
                .lock()
                .map_err(|_| "player-loop address registry poisoned")?;
            if addresses.bounds.is_some_and(|original| original != bounds) {
                return Err("player-loop image lifetime changed".into());
            }
            addresses.bounds = Some(bounds);
        }
        if PRODUCING.with(|flag| flag.replace(true)) {
            return Err("recursive player-loop producer".into());
        }
        struct Scope;
        impl Drop for Scope {
            fn drop(&mut self) {
                PRODUCING.with(|flag| flag.set(false));
            }
        }
        let _scope = Scope;
        let token = callbacks::guest_arguments(b.targets[index], &[])?;
        let native = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            route.rooted_native(token)
        })?;
        managed_calls::Api::load(b.library as Object)?.compatible(
            native,
            managed_calls::Shape::Reference {
                class: b.array as Object,
                string: false,
            },
        )?;
        callbacks::with_service(|memory, _| {
            let addresses = ADDRESSES
                .lock()
                .map_err(|_| "player-loop address registry poisoned")?;
            for &address in addresses.issued.iter().flatten() {
                memory.check(address, 1, crate::x64::memory::READ)?;
            }
            Ok::<_, String>(())
        })?;
        eprintln!(
            "NATIVE_MONO_PLAYER_LOOP_GET: current={}; actual-rooted-array=true",
            index == 1
        );
        Ok(native)
    })
}
unsafe extern "C" fn default_loop() -> Object {
    get(0)
}
unsafe extern "C" fn current_loop() -> Object {
    get(1)
}
unsafe extern "C" fn set_loop(native: Object) {
    checked(|| {
        let b = BINDING.get().ok_or("player-loop adapters absent")?;
        managed_calls::Api::load(b.library as Object)?.compatible(
            native,
            managed_calls::Shape::Reference {
                class: b.array as Object,
                string: false,
            },
        )?;
        if native.is_null() {
            return Err("player-loop replacement array is NULL".into());
        }
        let token = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            // The original setter reads each RuntimeType's metadata cell
            // directly. Publish actual typed children as well as array slots;
            // delegate references remain rooted opaque objects.
            route.root_container(native, memory)
        })?;
        callbacks::guest_arguments(b.targets[2], &[token])?;
        eprintln!("NATIVE_MONO_PLAYER_LOOP_SET: actual-engine=true; rooted-delegates=true");
        Ok(())
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loop_addresses_are_issued_only_by_producer_and_keep_their_field_family() {
        let mut addresses = Addresses {
            bounds: Some((0x180000000, 0x181000000)),
            issued: [BTreeSet::new(), BTreeSet::new()],
        };
        assert!(addresses.check(0x180001000, false, false).is_err());
        assert!(addresses.check(0x900001000, false, true).is_err());
        addresses.check(0x180001000, false, true).unwrap();
        addresses.check(0x180001000, false, false).unwrap();
        assert!(addresses.check(0x180001000, true, false).is_err());
        addresses.check(0, true, false).unwrap();
    }
}
