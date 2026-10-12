//! TextAsset byte arrays retain the actual engine allocation and Mono identity.
//! Raw native data views are a separate lifetime contract and are not exposed.
use super::{bindings, callbacks, engine_objects, managed_calls, Object};
use std::{ffi::CStr, sync::OnceLock};
struct Binding {
    targets: [u64; 3],
    library: usize,
    bytes: usize,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
const SPECS: [(&CStr, u64); 3] = [
    (c"get_bytes_Injected", 0x1800c2970),
    (c"GetPreviewBytes_Injected", 0x1800c2990),
    (c"GetDataSize_Injected", 0x1800c2e90),
];
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
                return Err(concat!("missing TextAsset API: ", $name).into());
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
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let corlib = unsafe { corlib() };
    let system = |name: &CStr| unsafe { find(corlib, c"System".as_ptr(), name.as_ptr()) };
    let owner = unsafe { find(core, c"UnityEngine".as_ptr(), c"TextAsset".as_ptr()) };
    let bytes_class = unsafe { array(system(c"Byte"), 1) };
    let bytes = (0x1d, bytes_class, false);
    let intptr = (0x18, system(c"IntPtr"), false);
    let int32 = (8, system(c"Int32"), false);
    let int64 = (0xa, system(c"Int64"), false);
    let signatures = [
        vec![bytes, intptr],
        vec![bytes, intptr, int32],
        vec![int64, intptr],
    ];
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; 3];
    for (index, ((member, target), signature)) in SPECS.iter().zip(&signatures).enumerate() {
        api.fixed_typed_icall(
            unsafe { method(owner, member.as_ptr(), (signature.len() - 1) as i32) },
            owner,
            signature,
            images,
        )?;
        targets[index] = registry.exact_target(
            &format!("UnityEngine.TextAsset::{}", member.to_str().unwrap()),
            *target,
        )?;
    }
    BINDING
        .set(Binding {
            targets,
            library: library as usize,
            bytes: bytes_class as usize,
        })
        .map_err(|_| "TextAsset adapters already installed")?;
    for ((member, _), adapter) in SPECS.iter().zip([
        get_bytes as *const (),
        preview as *const (),
        size as *const (),
    ]) {
        let name = std::ffi::CString::new(format!(
            "UnityEngine.TextAsset::{}",
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
            eprintln!("NATIVE_MONO_TEXT_ASSET_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_TEXT_ASSET_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
fn read_bytes(index: usize, pointer: usize, maximum: Option<i32>) -> Object {
    checked(|| {
        let b = BINDING.get().ok_or("TextAsset adapters absent")?;
        let mut args = vec![engine_objects::engine_pointer(pointer)?];
        if let Some(maximum) = maximum {
            args.push(maximum as u32 as u64);
        }
        let token = callbacks::guest_arguments(b.targets[index], &args)?;
        let native = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            route.rooted_native(token)
        })?;
        managed_calls::Api::load(b.library as Object)?.compatible(
            native,
            managed_calls::Shape::Reference {
                class: b.bytes as Object,
                string: false,
            },
        )?;
        eprintln!(
            "NATIVE_MONO_TEXT_ASSET_BYTES: member={:?}; actual-rooted-array=true; null={}",
            SPECS[index].0,
            native.is_null()
        );
        Ok(native)
    })
}
unsafe extern "C" fn get_bytes(pointer: usize) -> Object {
    read_bytes(0, pointer, None)
}
unsafe extern "C" fn preview(pointer: usize, maximum: i32) -> Object {
    read_bytes(1, pointer, Some(maximum))
}
unsafe extern "C" fn size(pointer: usize) -> i64 {
    checked(|| {
        let b = BINDING.get().ok_or("TextAsset adapters absent")?;
        Ok(
            callbacks::guest_arguments(b.targets[2], &[engine_objects::engine_pointer(pointer)?])?
                as i64,
        )
    })
}
