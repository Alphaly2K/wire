//! Fixed PlayerPrefs ABIs preserve Unity's real registry/storage behavior.
//! UTF16 inputs are copied; returned allocations use the shared string arena.
use super::{binding_strings, bindings, callbacks, logging, managed_calls, Object};
use std::{cell::Cell, ffi::CStr, sync::OnceLock};
const COUNT: usize = 10;
const SPECS: [(&CStr, u64); COUNT] = [
    (c"TrySetInt_Injected", 0x1800b4f90),
    (c"TrySetFloat_Injected", 0x1800b5360),
    (c"TrySetSetString_Injected", 0x1800b5760),
    (c"GetInt_Injected", 0x1800b5e50),
    (c"GetFloat_Injected", 0x1800b61f0),
    (c"GetString_Injected", 0x1800b6660),
    (c"HasKey_Injected", 0x1800b6d90),
    (c"DeleteKey_Injected", 0x1800b7130),
    (c"DeleteAll", 0x1800b7550),
    (c"Save", 0x18005fee0),
];
static TARGETS: OnceLock<[u64; COUNT]> = OnceLock::new();
thread_local! { static CALLS: Cell<[usize; COUNT]> = const { Cell::new([0; COUNT]) }; }
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
                return Err(concat!("missing PlayerPrefs API: ", $name).into());
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
    let corlib = unsafe { corlib() };
    let owner = unsafe { find(core, c"UnityEngine".as_ptr(), c"PlayerPrefs".as_ptr()) };
    let span = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"ManagedSpanWrapper".as_ptr(),
        )
    };
    logging::validate_span_layout(library, span)?;
    let ty = |name: &CStr| unsafe { find(corlib, c"System".as_ptr(), name.as_ptr()) };
    let void = (1, ty(c"Void"), false);
    let boolean = (2, ty(c"Boolean"), false);
    let int32 = (8, ty(c"Int32"), false);
    let single = (0xc, ty(c"Single"), false);
    let span = (0x11, span, true);
    let signatures = [
        vec![boolean, span, int32],
        vec![boolean, span, single],
        vec![boolean, span, span],
        vec![int32, span, int32],
        vec![single, span, single],
        vec![void, span, span, span],
        vec![boolean, span],
        vec![void, span],
        vec![void],
        vec![void],
    ];
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; COUNT];
    for (index, ((member, target), signature)) in SPECS.iter().zip(&signatures).enumerate() {
        api.fixed_typed_icall(
            unsafe { method(owner, member.as_ptr(), (signature.len() - 1) as i32) },
            owner,
            signature,
            images,
        )?;
        targets[index] = registry.exact_target(
            &format!(
                "UnityEngine.PlayerPrefs::{}",
                member.to_str().map_err(|e| e.to_string())?
            ),
            *target,
        )?;
    }
    TARGETS
        .set(targets)
        .map_err(|_| "PlayerPrefs adapters already installed")?;
    let adapters = [
        set_int as *const (),
        set_float as *const (),
        set_string as *const (),
        get_int as *const (),
        get_float as *const (),
        get_string as *const (),
        has_key as *const (),
        delete_key as *const (),
        delete_all as *const (),
        save as *const (),
    ];
    for ((member, _), adapter) in SPECS.iter().zip(adapters) {
        let name = std::ffi::CString::new(format!(
            "UnityEngine.PlayerPrefs::{}",
            member.to_str().unwrap()
        ))
        .unwrap();
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    Ok(())
}
fn checked<T>(operation: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_PLAYER_PREFS_REJECT: {error}");
            std::process::abort();
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_PLAYER_PREFS_REJECT: adapter panicked");
            std::process::abort();
        }
    }
}
fn call(
    index: usize,
    args: &[u64],
    floating: &[(usize, u64)],
    spans: &[(usize, Option<&[u16]>)],
    output: Option<usize>,
) -> Result<callbacks::GuestSpanResult, String> {
    let target = TARGETS.get().ok_or("PlayerPrefs adapters absent")?[index];
    let result = callbacks::guest_span_arguments(target, args, floating, spans, output)?;
    if matches!(index, 0 | 1 | 2 | 6) && result.0 .0 & 255 > 1 {
        return Err("PlayerPrefs returned noncanonical bool".into());
    }
    CALLS.with(|slot| {
        let mut calls = slot.get();
        if calls[index] == 0 {
            eprintln!("NATIVE_MONO_PLAYER_PREFS: member={:?}; original-engine=true; rax={:#x}; xmm0={:#x}", SPECS[index].0, result.0.0, result.0.1);
        }
        calls[index] += 1;
        slot.set(calls);
    });
    Ok(result)
}
unsafe extern "C" fn set_int(key: *const logging::Span, value: i32) -> u8 {
    checked(|| {
        let key = unsafe { logging::owned_span(key)? };
        Ok(call(
            0,
            &[0, value as u32 as u64],
            &[],
            &[(0, key.as_deref())],
            None,
        )?
        .0
         .0 as u8)
    })
}
unsafe extern "C" fn set_float(key: *const logging::Span, value: f32) -> u8 {
    checked(|| {
        let key = unsafe { logging::owned_span(key)? };
        Ok(call(
            1,
            &[0, 0],
            &[(1, value.to_bits() as u64)],
            &[(0, key.as_deref())],
            None,
        )?
        .0
         .0 as u8)
    })
}
unsafe extern "C" fn set_string(key: *const logging::Span, value: *const logging::Span) -> u8 {
    checked(|| {
        let key = unsafe { logging::owned_span(key)? };
        let value = unsafe { logging::owned_span(value)? };
        Ok(call(
            2,
            &[0, 0],
            &[],
            &[(0, key.as_deref()), (1, value.as_deref())],
            None,
        )?
        .0
         .0 as u8)
    })
}
unsafe extern "C" fn get_int(key: *const logging::Span, value: i32) -> i32 {
    checked(|| {
        let key = unsafe { logging::owned_span(key)? };
        Ok(call(
            3,
            &[0, value as u32 as u64],
            &[],
            &[(0, key.as_deref())],
            None,
        )?
        .0
         .0 as i32)
    })
}
unsafe extern "C" fn get_float(key: *const logging::Span, value: f32) -> f32 {
    checked(|| {
        let key = unsafe { logging::owned_span(key)? };
        Ok(f32::from_bits(
            call(
                4,
                &[0, 0],
                &[(1, value.to_bits() as u64)],
                &[(0, key.as_deref())],
                None,
            )?
            .0
             .1 as u32,
        ))
    })
}
unsafe extern "C" fn get_string(
    key: *const logging::Span,
    value: *const logging::Span,
    output: *mut logging::Span,
) {
    checked(|| {
        if output.is_null() || !(output as usize).is_multiple_of(8) {
            return Err("PlayerPrefs requires aligned output span".into());
        }
        let key = unsafe { logging::owned_span(key)? };
        let value = unsafe { logging::owned_span(value)? };
        let result = call(
            5,
            &[0; 3],
            &[],
            &[(0, key.as_deref()), (1, value.as_deref())],
            Some(2),
        )?;
        unsafe { binding_strings::publish(&result.1.ok_or("PlayerPrefs output absent")?, output)? };
        Ok(())
    });
}
unsafe extern "C" fn has_key(key: *const logging::Span) -> u8 {
    checked(|| {
        let key = unsafe { logging::owned_span(key)? };
        Ok(call(6, &[0], &[], &[(0, key.as_deref())], None)?.0 .0 as u8)
    })
}
unsafe extern "C" fn delete_key(key: *const logging::Span) {
    checked(|| {
        let key = unsafe { logging::owned_span(key)? };
        call(7, &[0], &[], &[(0, key.as_deref())], None)?;
        Ok(())
    });
}
unsafe extern "C" fn delete_all() {
    checked(|| call(8, &[], &[], &[], None).map(|_| ()));
}
unsafe extern "C" fn save() {
    checked(|| call(9, &[], &[], &[], None).map(|_| ()));
}
