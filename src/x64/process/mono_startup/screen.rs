//! Exact Screen value ABIs, copied into private guest slots for Unity calls.
use super::{bindings, callbacks, managed_calls, value_layout, Object};
use std::{ptr, sync::OnceLock};
static TARGETS: OnceLock<[u64; 2]> = OnceLock::new();
pub(super) fn installed() -> bool {
    TARGETS.get().is_some()
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
                return Err(concat!("missing Screen API: ", $name).into());
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
    let class =
        |name: &std::ffi::CStr| unsafe { find(core, c"UnityEngine".as_ptr(), name.as_ptr()) };
    let owner = class(c"Screen");
    let refresh = class(c"RefreshRate");
    let resolution = class(c"Resolution");
    let mode = class(c"FullScreenMode");
    let layouts = value_layout::Api::load(library, core)?;
    if layouts.layout(refresh)?.scalar_inline()? != (8, 4)
        || layouts.layout(resolution)?.scalar_inline()? != (16, 4)
    {
        return Err("Screen RefreshRate/Resolution layout mismatch".into());
    }
    let void = unsafe { find(corlib(), c"System".as_ptr(), c"Void".as_ptr()) };
    let int32 = unsafe { find(corlib(), c"System".as_ptr(), c"Int32".as_ptr()) };
    let api = managed_calls::Api::load(library)?;
    api.fixed_typed_icall(
        unsafe { method(owner, c"SetResolution_Injected".as_ptr(), 4) },
        owner,
        &[
            (1, void, false),
            (8, int32, false),
            (8, int32, false),
            (0x11, mode, false),
            (0x11, refresh, true),
        ],
        images,
    )?;
    api.fixed_typed_icall(
        unsafe { method(owner, c"get_currentResolution_Injected".as_ptr(), 1) },
        owner,
        &[(1, void, false), (0x11, resolution, true)],
        images,
    )?;
    let targets = [
        registry.exact_target("UnityEngine.Screen::SetResolution_Injected", 0x18007ceb0)?,
        registry.exact_target(
            "UnityEngine.Screen::get_currentResolution_Injected",
            0x18007cca0,
        )?,
    ];
    TARGETS
        .set(targets)
        .map_err(|_| "Screen adapters already installed")?;
    unsafe {
        add(
            c"UnityEngine.Screen::SetResolution_Injected".as_ptr(),
            set_resolution as *const () as Object,
        );
        add(
            c"UnityEngine.Screen::get_currentResolution_Injected".as_ptr(),
            current_resolution as *const () as Object,
        );
    }
    Ok(())
}
fn checked(work: impl FnOnce() -> Result<(), String>) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(Ok(())) => (),
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_SCREEN_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_SCREEN_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
unsafe extern "C" fn set_resolution(width: i32, height: i32, mode: i32, refresh: *const u32) {
    checked(|| {
        if refresh.is_null() || !(refresh as usize).is_multiple_of(4) || !(0..=3).contains(&mode) {
            return Err("Screen resolution argument shape mismatch".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(refresh.cast::<u8>(), 8) }.to_vec();
        callbacks::guest_byte_arguments(
            TARGETS.get().ok_or("Screen adapters absent")?[0],
            &[width as u32 as u64, height as u32 as u64, mode as u64, 0],
            &[(3, &bytes)],
        )?;
        eprintln!("NATIVE_MONO_SCREEN_RESOLUTION: width={width}; height={height}; mode={mode}; original-engine=true");
        Ok(())
    });
}
unsafe extern "C" fn current_resolution(output: *mut u32) {
    checked(|| {
        if output.is_null() || !(output as usize).is_multiple_of(4) {
            return Err("Screen resolution output shape mismatch".into());
        }
        let (_, result) = callbacks::guest_byte_arguments(
            TARGETS.get().ok_or("Screen adapters absent")?[1],
            &[0],
            &[(0, &[0; 16])],
        )?;
        unsafe { ptr::copy_nonoverlapping(result[0].as_ptr(), output.cast::<u8>(), 16) };
        Ok(())
    });
}
