//! Component queries execute the original engine. Types, arrays and result
//! lists cross the boundary through canonical roots and checked backing views.
use super::{bindings, callbacks, closed_types, engine_objects, logging, managed_calls, Object};
use std::{
    cell::Cell,
    ffi::{CStr, CString},
    sync::OnceLock,
};

const SPECS: [(&CStr, &CStr, u64); 8] = [
    (
        c"GameObject",
        c"GetComponentsInternal_Injected",
        0x1800bc790,
    ),
    (
        c"Component",
        c"GetComponentsForListInternal_Injected",
        0x1800bb050,
    ),
    (c"GameObject", c"GetComponent_Injected", 0x1800bbc60),
    (
        c"GameObject",
        c"GetComponentInChildren_Injected",
        0x1800bc590,
    ),
    (c"GameObject", c"GetComponentInParent_Injected", 0x1800bc690),
    (c"GameObject", c"GetComponentCount_Injected", 0x180074cc0),
    (c"GameObject", c"GetComponentIndex_Injected", 0x1800bd450),
    (c"Component", c"GetComponent_Injected", 0x1800bac90),
];
macro_rules! sym {
    ($lib:expr, $name:literal, $ty:ty) => {{
        let p = unsafe { libc::dlsym($lib, concat!($name, "\0").as_ptr().cast()) };
        if p.is_null() {
            return Err(concat!("missing component query API: ", $name).into());
        }
        unsafe { std::mem::transmute::<Object, $ty>(p) }
    }};
}
#[derive(Clone, Copy)]
struct Api {
    object_class: unsafe extern "C" fn(Object) -> Object,
    type_class: unsafe extern "C" fn(Object) -> Object,
    reflected: unsafe extern "C" fn(Object) -> Object,
    image: unsafe extern "C" fn(Object) -> Object,
    name: unsafe extern "C" fn(Object) -> *const libc::c_char,
    namespace: unsafe extern "C" fn(Object) -> *const libc::c_char,
    value: unsafe extern "C" fn(Object) -> i32,
    array_class: unsafe extern "C" fn(Object, u32) -> Object,
}
struct Binding {
    library: usize,
    api: Api,
    component: usize,
    corlib: usize,
    targets: [u64; 8],
    images: Vec<usize>,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
thread_local! { static CALLS: Cell<[usize; 8]> = const { Cell::new([0; 8]) }; }
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn report() -> String {
    format!(
        "native-component-query-calls={}\n",
        CALLS.with(|c| c.get().iter().sum::<usize>())
    )
}
pub(super) fn install(
    library: Object,
    core: Object,
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
    let find = sym!(
        library,
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, *const libc::c_char) -> Object
    );
    let method = sym!(
        library,
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, i32) -> Object
    );
    let corlib = sym!(library, "mono_get_corlib", unsafe extern "C" fn() -> Object);
    let add = sym!(
        library,
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let corlib = unsafe { corlib() };
    let class = |image, namespace: &CStr, name: &CStr| unsafe {
        find(image, namespace.as_ptr(), name.as_ptr())
    };
    let pointer = (0x18, class(corlib, c"System", c"IntPtr"), false);
    let boolean = (2, class(corlib, c"System", c"Boolean"), false);
    let integer = (8, class(corlib, c"System", c"Int32"), false);
    let void = (1, class(corlib, c"System", c"Void"), false);
    let ty = (0x12, class(corlib, c"System", c"Type"), false);
    let object = (0x1c, class(corlib, c"System", c"Object"), false);
    let array = (0x12, class(corlib, c"System", c"Array"), false);
    let span = class(core, c"UnityEngine.Bindings", c"ManagedSpanWrapper");
    logging::validate_span_layout(library, span)?;
    let call_api = managed_calls::Api::load(library)?;
    let mut targets = [0; 8];
    let mut names = Vec::new();
    for (index, (owner, member, target)) in SPECS.iter().enumerate() {
        let owner_class = class(core, c"UnityEngine", owner);
        let signature = match index {
            0 => vec![
                array, pointer, ty, boolean, boolean, boolean, boolean, object,
            ],
            1 => vec![void, pointer, ty, object],
            2 => vec![pointer, pointer, ty],
            3 | 4 => vec![pointer, pointer, ty, boolean],
            5 => vec![integer, pointer],
            6 => vec![integer, pointer, pointer],
            7 => vec![pointer, pointer, (0x11, span, true)],
            _ => unreachable!(),
        };
        let name = CString::new(format!(
            "UnityEngine.{}::{}",
            owner.to_str().unwrap(),
            member.to_str().unwrap()
        ))
        .unwrap();
        call_api
            .fixed_typed_icall(
                unsafe { method(owner_class, member.as_ptr(), (signature.len() - 1) as i32) },
                owner_class,
                &signature,
                images,
            )
            .map_err(|error| format!("{}: {error}", name.to_string_lossy()))?;
        targets[index] = registry.exact_target(name.to_str().unwrap(), *target)?;
        names.push(name);
    }
    let component_class = class(core, c"UnityEngine", c"Component");
    if component_class.is_null() {
        return Err("component query result declaration absent".into());
    }
    BINDING
        .set(Binding {
            library: library as usize,
            component: component_class as usize,
            corlib: corlib as usize,
            targets,
            images: images.iter().map(|image| *image as usize).collect(),
            api: Api {
                object_class: sym!(
                    library,
                    "mono_object_get_class",
                    unsafe extern "C" fn(Object) -> Object
                ),
                type_class: sym!(
                    library,
                    "mono_class_from_mono_type",
                    unsafe extern "C" fn(Object) -> Object
                ),
                reflected: sym!(
                    library,
                    "mono_reflection_type_get_type",
                    unsafe extern "C" fn(Object) -> Object
                ),
                image: sym!(
                    library,
                    "mono_class_get_image",
                    unsafe extern "C" fn(Object) -> Object
                ),
                name: sym!(
                    library,
                    "mono_class_get_name",
                    unsafe extern "C" fn(Object) -> *const libc::c_char
                ),
                namespace: sym!(
                    library,
                    "mono_class_get_namespace",
                    unsafe extern "C" fn(Object) -> *const libc::c_char
                ),
                value: sym!(
                    library,
                    "mono_class_is_valuetype",
                    unsafe extern "C" fn(Object) -> i32
                ),
                array_class: sym!(
                    library,
                    "mono_array_class_get",
                    unsafe extern "C" fn(Object, u32) -> Object
                ),
            },
        })
        .map_err(|_| "component queries already installed")?;
    for (name, adapter) in names.iter().zip([
        components as *const (),
        component_list as *const (),
        component as *const (),
        children as *const (),
        parent as *const (),
        count as *const (),
        index as *const (),
        component_name as *const (),
    ]) {
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    Ok(())
}
fn checked<T>(work: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_COMPONENT_QUERY_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_COMPONENT_QUERY_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
fn note(index: usize) {
    CALLS.with(|calls| {
        let mut counts = calls.get();
        if counts[index] == 0 { eprintln!("NATIVE_MONO_COMPONENT_QUERY_VERIFIED: owner={:?}; member={:?}; original-engine=true", SPECS[index].0, SPECS[index].1); }
        counts[index] += 1;
        calls.set(counts);
    });
}
fn binding() -> Result<&'static Binding, String> {
    BINDING
        .get()
        .ok_or_else(|| "component queries absent".into())
}
fn query_type(b: &Binding, ty: Object) -> Result<(u64, Object), String> {
    if ty.is_null() {
        return Err("component query type is NULL".into());
    }
    // The RuntimeType registry checks the object identity before interpreting it.
    let token =
        callbacks::with_metadata_service(|route, memory| route.root_reflection(ty, memory))?;
    let class = unsafe { (b.api.type_class)((b.api.reflected)(ty)) };
    if class.is_null()
        || !b
            .images
            .contains(&(unsafe { (b.api.image)(class) } as usize))
        || unsafe { (b.api.value)(class) } != 0
    {
        return Err("component query type is not an owned reference declaration".into());
    }
    closed_types::Api::load(b.library as Object)?.concrete(class)?;
    Ok((token, class))
}
fn result_list(b: &Binding, list: Object) -> Result<u64, String> {
    if list.is_null() {
        return Ok(0);
    }
    let class = unsafe { (b.api.object_class)(list) };
    let name = unsafe { (b.api.name)(class) };
    let namespace = unsafe { (b.api.namespace)(class) };
    if unsafe { (b.api.image)(class) } as usize != b.corlib
        || name.is_null()
        || namespace.is_null()
        || unsafe { CStr::from_ptr(name) } != c"List`1"
        || unsafe { CStr::from_ptr(namespace) } != c"System.Collections.Generic"
    {
        return Err("component result list is not the owned List<T> declaration".into());
    }
    closed_types::Api::load(b.library as Object)?.constructed(class)?;
    callbacks::with_metadata_service(|route, memory| route.root_container(list, memory))
}
fn bools(values: &[u8]) -> Result<(), String> {
    if values.iter().any(|value| *value > 1) {
        return Err("component query Boolean is not canonical".into());
    }
    Ok(())
}
unsafe extern "C" fn components(
    pointer: usize,
    ty: Object,
    typed: u8,
    recursive: u8,
    inactive: u8,
    reverse: u8,
    list: Object,
) -> Object {
    checked(|| {
        let b = binding()?;
        bools(&[typed, recursive, inactive, reverse])?;
        let pointer = engine_objects::engine_pointer(pointer)?;
        let (ty, class) = query_type(b, ty)?;
        let list = result_list(b, list)?;
        let token = callbacks::guest_arguments(
            b.targets[0],
            &[
                pointer,
                ty,
                typed as u64,
                recursive as u64,
                inactive as u64,
                reverse as u64,
                list,
            ],
        )?;
        let native = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            route.rooted_native(token)
        })?;
        if !native.is_null() {
            let element = if typed == 1 {
                class
            } else {
                b.component as Object
            };
            let array = unsafe { (b.api.array_class)(element, 1) };
            managed_calls::Api::load(b.library as Object)?.compatible(
                native,
                managed_calls::Shape::Reference {
                    class: array,
                    string: false,
                },
            )?;
        } else if list == 0 {
            return Err("component array query returned NULL without a result list".into());
        }
        note(0);
        Ok(native)
    })
}
unsafe extern "C" fn component_list(pointer: usize, ty: Object, list: Object) {
    checked(|| {
        let b = binding()?;
        let pointer = engine_objects::engine_pointer(pointer)?;
        let (ty, _) = query_type(b, ty)?;
        if list.is_null() {
            return Err("component list destination is NULL".into());
        }
        let list = result_list(b, list)?;
        callbacks::guest_arguments(b.targets[1], &[pointer, ty, list])?;
        callbacks::with_metadata_service(|route, memory| route.sync_guest(memory))?;
        note(1);
        Ok(())
    });
}
fn single(which: usize, pointer: usize, ty: Object, inactive: Option<u8>) -> usize {
    checked(|| {
        let b = binding()?;
        if let Some(inactive) = inactive {
            bools(&[inactive])?;
        }
        let pointer = engine_objects::engine_pointer(pointer)?;
        let (ty, _) = query_type(b, ty)?;
        let mut args = vec![pointer, ty];
        args.extend(inactive.map(u64::from));
        let handle = callbacks::guest_arguments(b.targets[which], &args)?;
        let native =
            engine_objects::engine_handle(b.library as Object, b.component as Object, handle)?;
        note(which);
        Ok(native)
    })
}
unsafe extern "C" fn component(pointer: usize, ty: Object) -> usize {
    single(2, pointer, ty, None)
}
unsafe extern "C" fn children(pointer: usize, ty: Object, inactive: u8) -> usize {
    single(3, pointer, ty, Some(inactive))
}
unsafe extern "C" fn parent(pointer: usize, ty: Object, inactive: u8) -> usize {
    single(4, pointer, ty, Some(inactive))
}
unsafe extern "C" fn count(pointer: usize) -> i32 {
    checked(|| {
        let b = binding()?;
        let pointer = engine_objects::engine_pointer(pointer)?;
        let result = callbacks::guest_arguments(b.targets[5], &[pointer])? as i32;
        note(5);
        Ok(result)
    })
}
unsafe extern "C" fn index(pointer: usize, component: usize) -> i32 {
    checked(|| {
        let b = binding()?;
        let pointer = engine_objects::engine_pointer(pointer)?;
        let component = engine_objects::engine_pointer(component)?;
        let result = callbacks::guest_arguments(b.targets[6], &[pointer, component])? as i32;
        note(6);
        Ok(result)
    })
}
unsafe extern "C" fn component_name(pointer: usize, span: *const logging::Span) -> usize {
    checked(|| {
        let b = binding()?;
        let pointer = engine_objects::engine_pointer(pointer)?;
        let units = unsafe { logging::owned_span(span) }?;
        let handle = callbacks::guest_positioned_string_span(
            b.targets[7],
            units.as_deref(),
            &[pointer],
            &[],
        )?;
        let result =
            engine_objects::engine_handle(b.library as Object, b.component as Object, handle)?;
        note(7);
        Ok(result)
    })
}
