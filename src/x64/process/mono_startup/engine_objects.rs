//! Fixed engine object getters return an actual native v2 GC handle to the
//! managed Unmarshal helper. No engine or host address is used as MonoObject.
use super::{bindings, callbacks, managed_calls, Object};
use std::{cell::Cell, ffi::CString, sync::OnceLock};
const SPECS: [(&str, u64, bool); 4] = [
    (
        "get_INTERNAL_currentRenderPipeline_Injected",
        0x1800dcdc0,
        true,
    ),
    (
        "get_INTERNAL_defaultRenderPipeline_Injected",
        0x1800dce10,
        true,
    ),
    (
        "get_INTERNAL_currentRenderPipelineGlobalSettings_Injected",
        0x1800dddc0,
        true,
    ),
    ("GetGraphicsSettings_Injected", 0x1800dcf90, false),
];
struct Binding {
    library: usize,
    targets: [u64; 4],
    classes: [usize; 4],
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
    registry: &bindings::Registry,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing engine object API: ", $name).into());
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
    let size = sym!(
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let intptr = unsafe { find(corlib(), c"System".as_ptr(), c"IntPtr".as_ptr()) };
    let owner = unsafe {
        find(
            core,
            c"UnityEngine.Rendering".as_ptr(),
            c"GraphicsSettings".as_ptr(),
        )
    };
    let object = unsafe { find(core, c"UnityEngine".as_ptr(), c"Object".as_ptr()) };
    let scriptable = unsafe { find(core, c"UnityEngine".as_ptr(), c"ScriptableObject".as_ptr()) };
    let mut alignment = 0;
    if [intptr, owner, object, scriptable]
        .iter()
        .any(|p| p.is_null())
        || unsafe { size(intptr, &mut alignment) } != 8
        || alignment != 8
    {
        return Err("engine object getter classes/IntPtr layout mismatch".into());
    }
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; 4];
    let mut classes = [0; 4];
    let mut names = Vec::new();
    for (index, (member, target, is_scriptable)) in SPECS.iter().enumerate() {
        let full = CString::new(format!("UnityEngine.Rendering.GraphicsSettings::{member}"))
            .map_err(|e| e.to_string())?;
        targets[index] =
            registry.exact_target(full.to_str().map_err(|e| e.to_string())?, *target)?;
        classes[index] = if *is_scriptable { scriptable } else { object } as usize;
        let member = CString::new(*member).map_err(|e| e.to_string())?;
        api.fixed_typed_icall(
            unsafe { method(owner, member.as_ptr(), 0) },
            owner,
            &[(0x18, intptr, false)],
            images,
        )?;
        names.push(full);
    }
    BINDING
        .set(Binding {
            library: library as usize,
            targets,
            classes,
        })
        .map_err(|_| "engine object getters already installed")?;
    for (name, adapter) in names.iter().zip([
        current as *const (),
        default_pipeline as *const (),
        global_settings as *const (),
        graphics_settings as *const (),
    ]) {
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    Ok(())
}
fn get(index: usize) -> usize {
    let result = std::panic::catch_unwind(|| -> Result<usize, String> {
        let b = BINDING.get().ok_or("engine object getter absent")?;
        let handle = callbacks::guest_arguments(b.targets[index], &[])?;
        let native = if handle == 0 {
            0
        } else {
            callbacks::with_metadata_service(|route, memory| {
                route.sync_guest(memory)?;
                let roots = route
                    .objects
                    .as_ref()
                    .ok_or("engine getter has no root service")?;
                let token = roots.handles.1.target(roots.handles.0, handle)?;
                let object = route.rooted_native(token)?;
                managed_calls::Api::load(b.library as Object)?.compatible(
                    object,
                    managed_calls::Shape::Reference {
                        class: b.classes[index] as Object,
                        string: false,
                    },
                )?;
                Ok(route.native_handle(handle)? as usize)
            })?
        };
        CALLS.with(|calls| calls.set(calls.get() + 1));
        eprintln!("NATIVE_MONO_ENGINE_OBJECT_GETTER: member={}; actual-engine-handle={handle:#x}; native-v2-handle=true", SPECS[index].0);
        Ok(native)
    });
    match result {
        Ok(Ok(handle)) => handle,
        other => {
            eprintln!("NATIVE_MONO_ENGINE_OBJECT_GETTER_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
unsafe extern "C" fn current() -> usize {
    get(0)
}
unsafe extern "C" fn default_pipeline() -> usize {
    get(1)
}
unsafe extern "C" fn global_settings() -> usize {
    get(2)
}
unsafe extern "C" fn graphics_settings() -> usize {
    get(3)
}
pub(super) fn report() -> String {
    format!(
        "native-engine-object-getter-calls={}\n",
        CALLS.with(Cell::get)
    )
}
