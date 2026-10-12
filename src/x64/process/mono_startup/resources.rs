//! Fixed Type -> Object[] Resources binding. Engine code executes in the guest;
//! only rooted identities cross into the original managed IL implementation.
use super::{bindings, callbacks, engine_objects, logging, managed_calls, Object};
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
    loads: [u64; 7],
    classes: [usize; 2],
}
static BINDING: OnceLock<Binding> = OnceLock::new();
thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
    static FIND_CALLS: Cell<usize> = const { Cell::new(0) };
}
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn install(
    library: Object,
    core: Object,
    targets: [u64; 2],
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
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
    let intptr = unsafe { find(corlib(), c"System".as_ptr(), c"IntPtr".as_ptr()) };
    let void = unsafe { find(corlib(), c"System".as_ptr(), c"Void".as_ptr()) };
    let span = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"ManagedSpanWrapper".as_ptr(),
        )
    };
    let resource_owner = unsafe { find(core, c"UnityEngine".as_ptr(), c"Resources".as_ptr()) };
    let shader = unsafe { find(core, c"UnityEngine".as_ptr(), c"Shader".as_ptr()) };
    let request = unsafe { find(core, c"UnityEngine".as_ptr(), c"ResourceRequest".as_ptr()) };
    let asynchronous = unsafe { find(core, c"UnityEngine".as_ptr(), c"AsyncOperation".as_ptr()) };
    if [shader, request, asynchronous].iter().any(|c| c.is_null()) {
        return Err("resource result classes absent".into());
    }
    logging::validate_span_layout(library, span)?;
    let handle = (0x18, intptr, false);
    let span = (0x11, span, true);
    let reflection = (0x12, ty, false);
    let signatures = [
        vec![handle, span],
        vec![handle, span, reflection],
        vec![(0x1d, object_array, false), span, reflection],
        vec![handle, span, reflection],
        vec![(1, void, false), handle],
        vec![handle, reflection, span],
        vec![handle],
    ];
    let mut loads = [0; 7];
    for (index, ((name, member, target, builtin), signature)) in
        LOADS.iter().zip(&signatures).enumerate()
    {
        let class = if *builtin { resource_owner } else { owner };
        api.fixed_typed_icall(
            unsafe { method(class, member.as_ptr(), (signature.len() - 1) as i32) },
            class,
            signature,
            images,
        )?;
        loads[index] = registry.exact_target(name.to_str().map_err(|e| e.to_string())?, *target)?;
    }
    BINDING
        .set(Binding {
            target: targets[0],
            find_target: targets[1],
            object_array: object_array as usize,
            library: library as usize,
            loads,
            classes: [object as usize, shader as usize],
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
    for ((name, ..), adapter) in LOADS.iter().zip([
        load_shader as *const (),
        load as *const (),
        load_all as *const (),
        load_async as *const (),
        unload_asset as *const (),
        builtin as *const (),
        unload_unused as *const (),
    ]) {
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    Ok(())
}
const LOADS: [(&std::ffi::CStr, &std::ffi::CStr, u64, bool); 7] = [
    (
        c"UnityEngine.ResourcesAPIInternal::FindShaderByName_Injected",
        c"FindShaderByName_Injected",
        0x1800b8500,
        false,
    ),
    (
        c"UnityEngine.ResourcesAPIInternal::Load_Injected",
        c"Load_Injected",
        0x1800b88f0,
        false,
    ),
    (
        c"UnityEngine.ResourcesAPIInternal::LoadAll_Injected",
        c"LoadAll_Injected",
        0x1800b8d40,
        false,
    ),
    (
        c"UnityEngine.ResourcesAPIInternal::LoadAsyncInternal_Injected",
        c"LoadAsyncInternal_Injected",
        0x1800b9160,
        false,
    ),
    (
        c"UnityEngine.ResourcesAPIInternal::UnloadAsset_Injected",
        c"UnloadAsset_Injected",
        0x1800b9510,
        false,
    ),
    (
        c"UnityEngine.Resources::GetBuiltinResource_Injected",
        c"GetBuiltinResource_Injected",
        0x1800b97f0,
        true,
    ),
    (
        c"UnityEngine.Resources::UnloadUnusedAssets_Injected",
        c"UnloadUnusedAssets_Injected",
        0x1800b9c20,
        true,
    ),
];
fn operation<T>(work: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_RESOURCE_LOAD_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_RESOURCE_LOAD_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
unsafe fn load_result(
    index: usize,
    span: *const logging::Span,
    ty: Object,
) -> Result<usize, String> {
    let b = BINDING.get().ok_or("resource loading absent")?;
    let units = unsafe { logging::owned_span(span)? };
    let token = if index == 0 {
        0
    } else {
        callbacks::with_metadata_service(|route, memory| route.root_reflection(ty, memory))?
    };
    let result = if index == 0 {
        callbacks::guest_string_span(b.loads[index], units.as_deref(), &[])?
    } else if index == 5 {
        callbacks::guest_positioned_string_span(b.loads[index], units.as_deref(), &[token], &[])?
    } else {
        callbacks::guest_string_span(b.loads[index], units.as_deref(), &[token])?
    };
    let native = if index == 3 {
        // ResourceRequest's actual marshaller constructs AsyncOperation(ptr).
        // This is an opaque original engine pointer, not a GC handle.
        if result == 0 {
            0
        } else {
            engine_objects::engine_pointer(result as usize)? as usize
        }
    } else if index == 2 {
        let native = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            route.rooted_native(result)
        })?;
        managed_calls::Api::load(b.library as Object)?.compatible(
            native,
            managed_calls::Shape::Reference {
                class: b.object_array as Object,
                string: false,
            },
        )?;
        if native.is_null() {
            return Err("LoadAll returned NULL instead of owned array".into());
        }
        native as usize
    } else {
        let class = match index {
            0 => b.classes[1],
            _ => b.classes[0],
        };
        engine_objects::engine_handle(b.library as Object, class as Object, result)?
    };
    eprintln!(
        "NATIVE_MONO_RESOURCE_LOAD: member={:?}; actual-engine-result={result:#x}",
        LOADS[index].0
    );
    Ok(native)
}
unsafe extern "C" fn load_shader(span: *const logging::Span) -> usize {
    operation(|| unsafe { load_result(0, span, std::ptr::null_mut()) })
}
unsafe extern "C" fn load(span: *const logging::Span, ty: Object) -> usize {
    operation(|| unsafe { load_result(1, span, ty) })
}
unsafe extern "C" fn load_all(span: *const logging::Span, ty: Object) -> Object {
    operation(|| unsafe { load_result(2, span, ty) }) as Object
}
unsafe extern "C" fn load_async(span: *const logging::Span, ty: Object) -> usize {
    operation(|| unsafe { load_result(3, span, ty) })
}
unsafe extern "C" fn unload_asset(pointer: usize) {
    operation(|| {
        let b = BINDING.get().ok_or("resource unloading absent")?;
        let pointer = if pointer == 0 {
            0
        } else {
            engine_objects::engine_pointer(pointer)?
        };
        callbacks::guest_arguments(b.loads[4], &[pointer])?;
        Ok(())
    });
}
unsafe extern "C" fn builtin(ty: Object, span: *const logging::Span) -> usize {
    operation(|| unsafe { load_result(5, span, ty) })
}
unsafe extern "C" fn unload_unused() -> usize {
    operation(|| {
        let b = BINDING.get().ok_or("resource unloading absent")?;
        let handle = callbacks::guest_arguments(b.loads[6], &[])?;
        if handle == 0 {
            Ok(0)
        } else {
            engine_objects::engine_pointer(handle as usize).map(|p| p as usize)
        }
    })
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
