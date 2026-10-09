//! Exact locked corelib GetType(String) call. No general runtime-invoke ABI is
//! inferred from this one static reference signature.
use super::{c_char, Object};
use std::{ffi::CStr, ptr};
pub(super) fn validate_get_type(library: Object, method: Object) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal,$ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing invoke signature symbol: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
    );
    let class = sym!(
        "mono_method_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let name = sym!(
        "mono_method_get_name",
        unsafe extern "C" fn(Object) -> *const c_char
    );
    let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
    let sig = sym!(
        "mono_method_signature",
        unsafe extern "C" fn(Object) -> Object
    );
    let count = sym!(
        "mono_signature_get_param_count",
        unsafe extern "C" fn(Object) -> u32
    );
    let instance = sym!(
        "mono_signature_is_instance",
        unsafe extern "C" fn(Object) -> u32
    );
    let params = sym!(
        "mono_signature_get_params",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let returns = sym!(
        "mono_signature_get_return_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let byref = sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32);
    let type_class = sym!(
        "mono_class_from_mono_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let generic = sym!(
        "unity_mono_method_is_generic",
        unsafe extern "C" fn(Object) -> u32
    );
    let inflated = sym!(
        "unity_mono_method_is_inflated",
        unsafe extern "C" fn(Object) -> u32
    );
    let expected = unsafe { find(corlib(), c"System".as_ptr(), c"Type".as_ptr()) };
    let text = unsafe { name(method) };
    let signature = unsafe { sig(method) };
    if expected.is_null()
        || text.is_null()
        || signature.is_null()
        || unsafe { class(method) } != expected
        || unsafe { token(method) } != 0x060011d6
        || unsafe { CStr::from_ptr(text) }.to_bytes() != b"GetType"
        || unsafe { generic(method) } != 0
        || unsafe { inflated(method) } != 0
        || unsafe { instance(signature) } != 0
        || unsafe { count(signature) } != 1
    {
        return Err(
            "managed invoke outside exact locked static System.Type.GetType(String)".into(),
        );
    }
    let mut iterator = ptr::null_mut();
    let parameter = unsafe { params(signature, &mut iterator) };
    let ret = unsafe { returns(signature) };
    if parameter.is_null()
        || ret.is_null()
        || unsafe { kind(parameter) } != 0xe
        || unsafe { byref(parameter) } != 0
        || unsafe { kind(ret) } != 0x12
        || unsafe { byref(ret) } != 0
        || unsafe { type_class(ret) } != expected
    {
        return Err("GetType managed reference signature mismatch".into());
    }
    Ok(())
}

pub(super) fn owned_bodies(
    library: Object,
    method: Object,
) -> Result<Vec<(String, Vec<u8>, String)>, String> {
    macro_rules! sym {
        ($name:literal,$ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing IL inspection symbol: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let class = sym!(
        "mono_method_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let find = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const c_char, i32) -> Object
    );
    let header = sym!(
        "mono_method_get_header",
        unsafe extern "C" fn(Object) -> Object
    );
    let code = sym!(
        "mono_method_header_get_code",
        unsafe extern "C" fn(Object, *mut u32, *mut u32) -> *const u8
    );
    let free = sym!("mono_metadata_free_mh", unsafe extern "C" fn(Object));
    let disasm = sym!(
        "mono_disasm_code",
        unsafe extern "C" fn(Object, Object, *const u8, *const u8) -> *mut c_char
    );
    let free_text = sym!("mono_free", unsafe extern "C" fn(Object));
    let class_image = sym!(
        "mono_class_get_image",
        unsafe extern "C" fn(Object) -> Object
    );
    let resolve = sym!(
        "mono_get_method",
        unsafe extern "C" fn(Object, u32, Object) -> Object
    );
    let fullname = sym!(
        "mono_method_full_name",
        unsafe extern "C" fn(Object, i32) -> *mut c_char
    );
    let flags = sym!(
        "mono_method_get_flags",
        unsafe extern "C" fn(Object, *mut u32) -> u32
    );
    let mut result = Vec::new();
    for (label, target) in [
        ("method", method),
        ("class-cctor", unsafe {
            find(class(method), c".cctor".as_ptr(), 0)
        }),
    ] {
        if target.is_null() {
            continue;
        }
        let mh = unsafe { header(target) };
        if mh.is_null() {
            continue;
        }
        let mut size = 0;
        let mut stack = 0;
        let p = unsafe { code(mh, &mut size, &mut stack) };
        let bytes = if size <= 65536 && (size == 0 || !p.is_null()) {
            Some(if size == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(p, size as usize) }.to_vec()
            })
        } else {
            None
        };
        let listing = if let Some(bytes) = &bytes {
            if bytes.is_empty() {
                Some(String::new())
            } else {
                let text = unsafe { disasm(ptr::null_mut(), target, p, p.add(size as usize)) };
                if text.is_null() {
                    None
                } else {
                    let mut owned = Vec::new();
                    let mut terminated = false;
                    for i in 0..1024 * 1024 {
                        let byte = unsafe { text.add(i).read() } as u8;
                        if byte == 0 {
                            terminated = true;
                            break;
                        }
                        owned.push(byte);
                    }
                    unsafe {
                        free_text(text.cast());
                    }
                    if terminated {
                        Some(String::from_utf8_lossy(&owned).into_owned())
                    } else {
                        None
                    }
                }
            }
        } else {
            None
        };
        let listing = if let Some(mut listing) = listing {
            let mut dependencies = Vec::new();
            for line in listing.lines() {
                let columns: Vec<_> = line.split_whitespace().collect();
                if columns.len() < 3
                    || !matches!(
                        columns[1],
                        "call" | "callvirt" | "newobj" | "ldftn" | "ldvirtftn"
                    )
                {
                    continue;
                }
                let Some(hex) = columns[2].strip_prefix("0x") else {
                    continue;
                };
                let Ok(token) = u32::from_str_radix(hex, 16) else {
                    continue;
                };
                if dependencies.contains(&token) {
                    continue;
                }
                dependencies.push(token);
            }
            for token in dependencies {
                let member = unsafe { resolve(class_image(class(target)), token, ptr::null_mut()) };
                if member.is_null() {
                    listing.push_str(&format!("dependency {token:#x}: unresolved\n"));
                    continue;
                }
                let name = unsafe { fullname(member, 1) };
                if name.is_null() {
                    listing.push_str(&format!("dependency {token:#x}: NULL name\n"));
                    continue;
                }
                let text = unsafe { CStr::from_ptr(name) }
                    .to_string_lossy()
                    .into_owned();
                unsafe {
                    free_text(name.cast());
                }
                let mut implementation = 0;
                let attrs = unsafe { flags(member, &mut implementation) };
                listing.push_str(&format!(
                    "dependency {token:#x}: {text}; flags={attrs:#x}; impl={implementation:#x}\n"
                ));
            }
            Some(listing)
        } else {
            None
        };
        unsafe {
            free(mh);
        }
        result.push((
            format!("{label}/max-stack={stack}"),
            bytes.ok_or("pending IL exceeds diagnostic bound")?,
            listing.ok_or("pending IL listing exceeds diagnostic bound")?,
        ));
    }
    Ok(result)
}
