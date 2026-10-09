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

pub(super) fn validate_classlib_init(
    library: Object,
    method: Object,
    image: Object,
    step: usize,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal,$ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing classlib signature symbol: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
    );
    let class = sym!(
        "mono_method_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let class_image = sym!(
        "mono_class_get_image",
        unsafe extern "C" fn(Object) -> Object
    );
    let name = sym!(
        "mono_method_get_name",
        unsafe extern "C" fn(Object) -> *const c_char
    );
    let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
    let signature = sym!(
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
    let returns = sym!(
        "mono_signature_get_return_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let byref = sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32);
    let generic = sym!(
        "unity_mono_method_is_generic",
        unsafe extern "C" fn(Object) -> u32
    );
    let inflated = sym!(
        "unity_mono_method_is_inflated",
        unsafe extern "C" fn(Object) -> u32
    );
    let (class_name, expected_name, expected_token) = match step {
        0 => (
            c"ClassLibraryInitializer",
            b"InitAssemblyRedirections".as_slice(),
            0x060022ff,
        ),
        1 => (c"ClassLibraryInitializer", b"Init".as_slice(), 0x060022fe),
        2 => (
            c"UnhandledExceptionHandler",
            b"RegisterUECatcher".as_slice(),
            0x060024b2,
        ),
        3 => (
            c"StackTraceUtility",
            b"SetProjectFolder".as_slice(),
            0x06002478,
        ),
        4 => (
            c"UnitySynchronizationContext",
            b"InitializeSynchronizationContext".as_slice(),
            0x0600257f,
        ),
        5 => (
            c"SubsystemDescriptorStore",
            b"ClearManagedDescriptors".as_slice(),
            0x06000057,
        ),
        _ => return Err("managed startup step outside exact static void sequence".into()),
    };
    let namespace = if step == 5 {
        c"UnityEngine.SubsystemsImplementation"
    } else {
        c"UnityEngine"
    };
    let expected = unsafe { find(image, namespace.as_ptr(), class_name.as_ptr()) };
    let text = unsafe { name(method) };
    let sig = unsafe { signature(method) };
    if expected.is_null()
        || text.is_null()
        || sig.is_null()
        || unsafe { class(method) } != expected
        || unsafe { class_image(expected) } != image
        || unsafe { token(method) } != expected_token
        || unsafe { CStr::from_ptr(text) }.to_bytes() != expected_name
        || unsafe { generic(method) } != 0
        || unsafe { inflated(method) } != 0
        || unsafe { count(sig) } != u32::from(step == 3)
        || unsafe { instance(sig) } != 0
    {
        return Err("managed init outside locked ordered Unity startup methods".into());
    }
    if step == 3 {
        let params = sym!(
            "mono_signature_get_params",
            unsafe extern "C" fn(Object, *mut Object) -> Object
        );
        let mut iterator = ptr::null_mut();
        let parameter = unsafe { params(sig, &mut iterator) };
        if parameter.is_null()
            || unsafe { kind(parameter) } != 0xe
            || unsafe { byref(parameter) } != 0
        {
            return Err("project folder signature requires a String reference".into());
        }
    }
    let ret = unsafe { returns(sig) };
    if ret.is_null() || unsafe { kind(ret) } != 1 || unsafe { byref(ret) } != 0 {
        return Err("assembly redirection init must return void".into());
    }
    Ok(())
}

pub(super) fn verify_synchronization(library: Object, image: Object) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal,$ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing synchronization ABI symbol: ", $name).into());
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
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let object_class = sym!(
        "mono_object_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let field = sym!(
        "mono_class_get_field_from_name",
        unsafe extern "C" fn(Object, *const c_char) -> Object
    );
    let field_type = sym!(
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let field_get = sym!(
        "mono_field_get_value",
        unsafe extern "C" fn(Object, Object, Object)
    );
    let static_get = sym!(
        "mono_field_static_get_value",
        unsafe extern "C" fn(Object, Object, Object)
    );
    let vtable = sym!(
        "mono_class_vtable",
        unsafe extern "C" fn(Object, Object) -> Object
    );
    let domain = sym!("mono_domain_get", unsafe extern "C" fn() -> Object);
    let current_thread = sym!("mono_thread_current", unsafe extern "C" fn() -> Object);
    let thread_id = sym!(
        "mono_thread_get_managed_id",
        unsafe extern "C" fn(Object) -> i32
    );
    let invoke = sym!(
        "mono_runtime_invoke",
        unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object
    );
    let new_handle = sym!(
        "mono_gchandle_new_v2",
        unsafe extern "C" fn(Object, i32) -> Object
    );
    let target = sym!(
        "mono_gchandle_get_target_v2",
        unsafe extern "C" fn(Object) -> Object
    );
    let free = sym!("mono_gchandle_free_v2", unsafe extern "C" fn(Object));
    let collect = sym!("mono_gc_collect", unsafe extern "C" fn(i32));
    let generation = sym!("mono_gc_max_generation", unsafe extern "C" fn() -> i32);
    let class = unsafe {
        find(
            image,
            c"UnityEngine".as_ptr(),
            c"UnitySynchronizationContext".as_ptr(),
        )
    };
    let awaitable = unsafe { find(image, c"UnityEngine".as_ptr(), c"Awaitable".as_ptr()) };
    let base = unsafe {
        find(
            corlib(),
            c"System.Threading".as_ptr(),
            c"SynchronizationContext".as_ptr(),
        )
    };
    if class.is_null() || awaitable.is_null() || base.is_null() {
        return Err("synchronization classes missing".into());
    }
    let current = unsafe { method(base, c"get_Current".as_ptr(), 0) };
    if current.is_null() {
        return Err("SynchronizationContext.Current absent".into());
    }
    let mut exception = ptr::null_mut();
    let context = unsafe { invoke(current, ptr::null_mut(), ptr::null_mut(), &mut exception) };
    if !exception.is_null() || context.is_null() || unsafe { object_class(context) } != class {
        return Err("actual main-thread current context is not UnitySynchronizationContext".into());
    }
    let handle = unsafe { new_handle(context, 1) };
    if handle.is_null() {
        return Err("current synchronization context pin failed".into());
    }
    struct Pin(Object, unsafe extern "C" fn(Object));
    impl Drop for Pin {
        fn drop(&mut self) {
            unsafe {
                (self.1)(self.0);
            }
        }
    }
    let _pin = Pin(handle, free);
    let main_id = unsafe { field(class, c"m_MainThreadID".as_ptr()) };
    let await_id = unsafe { field(awaitable, c"_mainThreadId".as_ptr()) };
    let await_context = unsafe { field(awaitable, c"_synchronizationContext".as_ptr()) };
    if main_id.is_null()
        || await_id.is_null()
        || await_context.is_null()
        || unsafe { kind(field_type(main_id)) } != 8
        || unsafe { kind(field_type(await_id)) } != 8
        || unsafe { kind(field_type(await_context)) } != 0x12
    {
        return Err("synchronization field contract mismatch".into());
    }
    let thread = unsafe { current_thread() };
    if thread.is_null() {
        return Err("synchronization thread owner absent".into());
    }
    let expected_id = unsafe { thread_id(thread) };
    if expected_id <= 0 {
        return Err("synchronization managed thread id invalid".into());
    }
    let await_vtable = unsafe { vtable(domain(), awaitable) };
    if await_vtable.is_null() {
        return Err("Awaitable static storage absent".into());
    }
    for cycle in 0..2 {
        let mut actual_id = 0i32;
        let mut awaitable_id = 0i32;
        let mut stored = ptr::null_mut();
        unsafe {
            field_get(context, main_id, (&mut actual_id as *mut i32).cast());
            static_get(
                await_vtable,
                await_id,
                (&mut awaitable_id as *mut i32).cast(),
            );
            static_get(
                await_vtable,
                await_context,
                (&mut stored as *mut Object).cast(),
            );
        }
        if actual_id != expected_id
            || awaitable_id != expected_id
            || stored != context
            || unsafe { target(handle) } != context
        {
            return Err("Unity context/Awaitable/thread identity mismatch across GC".into());
        }
        if cycle == 0 {
            unsafe {
                collect(generation());
            }
        }
    }
    exception = ptr::null_mut();
    if unsafe { invoke(current, ptr::null_mut(), ptr::null_mut(), &mut exception) } != context
        || !exception.is_null()
        || unsafe { current_thread() } != thread
    {
        return Err("synchronization TLS owner changed across GC".into());
    }
    eprintln!("NATIVE_MONO_SYNCHRONIZATION_VERIFIED: Unity current/Awaitable shared=true managed-thread-id={expected_id} gc-stable=true");
    Ok(())
}

/// Verify the real CoreModule field, including the IL's slash normalization,
/// after collection. This is a native static root, not a guest object view.
pub(super) fn verify_project_folder(
    library: Object,
    image: Object,
    input: Object,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal,$ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing project folder symbol: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
    );
    let field = sym!(
        "mono_class_get_field_from_name",
        unsafe extern "C" fn(Object, *const c_char) -> Object
    );
    let field_type = sym!(
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let field_flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let get = sym!(
        "mono_field_static_get_value",
        unsafe extern "C" fn(Object, Object, Object)
    );
    let vtable = sym!(
        "mono_class_vtable",
        unsafe extern "C" fn(Object, Object) -> Object
    );
    let domain = sym!("mono_domain_get", unsafe extern "C" fn() -> Object);
    let length = sym!("mono_string_length", unsafe extern "C" fn(Object) -> i32);
    let chars = sym!(
        "mono_string_chars",
        unsafe extern "C" fn(Object) -> *const u16
    );
    let object_class = sym!(
        "mono_object_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let string_class = sym!("mono_get_string_class", unsafe extern "C" fn() -> Object);
    let collect = sym!("mono_gc_collect", unsafe extern "C" fn(i32));
    let generation = sym!("mono_gc_max_generation", unsafe extern "C" fn() -> i32);
    let units = |object: Object| -> Result<Option<Vec<u16>>, String> {
        if object.is_null() {
            return Ok(None);
        }
        if unsafe { object_class(object) } != unsafe { string_class() } {
            return Err("project folder is not a String".into());
        }
        let n = unsafe { length(object) };
        let p = unsafe { chars(object) };
        if !(0..=65536).contains(&n) || p.is_null() {
            return Err("project folder string outside bound".into());
        }
        Ok(Some(
            unsafe { std::slice::from_raw_parts(p, n as usize) }.to_vec(),
        ))
    };
    let expected = units(input)?.map(|text| {
        text.into_iter()
            .map(|u| if u == 92 { 47 } else { u })
            .collect::<Vec<_>>()
    });
    let class = unsafe {
        find(
            image,
            c"UnityEngine".as_ptr(),
            c"StackTraceUtility".as_ptr(),
        )
    };
    if class.is_null() {
        return Err("project folder class absent".into());
    }
    let field = unsafe { field(class, c"projectFolder".as_ptr()) };
    if field.is_null()
        || unsafe { kind(field_type(field)) } != 0xe
        || unsafe { field_flags(field) } & 0x10 == 0
    {
        return Err("project folder static String field mismatch".into());
    }
    let storage = unsafe { vtable(domain(), class) };
    if storage.is_null() {
        return Err("project folder static storage absent".into());
    }
    for cycle in 0..2 {
        let mut stored = ptr::null_mut();
        unsafe {
            get(storage, field, (&mut stored as *mut Object).cast());
        }
        if units(stored)? != expected {
            return Err("project folder normalization/static root mismatch".into());
        }
        if cycle == 0 {
            unsafe {
                collect(generation());
            }
        }
    }
    eprintln!("NATIVE_MONO_PROJECT_FOLDER_VERIFIED: normalized=true static-root-gc-stable=true");
    Ok(())
}

pub(super) fn verify_subsystems(library: Object, image: Object) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal,$ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing subsystem verification symbol: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
    );
    let field = sym!(
        "mono_class_get_field_from_name",
        unsafe extern "C" fn(Object, *const c_char) -> Object
    );
    let field_type = sym!(
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let field_class = sym!(
        "mono_class_from_mono_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
    let get = sym!(
        "mono_field_static_get_value",
        unsafe extern "C" fn(Object, Object, Object)
    );
    let vtable = sym!(
        "mono_class_vtable",
        unsafe extern "C" fn(Object, Object) -> Object
    );
    let domain = sym!("mono_domain_get", unsafe extern "C" fn() -> Object);
    let object_class = sym!(
        "mono_object_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const c_char, i32) -> Object
    );
    let invoke = sym!(
        "mono_runtime_invoke",
        unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object
    );
    let unbox = sym!("mono_object_unbox", unsafe extern "C" fn(Object) -> Object);
    let int_class = sym!("mono_get_int32_class", unsafe extern "C" fn() -> Object);
    let collect = sym!("mono_gc_collect", unsafe extern "C" fn(i32));
    let generation = sym!("mono_gc_max_generation", unsafe extern "C" fn() -> i32);
    let class = unsafe {
        find(
            image,
            c"UnityEngine.SubsystemsImplementation".as_ptr(),
            c"SubsystemDescriptorStore".as_ptr(),
        )
    };
    if class.is_null() {
        return Err("subsystem descriptor store absent".into());
    }
    let storage = unsafe { vtable(domain(), class) };
    if storage.is_null() {
        return Err("subsystem descriptor static storage absent".into());
    }
    for cycle in 0..2 {
        for name in [
            c"s_IntegratedDescriptors",
            c"s_StandaloneDescriptors",
            c"s_DeprecatedDescriptors",
        ] {
            let f = unsafe { field(class, name.as_ptr()) };
            if f.is_null() || unsafe { flags(f) } & 0x10 == 0 {
                return Err("subsystem descriptor list field mismatch".into());
            }
            let expected = unsafe { field_class(field_type(f)) };
            let mut list: Object = ptr::null_mut();
            unsafe {
                get(storage, f, (&mut list as *mut Object).cast());
            }
            if expected.is_null() || list.is_null() || unsafe { object_class(list) } != expected {
                return Err("subsystem list static root/type mismatch".into());
            }
            let count = unsafe { method(expected, c"get_Count".as_ptr(), 0) };
            if count.is_null() {
                return Err("subsystem list Count absent".into());
            }
            let mut exception = ptr::null_mut();
            let boxed = unsafe { invoke(count, list, ptr::null_mut(), &mut exception) };
            if !exception.is_null()
                || boxed.is_null()
                || unsafe { object_class(boxed) } != unsafe { int_class() }
            {
                return Err("subsystem list Count failed".into());
            }
            let value = unsafe { unbox(boxed) };
            if value.is_null() || unsafe { value.cast::<i32>().read_unaligned() } != 0 {
                return Err("startup subsystem lists not empty".into());
            }
        }
        if cycle == 0 {
            unsafe {
                collect(generation());
            }
        }
    }
    eprintln!("NATIVE_MONO_SUBSYSTEMS_VERIFIED: real-generic-lists=3 counts=0 static-roots-gc-stable=true");
    Ok(())
}
