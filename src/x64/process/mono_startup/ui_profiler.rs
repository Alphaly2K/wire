//! UI profiling calls preserve the original engine behavior. Managed UTF16 is
//! copied into a scoped guest span; engine object addresses stay guest data.
use super::{bindings, callbacks, engine_objects, logging, managed_calls, Object};
use std::{cell::Cell, ffi::CStr, ptr, sync::OnceLock};
const SPECS: [(&CStr, &CStr, u64); 3] = [
    (
        c"UnityEngine.UISystemProfilerApi::BeginSample",
        c"BeginSample",
        0x18005fee0,
    ),
    (
        c"UnityEngine.UISystemProfilerApi::EndSample",
        c"EndSample",
        0x18005fee0,
    ),
    (
        c"UnityEngine.UISystemProfilerApi::AddMarker_Injected",
        c"AddMarker_Injected",
        0x1800b4880,
    ),
];
static TARGETS: OnceLock<[u64; 3]> = OnceLock::new();
thread_local! { static SEEN: Cell<u8> = const { Cell::new(0) }; }
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
                return Err(concat!("missing UI profiler symbol: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, *const libc::c_char) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, i32) -> Object
    );
    let nested = sym!(
        "mono_class_get_nested_types",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let name = sym!(
        "mono_class_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let mut owners: Vec<_> = images
        .iter()
        .map(|&image| unsafe {
            find(
                image,
                c"UnityEngine".as_ptr(),
                c"UISystemProfilerApi".as_ptr(),
            )
        })
        .filter(|p| !p.is_null())
        .collect();
    owners.sort_unstable_by_key(|p| *p as usize);
    owners.dedup();
    if owners.len() != 1 {
        return Err("UI profiler owner is not unique in owned images".into());
    }
    let owner = owners[0];
    let mut iter = ptr::null_mut();
    let mut sample: Object = ptr::null_mut();
    loop {
        let candidate = unsafe { nested(owner, &mut iter) };
        if candidate.is_null() {
            break;
        }
        let candidate_name = unsafe { name(candidate) };
        if !candidate_name.is_null() && unsafe { CStr::from_ptr(candidate_name) } == c"SampleType" {
            if !sample.is_null() {
                return Err("UI profiler SampleType is ambiguous".into());
            }
            sample = candidate;
        }
    }
    if sample.is_null() {
        return Err("UI profiler nested SampleType absent".into());
    }
    let corlib = unsafe { corlib() };
    if !images.contains(&corlib) || !images.contains(&core) {
        return Err("UI profiler core images are not owned".into());
    }
    let void = unsafe { find(corlib, c"System".as_ptr(), c"Void".as_ptr()) };
    let intptr = unsafe { find(corlib, c"System".as_ptr(), c"IntPtr".as_ptr()) };
    let span = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"ManagedSpanWrapper".as_ptr(),
        )
    };
    logging::validate_span_layout(library, span)?;
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; 3];
    for (index, &(binding, member, target)) in SPECS.iter().enumerate() {
        if index < 2 {
            api.scalar_icall(
                unsafe { method(owner, member.as_ptr(), 1) },
                owner,
                void,
                &[sample],
                images,
            )?;
        } else {
            api.fixed_typed_icall(
                unsafe { method(owner, member.as_ptr(), 2) },
                owner,
                &[(1, void, false), (0x11, span, true), (0x18, intptr, false)],
                images,
            )?;
        }
        targets[index] =
            registry.exact_target(binding.to_str().map_err(|e| e.to_string())?, target)?;
    }
    TARGETS
        .set(targets)
        .map_err(|_| "UI profiler adapters already installed")?;
    for ((binding, _, _), adapter) in SPECS.into_iter().zip([
        begin as *const () as Object,
        end as *const () as Object,
        marker as *const () as Object,
    ]) {
        unsafe { add(binding.as_ptr(), adapter) };
    }
    Ok(())
}
fn checked(index: usize, work: impl FnOnce(u64) -> Result<(), String>) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let targets = TARGETS.get().ok_or("UI profiler adapters absent")?;
        work(targets[index])?;
        SEEN.with(|seen| {
            let bit = 1 << index;
            if seen.get() & bit == 0 {
                seen.set(seen.get() | bit);
                eprintln!(
                    "NATIVE_MONO_UI_PROFILER_VERIFIED: member={:?}; original-engine=true",
                    SPECS[index].1
                );
            }
        });
        Ok::<_, String>(())
    })) {
        Ok(Ok(())) => (),
        other => {
            eprintln!("NATIVE_MONO_UI_PROFILER_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
unsafe extern "C" fn begin(sample: i32) {
    checked(0, |target| {
        callbacks::guest_arguments(target, &[sample as u32 as u64]).map(|_| ())
    });
}
unsafe extern "C" fn end(sample: i32) {
    checked(1, |target| {
        callbacks::guest_arguments(target, &[sample as u32 as u64]).map(|_| ())
    });
}
unsafe extern "C" fn marker(span: *const logging::Span, object: usize) {
    checked(2, |target| {
        let pointer = engine_objects::engine_pointer(object)?;
        let units = unsafe { logging::owned_span(span) }?;
        callbacks::guest_string_span(target, units.as_deref(), &[pointer]).map(|_| ())
    });
}
