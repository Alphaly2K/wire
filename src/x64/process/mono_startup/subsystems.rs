//! Fixed no-argument adapter for the locked subsystem scripting map.
use super::{c_char, callbacks, invoke, Object};
use std::{cell::Cell, sync::OnceLock};
pub(super) const NAME: &str = "UnityEngine.SubsystemManager::StaticConstructScriptingClassMap";
pub(super) const TARGET: u64 = 0x180135070;
static TARGET_PC: OnceLock<u64> = OnceLock::new();
thread_local! { static CALLS: Cell<usize> = const { Cell::new(0) }; }
pub(super) fn install(library: Object, image: Object, target: u64) -> Result<(), String> {
    if target != TARGET {
        return Err("subsystem map target mismatch".into());
    }
    macro_rules! sym {
        ($name:literal,$ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing subsystem map symbol: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let class = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const c_char, i32) -> Object
    );
    let class = unsafe { class(image, c"UnityEngine".as_ptr(), c"SubsystemManager".as_ptr()) };
    if class.is_null() {
        return Err("subsystem map class absent".into());
    }
    let method = unsafe { method(class, c"StaticConstructScriptingClassMap".as_ptr(), 0) };
    if method.is_null() {
        return Err("subsystem map method absent".into());
    }
    invoke::validate_classlib_init(library, method, image, 7)?;
    TARGET_PC
        .set(target)
        .map_err(|_| "subsystem map adapter already installed")?;
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const c_char, Object)
    );
    unsafe {
        add(
            c"UnityEngine.SubsystemManager::StaticConstructScriptingClassMap".as_ptr(),
            construct as *const () as Object,
        );
    }
    Ok(())
}
unsafe extern "C" fn construct() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        callbacks::guest_noargs(
            *TARGET_PC
                .get()
                .ok_or("subsystem map target not installed")?,
        )
    }));
    match result {
        Ok(Ok(())) => CALLS.with(|calls| calls.set(calls.get() + 1)),
        other => {
            eprintln!("NATIVE_MONO_SUBSYSTEM_MAP_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
pub(super) fn verify() -> Result<(), String> {
    if CALLS.with(|calls| calls.get()) != 1 {
        return Err("actual subsystem map callback count mismatch".into());
    }
    eprintln!("NATIVE_MONO_SUBSYSTEM_MAP_VERIFIED: original-guest-target=true actual-calls=1");
    Ok(())
}
