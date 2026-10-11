//! Fixed compiled adapters for the locked subsystem scripting map and analytics.
use super::{c_char, callbacks, invoke, logging, Object};
use std::{cell::Cell, sync::OnceLock};
pub(super) const NAME: &str = "UnityEngine.SubsystemManager::StaticConstructScriptingClassMap";
pub(super) const TARGET: u64 = 0x180135070;
pub(super) const ANALYTICS_NAME: &str = "UnityEngine.SubsystemsImplementation.SubsystemDescriptorStore::ReportSingleSubsystemAnalytics_Injected";
pub(super) const ANALYTICS_TARGET: u64 = 0x1801350b0;
static ANALYTICS_PC: OnceLock<u64> = OnceLock::new();
thread_local! { static ANALYTICS_CALLS: Cell<usize> = const { Cell::new(0) }; }
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
    invoke::validate_classlib_init(library, method, image, invoke::SUBSYSTEM_MAP_ICALL_STEP)?;
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
pub(super) fn install_analytics(
    library: Object,
    image: Object,
    core: Object,
    target: u64,
) -> Result<(), String> {
    callbacks::with_service(|_, _| Ok(()))?;
    if target != ANALYTICS_TARGET {
        return Err("subsystem analytics target mismatch".into());
    }
    macro_rules! sym {
        ($name:literal,$ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing subsystem analytics symbol: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const c_char, i32) -> Object
    );
    let class = unsafe {
        find(
            image,
            c"UnityEngine.SubsystemsImplementation".as_ptr(),
            c"SubsystemDescriptorStore".as_ptr(),
        )
    };
    if class.is_null() {
        return Err("subsystem descriptor store class absent".into());
    }
    let method = unsafe {
        method(
            class,
            c"ReportSingleSubsystemAnalytics_Injected".as_ptr(),
            1,
        )
    };
    logging::validate_span_method(library, core, method)?;
    ANALYTICS_PC
        .set(target)
        .map_err(|_| "subsystem analytics adapter already installed")?;
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const c_char, Object)
    );
    unsafe {
        add(c"UnityEngine.SubsystemsImplementation.SubsystemDescriptorStore::ReportSingleSubsystemAnalytics_Injected".as_ptr(), analytics as *const () as Object);
    }
    Ok(())
}
unsafe extern "C" fn analytics(span: *const logging::Span) {
    let result = std::panic::catch_unwind(|| -> Result<(), String> {
        callbacks::with_service(|_, _| Ok(()))?;
        let units = unsafe { logging::owned_span(span) }?;
        callbacks::log_span(
            *ANALYTICS_PC
                .get()
                .ok_or("subsystem analytics adapter not installed")?,
            units.as_deref(),
        )?;
        ANALYTICS_CALLS.with(|calls| calls.set(calls.get() + 1));
        Ok(())
    });
    if !matches!(result, Ok(Ok(()))) {
        eprintln!("NATIVE_MONO_SUBSYSTEM_ANALYTICS_REJECT: {result:?}");
        std::process::abort();
    }
}
pub(super) fn report() -> String {
    format!(
        "native-subsystem-analytics-calls={}\n",
        ANALYTICS_CALLS.with(Cell::get)
    )
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
