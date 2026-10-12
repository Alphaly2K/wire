//! Actual Unity job reflection registration, using rooted RuntimeTypes and
//! managed delegates. Creating the engine record does not admit job execution.
use super::{callbacks, managed_calls, Object};
use std::{cell::Cell, sync::OnceLock};
pub(super) const NAME: &str = "Unity.Jobs.LowLevel.Unsafe.JobsUtility::CreateJobReflectionData";
pub(super) const TARGET: u64 = 0x18005fdb0;
struct Binding {
    library: usize,
    target: u64,
    ty: usize,
    delegate: usize,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
thread_local! { static CALLS: Cell<usize> = const { Cell::new(0) }; }
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn install(
    library: Object,
    core: Object,
    images: &[Object],
    target: u64,
) -> Result<(), String> {
    if target != TARGET || !images.contains(&core) {
        return Err("job reflection target/image mismatch".into());
    }
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing job reflection API: ", $name).into());
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
    let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let owner = unsafe {
        find(
            core,
            c"Unity.Jobs.LowLevel.Unsafe".as_ptr(),
            c"JobsUtility".as_ptr(),
        )
    };
    if owner.is_null() {
        return Err("job reflection owner absent".into());
    }
    let method = unsafe { method(owner, c"CreateJobReflectionData".as_ptr(), 5) };
    if method.is_null() || unsafe { token(method) } != 0x0600013e {
        return Err("job reflection method/token mismatch".into());
    }
    let corlib = unsafe { corlib() };
    let system = |name: &std::ffi::CStr| unsafe { find(corlib, c"System".as_ptr(), name.as_ptr()) };
    let ty = system(c"Type");
    let object = system(c"Object");
    let delegate = system(c"Delegate");
    if delegate.is_null() {
        return Err("job reflection Delegate class absent".into());
    }
    managed_calls::Api::load(library)?.fixed_typed_icall(
        method,
        owner,
        &[
            (0x18, system(c"IntPtr"), false),
            (0x12, ty, false),
            (0x12, ty, false),
            (0x1c, object, false),
            (0x1c, object, false),
            (0x1c, object, false),
        ],
        images,
    )?;
    BINDING
        .set(Binding {
            library: library as usize,
            target,
            ty: ty as usize,
            delegate: delegate as usize,
        })
        .map_err(|_| "job reflection already installed")?;
    unsafe {
        add(
            c"Unity.Jobs.LowLevel.Unsafe.JobsUtility::CreateJobReflectionData".as_ptr(),
            create as *const () as Object,
        );
    }
    Ok(())
}
unsafe extern "C" fn create(
    wrapper: Object,
    user: Object,
    first: Object,
    second: Object,
    third: Object,
) -> usize {
    match std::panic::catch_unwind(|| -> Result<usize, String> {
        let binding = BINDING.get().ok_or("job reflection binding absent")?;
        let api = managed_calls::Api::load(binding.library as Object)?;
        for ty in [wrapper, user] {
            api.compatible(
                ty,
                managed_calls::Shape::Reference {
                    class: binding.ty as Object,
                    string: false,
                },
            )?;
        }
        for delegate in [first, second, third] {
            api.compatible(
                delegate,
                managed_calls::Shape::Reference {
                    class: binding.delegate as Object,
                    string: false,
                },
            )?;
        }
        let arguments = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            let reflection = |route: &mut super::reentry::Metadata<'_>,
                              memory: &mut crate::x64::memory::Memory,
                              ty: Object| {
                if ty.is_null() {
                    Ok(0)
                } else {
                    route.root_reflection(ty, memory)
                }
            };
            Ok([
                reflection(route, memory, wrapper)?,
                reflection(route, memory, user)?,
                route.root_reference(first, memory)?,
                route.root_reference(second, memory)?,
                route.root_reference(third, memory)?,
            ])
        })?;
        let result = callbacks::guest_arguments(binding.target, &arguments)?;
        if result != 0 {
            callbacks::with_service(|memory, runtime| {
                if !memory.owned_noncode_access(result, 1)
                    || !(runtime.live_data_slot(result, 1)
                        || memory.guest_private_data_slot(result, 1))
                {
                    return Err(
                        "job reflection result is not an original live engine data handle".into(),
                    );
                }
                Ok(())
            })?;
        }
        CALLS.with(|calls| calls.set(calls.get() + 1));
        eprintln!("NATIVE_MONO_JOB_REFLECTION_VERIFIED: original-engine-result={result:#x}; rooted-types-and-delegates=true; job-execution-admitted=false");
        Ok(result as usize)
    }) {
        Ok(Ok(result)) => result,
        other => {
            eprintln!("NATIVE_MONO_JOB_REFLECTION_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
pub(super) fn report() -> String {
    format!("native-job-reflection-calls={}\n", CALLS.with(Cell::get))
}
