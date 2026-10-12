//! Fixed Shader adapters use original property IDs and global engine state.
//! Buffer tokens are resolved privately; vectors/matrices cross as owned bytes.
use super::{bindings, callbacks, gpu_buffers, logging, managed_calls, value_layout, Object};
use std::{
    cell::Cell,
    ffi::{CStr, CString},
    ptr,
    sync::OnceLock,
};
const COUNT: usize = 16;
const SPECS: [(&CStr, u64); COUNT] = [
    (c"PropertyToID_Injected", 0x180093340),
    (c"EnableKeyword_Injected", 0x180091650),
    (c"DisableKeyword_Injected", 0x180091a80),
    (c"IsKeywordEnabled_Injected", 0x180091ed0),
    (c"SetGlobalIntImpl", 0x1800938a0),
    (c"SetGlobalFloatImpl", 0x180093920),
    (c"GetGlobalIntImpl", 0x180093be0),
    (c"GetGlobalFloatImpl", 0x180093c30),
    (c"SetGlobalVectorImpl_Injected", 0x180093940),
    (c"SetGlobalMatrixImpl_Injected", 0x1800939b0),
    (c"GetGlobalVectorImpl_Injected", 0x180093c80),
    (c"GetGlobalMatrixImpl_Injected", 0x180093d30),
    (c"SetGlobalBufferImpl_Injected", 0x180093b40),
    (c"SetGlobalGraphicsBufferImpl_Injected", 0x180093b40),
    (c"SetGlobalConstantBufferImpl_Injected", 0x180093b70),
    (c"SetGlobalConstantGraphicsBufferImpl_Injected", 0x180093b70),
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
                return Err(concat!("missing Shader API: ", $name).into());
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
    let system = |name: &CStr| unsafe { find(corlib, c"System".as_ptr(), name.as_ptr()) };
    let unity = |name: &CStr| unsafe { find(core, c"UnityEngine".as_ptr(), name.as_ptr()) };
    let owner = unity(c"Shader");
    let span_class = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"ManagedSpanWrapper".as_ptr(),
        )
    };
    logging::validate_span_layout(library, span_class)?;
    let vector_class = unity(c"Vector4");
    let matrix_class = unity(c"Matrix4x4");
    let layouts = value_layout::Api::load(library, core)?;
    if layouts.layout(vector_class)?.scalar_inline()? != (16, 4)
        || layouts.layout(matrix_class)?.scalar_inline()? != (64, 4)
    {
        return Err("Shader Vector4/Matrix4x4 layout mismatch".into());
    }
    let void = (1, system(c"Void"), false);
    let int32 = (8, system(c"Int32"), false);
    let single = (0xc, system(c"Single"), false);
    let intptr = (0x18, system(c"IntPtr"), false);
    let boolean = (2, system(c"Boolean"), false);
    let span = (0x11, span_class, true);
    let vector = (0x11, vector_class, true);
    let matrix = (0x11, matrix_class, true);
    let signatures = [
        vec![int32, span],
        vec![void, span],
        vec![void, span],
        vec![boolean, span],
        vec![void, int32, int32],
        vec![void, int32, single],
        vec![int32, int32],
        vec![single, int32],
        vec![void, int32, vector],
        vec![void, int32, matrix],
        vec![void, int32, vector],
        vec![void, int32, matrix],
        vec![void, int32, intptr],
        vec![void, int32, intptr],
        vec![void, int32, intptr, int32, int32],
        vec![void, int32, intptr, int32, int32],
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
            &format!("UnityEngine.Shader::{}", member.to_str().unwrap()),
            *target,
        )?;
    }
    TARGETS
        .set(targets)
        .map_err(|_| "Shader adapters already installed")?;
    let adapters = [
        property_id as *const (),
        enable as *const (),
        disable as *const (),
        enabled as *const (),
        set_int as *const (),
        set_float as *const (),
        get_int as *const (),
        get_float as *const (),
        set_vector as *const (),
        set_matrix as *const (),
        get_vector as *const (),
        get_matrix as *const (),
        set_buffer as *const (),
        set_graphics_buffer as *const (),
        set_constant_buffer as *const (),
        set_constant_graphics_buffer as *const (),
    ];
    for ((member, _), adapter) in SPECS.iter().zip(adapters) {
        let name =
            CString::new(format!("UnityEngine.Shader::{}", member.to_str().unwrap())).unwrap();
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    Ok(())
}
fn checked<T>(work: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_SHADER_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_SHADER_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
fn target(index: usize) -> Result<u64, String> {
    let pc = TARGETS.get().ok_or("Shader adapters absent")?[index];
    CALLS.with(|slot| {
        let mut calls = slot.get();
        if calls[index] == 0 {
            eprintln!(
                "NATIVE_MONO_SHADER: member={:?}; original-engine=true",
                SPECS[index].0
            );
        }
        calls[index] += 1;
        slot.set(calls);
    });
    Ok(pc)
}
unsafe fn string(index: usize, span: *const logging::Span) -> u64 {
    checked(|| {
        let units = unsafe { logging::owned_span(span)? };
        let result = callbacks::guest_string_span(target(index)?, units.as_deref(), &[])?;
        if index == 3 && result & 255 > 1 {
            return Err("Shader keyword Boolean is not canonical".into());
        }
        Ok(result)
    })
}
unsafe extern "C" fn property_id(span: *const logging::Span) -> i32 {
    unsafe { string(0, span) as i32 }
}
unsafe extern "C" fn enable(span: *const logging::Span) {
    unsafe {
        string(1, span);
    }
}
unsafe extern "C" fn disable(span: *const logging::Span) {
    unsafe {
        string(2, span);
    }
}
unsafe extern "C" fn enabled(span: *const logging::Span) -> u8 {
    unsafe { string(3, span) as u8 }
}
unsafe extern "C" fn set_int(name: i32, value: i32) {
    checked(|| callbacks::guest_arguments(target(4)?, &[name as u32 as u64, value as u32 as u64]));
}
unsafe extern "C" fn set_float(name: i32, value: f32) {
    checked(|| {
        callbacks::guest_scalars(
            target(5)?,
            &[name as u32 as u64, 0],
            &[(1, value.to_bits() as u64)],
        )
    });
}
unsafe extern "C" fn get_int(name: i32) -> i32 {
    checked(|| Ok(callbacks::guest_arguments(target(6)?, &[name as u32 as u64])? as i32))
}
unsafe extern "C" fn get_float(name: i32) -> f32 {
    checked(|| {
        Ok(f32::from_bits(
            callbacks::guest_scalars(target(7)?, &[name as u32 as u64], &[])?.1 as u32,
        ))
    })
}
unsafe fn value(index: usize, name: i32, pointer: *mut u8, bytes: usize, output: bool) {
    checked(|| {
        if pointer.is_null() || !(pointer as usize).is_multiple_of(4) {
            return Err("Shader value pointer/alignment mismatch".into());
        }
        let payload = if output {
            vec![0; bytes]
        } else {
            unsafe { std::slice::from_raw_parts(pointer, bytes) }.to_vec()
        };
        let (_, result) = callbacks::guest_byte_arguments(
            target(index)?,
            &[name as u32 as u64, 0],
            &[(1, &payload)],
        )?;
        if output {
            unsafe { ptr::copy_nonoverlapping(result[0].as_ptr(), pointer, bytes) };
        }
        Ok(())
    });
}
unsafe extern "C" fn set_vector(name: i32, pointer: *const u8) {
    unsafe { value(8, name, pointer.cast_mut(), 16, false) }
}
unsafe extern "C" fn set_matrix(name: i32, pointer: *const u8) {
    unsafe { value(9, name, pointer.cast_mut(), 64, false) }
}
unsafe extern "C" fn get_vector(name: i32, pointer: *mut u8) {
    unsafe { value(10, name, pointer, 16, true) }
}
unsafe extern "C" fn get_matrix(name: i32, pointer: *mut u8) {
    unsafe { value(11, name, pointer, 64, true) }
}
fn buffer(index: usize, kind: usize, name: i32, token: usize, range: Option<(i32, i32)>) {
    checked(|| {
        let guest = gpu_buffers::binding_pointer(token, kind, range)?;
        let mut args = vec![name as u32 as u64, guest];
        if let Some((offset, size)) = range {
            args.extend([offset as u32 as u64, size as u32 as u64]);
        }
        callbacks::guest_arguments(target(index)?, &args)?;
        Ok(())
    });
}
unsafe extern "C" fn set_buffer(name: i32, token: usize) {
    buffer(12, 0, name, token, None)
}
unsafe extern "C" fn set_graphics_buffer(name: i32, token: usize) {
    buffer(13, 1, name, token, None)
}
unsafe extern "C" fn set_constant_buffer(name: i32, token: usize, offset: i32, size: i32) {
    buffer(14, 0, name, token, Some((offset, size)))
}
unsafe extern "C" fn set_constant_graphics_buffer(name: i32, token: usize, offset: i32, size: i32) {
    buffer(15, 1, name, token, Some((offset, size)))
}
