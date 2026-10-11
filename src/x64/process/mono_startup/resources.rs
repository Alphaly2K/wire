//! Fixed Type -> Object[] Resources binding. Engine code executes in the guest;
//! only rooted identities cross into the original managed IL implementation.
use super::{callbacks, managed_calls, Object};
use std::{cell::Cell, sync::OnceLock};
pub(super) const NAME: &str = "UnityEngine.ResourcesAPIInternal::FindObjectsOfTypeAll";
pub(super) const TARGET: u64 = 0x1800b84a0;
pub(super) const FIND_NAME: &str = "UnityEngine.Object::FindObjectsByType";
pub(super) const FIND_TARGET: u64 = 0x1800c3580;
struct Binding {
    target: u64,
    find_target: u64,
    object_array: usize,
    library: usize,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
    static FIND_CALLS: Cell<usize> = const { Cell::new(0) };
}
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn install(library: Object, core: Object, targets: [u64; 2]) -> Result<(), String> {
    if targets != [TARGET, FIND_TARGET] || core.is_null() {
        return Err("Resources binding identity mismatch".into());
    }
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing Resources API: ", $name).into());
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
    let array = sym!(
        "mono_array_class_get",
        unsafe extern "C" fn(Object, u32) -> Object
    );
    let owner = unsafe {
        find(
            core,
            c"UnityEngine".as_ptr(),
            c"ResourcesAPIInternal".as_ptr(),
        )
    };
    let object = unsafe { find(core, c"UnityEngine".as_ptr(), c"Object".as_ptr()) };
    let ty = unsafe { find(corlib(), c"System".as_ptr(), c"Type".as_ptr()) };
    if owner.is_null() || object.is_null() || ty.is_null() {
        return Err("Resources ABI classes absent".into());
    }
    let object_array = unsafe { array(object, 1) };
    let resource_method = unsafe { method(owner, c"FindObjectsOfTypeAll".as_ptr(), 1) };
    let api = managed_calls::Api::load(library)?;
    api.reference_icall(resource_method, owner, ty, object_array)?;
    let find_method = unsafe { method(object, c"FindObjectsByType".as_ptr(), 3) };
    let inactive = unsafe {
        find(
            core,
            c"UnityEngine".as_ptr(),
            c"FindObjectsInactive".as_ptr(),
        )
    };
    let sort = unsafe {
        find(
            core,
            c"UnityEngine".as_ptr(),
            c"FindObjectsSortMode".as_ptr(),
        )
    };
    api.find_objects_icall(find_method, object, [ty, inactive, sort], object_array)?;
    BINDING
        .set(Binding {
            target: targets[0],
            find_target: targets[1],
            object_array: object_array as usize,
            library: library as usize,
        })
        .map_err(|_| "Resources binding already installed")?;
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    unsafe {
        add(
            c"UnityEngine.ResourcesAPIInternal::FindObjectsOfTypeAll".as_ptr(),
            find_objects as *const () as Object,
        );
        add(
            c"UnityEngine.Object::FindObjectsByType".as_ptr(),
            find_by_type as *const () as Object,
        );
    }
    Ok(())
}
fn find_result(ty: Object, options: Option<(i32, i32)>) -> Result<Object, String> {
    let binding = BINDING.get().ok_or("Resources binding not installed")?;
    if ty.is_null() {
        return Err("Resources Type argument is NULL".into());
    }
    let token =
        callbacks::with_metadata_service(|route, memory| route.root_reflection(ty, memory))?;
    let result = if let Some((inactive, sort)) = options {
        if !(0..=1).contains(&inactive) || !(0..=1).contains(&sort) {
            return Err("object query enum value exceeds the frozen profile".into());
        }
        callbacks::guest_arguments(binding.find_target, &[token, inactive as u64, sort as u64])?
    } else {
        callbacks::guest_arguments(binding.target, &[token])?
    };
    let native = callbacks::with_metadata_service(|route, memory| {
        route.sync_guest(memory)?;
        route.rooted_native(result)
    })?;
    // Preserve actual array identity and CLR covariance, including empty arrays.
    managed_calls::Api::load(binding.library as Object)?.compatible(
        native,
        managed_calls::Shape::Reference {
            class: binding.object_array as Object,
            string: false,
        },
    )?;
    if native.is_null() {
        return Err("object query did not return a rooted array".into());
    }
    if options.is_some() {
        FIND_CALLS.with(|calls| calls.set(calls.get() + 1));
        eprintln!("NATIVE_MONO_OBJECT_QUERY_VERIFIED: find-by-type=true; actual-rooted-array=true");
    } else {
        CALLS.with(|calls| calls.set(calls.get() + 1));
    }
    Ok(native)
}
fn checked(ty: Object, options: Option<(i32, i32)>) -> Object {
    match std::panic::catch_unwind(|| find_result(ty, options)) {
        Ok(Ok(native)) => native,
        other => {
            eprintln!("NATIVE_MONO_RESOURCES_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
unsafe extern "C" fn find_objects(ty: Object) -> Object {
    checked(ty, None)
}
unsafe extern "C" fn find_by_type(ty: Object, inactive: i32, sort: i32) -> Object {
    checked(ty, Some((inactive, sort)))
}
pub(super) fn report() -> String {
    format!(
        "native-resources-find-calls={}\nnative-object-find-by-type-calls={}\n",
        CALLS.with(Cell::get),
        FIND_CALLS.with(Cell::get)
    )
}
