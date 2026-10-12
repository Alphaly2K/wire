//! Fixed engine object getters return an actual native v2 GC handle to the
//! managed Unmarshal helper. No engine or host address is used as MonoObject.
use super::{bindings, callbacks, logging, managed_calls, Object};
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
const FIND_SPECS: [(&str, u64); 2] = [
    ("Find_Injected", 0x1800bf730),
    ("FindGameObjectWithTag_Injected", 0x1800bdf40),
];
struct Binding {
    library: usize,
    targets: [u64; 4],
    classes: [usize; 4],
    find_targets: [u64; 2],
    game_object: usize,
    component: usize,
    operations: [u64; 5],
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
    let game_object = unsafe { find(core, c"UnityEngine".as_ptr(), c"GameObject".as_ptr()) };
    let span = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"ManagedSpanWrapper".as_ptr(),
        )
    };
    let corlib = unsafe { corlib() };
    let void = unsafe { find(corlib, c"System".as_ptr(), c"Void".as_ptr()) };
    let ty = unsafe { find(corlib, c"System".as_ptr(), c"Type".as_ptr()) };
    let component = unsafe { find(core, c"UnityEngine".as_ptr(), c"Component".as_ptr()) };
    let boolean = unsafe { find(corlib, c"System".as_ptr(), c"Boolean".as_ptr()) };
    let single = unsafe { find(corlib, c"System".as_ptr(), c"Single".as_ptr()) };
    let mut alignment = 0;
    if [
        intptr,
        owner,
        object,
        scriptable,
        game_object,
        span,
        void,
        ty,
        component,
        boolean,
        single,
    ]
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
    logging::validate_span_layout(library, span)?;
    let mut find_targets = [0; 2];
    let mut find_names = Vec::new();
    let operation_specs = [
        (
            "UnityEngine.GameObject::Internal_CreateGameObject_Injected",
            0x1800bf350,
            game_object,
            c"Internal_CreateGameObject_Injected",
            vec![
                (1, void, false),
                (0x12, game_object, false),
                (0x11, span, true),
            ],
        ),
        (
            "UnityEngine.GameObject::Internal_AddComponentWithType_Injected",
            0x1800bd370,
            game_object,
            c"Internal_AddComponentWithType_Injected",
            vec![
                (0x18, intptr, false),
                (0x18, intptr, false),
                (0x12, ty, false),
            ],
        ),
        (
            "UnityEngine.Object::DontDestroyOnLoad_Injected",
            0x1800c3600,
            object,
            c"DontDestroyOnLoad_Injected",
            vec![(1, void, false), (0x18, intptr, false)],
        ),
        (
            "UnityEngine.Object::DestroyImmediate_Injected",
            0x1800c2fd0,
            object,
            c"DestroyImmediate_Injected",
            vec![(1, void, false), (0x18, intptr, false), (2, boolean, false)],
        ),
        (
            "UnityEngine.Object::Destroy_Injected",
            0x1800c2eb0,
            object,
            c"Destroy_Injected",
            vec![
                (1, void, false),
                (0x18, intptr, false),
                (0xc, single, false),
            ],
        ),
    ];
    let mut operations = [0; 5];
    for (index, (name, target, class, member, specs)) in operation_specs.iter().enumerate() {
        operations[index] = registry.exact_target(name, *target)?;
        api.fixed_typed_icall(
            unsafe { method(*class, member.as_ptr(), (specs.len() - 1) as i32) },
            *class,
            specs,
            images,
        )?;
    }
    for (index, (member, target)) in FIND_SPECS.iter().enumerate() {
        let full =
            CString::new(format!("UnityEngine.GameObject::{member}")).map_err(|e| e.to_string())?;
        find_targets[index] =
            registry.exact_target(full.to_str().map_err(|e| e.to_string())?, *target)?;
        let member = CString::new(*member).map_err(|e| e.to_string())?;
        api.fixed_typed_icall(
            unsafe { method(game_object, member.as_ptr(), 1) },
            game_object,
            &[(0x18, intptr, false), (0x11, span, true)],
            images,
        )?;
        find_names.push(full);
    }
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
            find_targets,
            game_object: game_object as usize,
            component: component as usize,
            operations,
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
    for (name, adapter) in find_names
        .iter()
        .zip([find_name as *const (), find_tag as *const ()])
    {
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    for ((name, ..), adapter) in operation_specs.iter().zip([
        create_game_object as *const (),
        add_component as *const (),
        dont_destroy as *const (),
        destroy_immediate as *const (),
        destroy as *const (),
    ]) {
        let name = CString::new(*name).map_err(|e| e.to_string())?;
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    Ok(())
}
fn native_handle(b: &Binding, class: usize, handle: u64) -> Result<usize, String> {
    engine_handle(b.library as Object, class as Object, handle)
}
pub(super) fn engine_handle(library: Object, class: Object, handle: u64) -> Result<usize, String> {
    if handle == 0 {
        return Ok(0);
    }
    callbacks::with_metadata_service(|route, memory| {
        route.sync_guest(memory)?;
        let roots = route
            .objects
            .as_ref()
            .ok_or("engine object result has no root service")?;
        let token = roots.handles.1.target(roots.handles.0, handle)?;
        let object = route.rooted_native(token)?;
        managed_calls::Api::load(library)?.compatible(
            object,
            managed_calls::Shape::Reference {
                class,
                string: false,
            },
        )?;
        Ok(route.native_handle(handle)? as usize)
    })
}
fn get(index: usize) -> usize {
    let result = std::panic::catch_unwind(|| -> Result<usize, String> {
        let b = BINDING.get().ok_or("engine object getter absent")?;
        let handle = callbacks::guest_arguments(b.targets[index], &[])?;
        let native = native_handle(b, b.classes[index], handle)?;
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
unsafe fn find_object(span: *const logging::Span, index: usize) -> usize {
    let result = std::panic::catch_unwind(|| -> Result<usize, String> {
        let b = BINDING.get().ok_or("GameObject lookup absent")?;
        let units = unsafe { logging::owned_span(span) }?;
        let handle = callbacks::guest_string_span(b.find_targets[index], units.as_deref(), &[])?;
        let native = native_handle(b, b.game_object, handle)?;
        CALLS.with(|calls| calls.set(calls.get() + 1));
        eprintln!(
            "NATIVE_MONO_GAME_OBJECT_LOOKUP: member={}; actual-engine-handle={handle:#x}",
            FIND_SPECS[index].0
        );
        Ok(native)
    });
    match result {
        Ok(Ok(handle)) => handle,
        other => {
            eprintln!("NATIVE_MONO_GAME_OBJECT_LOOKUP_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
unsafe extern "C" fn find_name(span: *const logging::Span) -> usize {
    unsafe { find_object(span, 0) }
}
unsafe extern "C" fn find_tag(span: *const logging::Span) -> usize {
    unsafe { find_object(span, 1) }
}
fn checked<T>(operation: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_ENGINE_OBJECT_OPERATION_REJECT: {error}");
            std::process::abort();
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_ENGINE_OBJECT_OPERATION_REJECT: adapter panicked");
            std::process::abort();
        }
    }
}
pub(super) fn engine_pointer(pointer: usize) -> Result<u64, String> {
    callbacks::with_service(|memory, runtime| {
        let pointer = pointer as u64;
        if pointer == 0
            || !memory.owned_noncode_access(pointer, 1)
            || !(runtime.live_data_slot(pointer, 1) || memory.guest_private_data_slot(pointer, 1))
        {
            return Err("Unity operation requires live original engine data pointer".into());
        }
        Ok(pointer)
    })
}
unsafe extern "C" fn create_game_object(object: Object, span: *const logging::Span) {
    checked(|| {
        let b = BINDING.get().ok_or("GameObject creation absent")?;
        let units = unsafe { logging::owned_span(span) }?;
        managed_calls::Api::load(b.library as Object)?.compatible(
            object,
            managed_calls::Shape::Reference {
                class: b.game_object as Object,
                string: false,
            },
        )?;
        if object.is_null() {
            return Err("GameObject creation receiver is NULL".into());
        }
        let token = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            let token = route.root_reference(object, memory)?;
            let roots = route
                .objects
                .as_mut()
                .ok_or("GameObject creation has no roots")?;
            roots
                .roots
                .publish_engine_object(roots.api, memory, token)?;
            Ok(token)
        })?;
        callbacks::guest_positioned_string_span(b.operations[0], units.as_deref(), &[token], &[])?;
        callbacks::with_metadata_service(|route, memory| route.sync_guest(memory))?;
        CALLS.with(|calls| calls.set(calls.get() + 1));
        eprintln!("NATIVE_MONO_GAME_OBJECT_CREATED: rooted-self={token:#x}; original-engine=true");
        Ok(())
    });
}
unsafe extern "C" fn add_component(pointer: usize, ty: Object) -> usize {
    checked(|| {
        let b = BINDING.get().ok_or("component creation absent")?;
        let pointer = engine_pointer(pointer)?;
        let ty =
            callbacks::with_metadata_service(|route, memory| route.root_reflection(ty, memory))?;
        let handle = callbacks::guest_arguments(b.operations[1], &[pointer, ty])?;
        let result = native_handle(b, b.component, handle)?;
        CALLS.with(|calls| calls.set(calls.get() + 1));
        Ok(result)
    })
}
unsafe extern "C" fn dont_destroy(pointer: usize) {
    checked(|| {
        let b = BINDING.get().ok_or("DontDestroyOnLoad absent")?;
        let pointer = engine_pointer(pointer)?;
        callbacks::guest_arguments(b.operations[2], &[pointer])?;
        CALLS.with(|calls| calls.set(calls.get() + 1));
        Ok(())
    });
}
unsafe extern "C" fn destroy_immediate(pointer: usize, allow_assets: u8) {
    checked(|| {
        let b = BINDING.get().ok_or("DestroyImmediate absent")?;
        if allow_assets > 1 {
            return Err("DestroyImmediate Boolean is not canonical".into());
        }
        let pointer = if pointer == 0 {
            0
        } else {
            engine_pointer(pointer)?
        };
        callbacks::guest_arguments(b.operations[3], &[pointer, u64::from(allow_assets)])?;
        callbacks::with_metadata_service(|route, memory| route.sync_guest(memory))?;
        CALLS.with(|calls| calls.set(calls.get() + 1));
        eprintln!("NATIVE_MONO_ENGINE_OBJECT_DESTROYED: immediate=true; original-engine=true");
        Ok(())
    });
}
unsafe extern "C" fn destroy(pointer: usize, delay: f32) {
    checked(|| {
        let b = BINDING.get().ok_or("Destroy absent")?;
        let pointer = if pointer == 0 {
            0
        } else {
            engine_pointer(pointer)?
        };
        callbacks::guest_scalars(
            b.operations[4],
            &[pointer, 0],
            &[(1, u64::from(delay.to_bits()))],
        )?;
        callbacks::with_metadata_service(|route, memory| route.sync_guest(memory))?;
        CALLS.with(|calls| calls.set(calls.get() + 1));
        Ok(())
    });
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
