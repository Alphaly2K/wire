//! Fixed Type/Boolean factory returning an actual native v2 handle to managed
//! Unmarshal. Guest handle cells and native GC slots stay separate identities.
use super::{callbacks, managed_calls, Object};
use std::{cell::Cell, sync::OnceLock};
pub(super) const NAME: &str =
    "UnityEngine.ScriptableObject::CreateScriptableObjectInstanceFromType_Injected";
pub(super) const TARGET: u64 = 0x1800c2260;
struct Binding {
    library: usize,
    target: u64,
    class: usize,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
thread_local! { static CALLS: Cell<usize> = const { Cell::new(0) }; }
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn install(library: Object, core: Object, target: u64) -> Result<(), String> {
    if core.is_null() || target != TARGET {
        return Err("ScriptableObject factory identity mismatch".into());
    }
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing ScriptableObject factory API: ", $name).into());
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
    let class = unsafe { find(core, c"UnityEngine".as_ptr(), c"ScriptableObject".as_ptr()) };
    let ty = unsafe { find(corlib(), c"System".as_ptr(), c"Type".as_ptr()) };
    if class.is_null() || ty.is_null() {
        return Err("ScriptableObject factory ABI classes absent".into());
    }
    let method = unsafe {
        method(
            class,
            c"CreateScriptableObjectInstanceFromType_Injected".as_ptr(),
            2,
        )
    };
    managed_calls::Api::load(library)?.type_bool_intptr_icall(method, class, ty)?;
    BINDING
        .set(Binding {
            library: library as usize,
            target,
            class: class as usize,
        })
        .map_err(|_| "ScriptableObject factory already installed")?;
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    unsafe {
        add(
            c"UnityEngine.ScriptableObject::CreateScriptableObjectInstanceFromType_Injected"
                .as_ptr(),
            create as *const () as Object,
        );
    }
    Ok(())
}
unsafe extern "C" fn create(ty: Object, defaults: u8) -> usize {
    let result = std::panic::catch_unwind(|| -> Result<usize, String> {
        let binding = BINDING
            .get()
            .ok_or("ScriptableObject factory not installed")?;
        if ty.is_null() || defaults > 1 {
            return Err("ScriptableObject factory requires Type and canonical Boolean".into());
        }
        let ty =
            callbacks::with_metadata_service(|route, memory| route.root_reflection(ty, memory))?;
        let handle = callbacks::guest_arguments(binding.target, &[ty, u64::from(defaults)])?;
        if handle == 0 {
            return Ok(0);
        }
        let native = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            // Original engine result is a GC slot, not a MonoObject pointer.
            let roots = route
                .objects
                .as_ref()
                .ok_or("factory handle has no root service")?;
            let token = roots.handles.1.target(roots.handles.0, handle)?;
            let object = route.rooted_native(token)?;
            managed_calls::Api::load(binding.library as Object)?.compatible(
                object,
                managed_calls::Shape::Reference {
                    class: binding.class as Object,
                    string: false,
                },
            )?;
            route.native_handle(handle)
        })?;
        CALLS.with(|calls| calls.set(calls.get() + 1));
        Ok(native as usize)
    });
    match result {
        Ok(Ok(handle)) => handle,
        other => {
            eprintln!("NATIVE_MONO_SCRIPTABLE_FACTORY_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
pub(super) fn report() -> String {
    format!(
        "native-scriptable-factory-calls={}\n",
        CALLS.with(Cell::get)
    )
}
