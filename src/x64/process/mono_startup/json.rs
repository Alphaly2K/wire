//! Original Unity JSON deserialization through an owned UTF16 span and typed
//! references. The engine's serializer remains responsible for its semantics.
use super::{callbacks, logging, managed_calls, Object};
use std::{cell::Cell, sync::OnceLock};
pub(super) const NAME: &str = "UnityEngine.JsonUtility::FromJsonInternal_Injected";
pub(super) const TARGET: u64 = 0x180132470;
struct Binding {
    library: usize,
    target: u64,
    object: usize,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
thread_local! { static CALLS: Cell<usize> = const { Cell::new(0) }; }
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn install(
    library: Object,
    core: Object,
    images: &[Object],
    target: u64,
) -> Result<(), String> {
    if core.is_null() || target != TARGET {
        return Err("JSON binding identity mismatch".into());
    }
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing JSON API: ", $name).into());
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
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let mut owners: Vec<_> = images
        .iter()
        .map(|&image| unsafe { find(image, c"UnityEngine".as_ptr(), c"JsonUtility".as_ptr()) })
        .filter(|p| !p.is_null())
        .collect();
    owners.sort_unstable_by_key(|p| *p as usize);
    owners.dedup();
    if owners.len() != 1 {
        return Err("JSON binding class is not a unique owned identity".into());
    }
    let owner = owners[0];
    let span = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"ManagedSpanWrapper".as_ptr(),
        )
    };
    let corlib = unsafe { corlib() };
    let object = unsafe { find(corlib, c"System".as_ptr(), c"Object".as_ptr()) };
    let ty = unsafe { find(corlib, c"System".as_ptr(), c"Type".as_ptr()) };
    let method = unsafe { method(owner, c"FromJsonInternal_Injected".as_ptr(), 3) };
    managed_calls::Api::load(library)?.json_from_icall(method, owner, span, object, ty)?;
    logging::validate_span_layout(library, span)?;
    BINDING
        .set(Binding {
            library: library as usize,
            target,
            object: object as usize,
        })
        .map_err(|_| "JSON binding already installed")?;
    unsafe {
        add(
            c"UnityEngine.JsonUtility::FromJsonInternal_Injected".as_ptr(),
            from_json as *const () as Object,
        );
    }
    Ok(())
}
unsafe extern "C" fn from_json(span: *const logging::Span, existing: Object, ty: Object) -> Object {
    let result = std::panic::catch_unwind(|| -> Result<Object, String> {
        let binding = BINDING.get().ok_or("JSON binding not installed")?;
        let units = unsafe { logging::owned_span(span) }?;
        let (existing, ty) = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            Ok((
                route.root_reference(existing, memory)?,
                route.root_reflection(ty, memory)?,
            ))
        })?;
        let token =
            callbacks::guest_string_span(binding.target, units.as_deref(), &[existing, ty])?;
        let native = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            route.rooted_native(token)
        })?;
        managed_calls::Api::load(binding.library as Object)?.compatible(
            native,
            managed_calls::Shape::Reference {
                class: binding.object as Object,
                string: false,
            },
        )?;
        CALLS.with(|calls| calls.set(calls.get() + 1));
        eprintln!(
            "NATIVE_MONO_JSON_DESERIALIZE_VERIFIED: utf16-units={}; rooted-result={token:#x}",
            units.as_ref().map_or(0, Vec::len)
        );
        Ok(native)
    });
    match result {
        Ok(Ok(native)) => native,
        other => {
            eprintln!("NATIVE_MONO_JSON_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
pub(super) fn report() -> String {
    format!("native-json-deserialize-calls={}\n", CALLS.with(Cell::get))
}
