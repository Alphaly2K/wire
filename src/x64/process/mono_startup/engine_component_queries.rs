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
    method: unsafe extern "C" fn(Object, *const libc::c_char, i32) -> Object,
    length: unsafe extern "C" fn(Object) -> usize,
    array_address: unsafe extern "C" fn(Object, i32, usize) -> Object,
}
struct Binding {
    library: usize,
    api: Api,
    component: usize,
    corlib: usize,
    targets: [u64; 8],
    images: Vec<usize>,
    component_game_object: u64,
    game_object: usize,
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
            component_game_object: registry.exact_target(
                "UnityEngine.Component::get_gameObject_Injected",
                0x1800bab90,
            )?,
            game_object: class(core, c"UnityEngine", c"GameObject") as usize,
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
                method,
                length: sym!(
                    library,
                    "mono_array_length",
                    unsafe extern "C" fn(Object) -> usize
                ),
                array_address: sym!(
                    library,
                    "mono_array_addr_with_size",
                    unsafe extern "C" fn(Object, i32, usize) -> Object
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
fn result_list(b: &Binding, list: Object) -> Result<Option<List>, String> {
    if list.is_null() {
        return Ok(None);
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
    let api = managed_calls::Api::load(b.library as Object)?;
    let images: Vec<_> = b.images.iter().map(|&image| image as Object).collect();
    let clear = unsafe { (b.api.method)(class, c"Clear".as_ptr(), 0) };
    let add = unsafe { (b.api.method)(class, c"Add".as_ptr(), 1) };
    let clear_desc = api
        .describe_constructed_instance(clear, &images, &images)?
        .ok_or("component List.Clear has no ordinary IL")?;
    let add_desc = api
        .describe_constructed_instance(add, &images, &images)?
        .ok_or("component List.Add has no ordinary IL")?;
    if clear_desc.receiver != Some(class)
        || !clear_desc.parameters.is_empty()
        || clear_desc.returns.is_some()
        || add_desc.receiver != Some(class)
        || add_desc.parameters.len() != 1
        || add_desc.parameters[0].byref
        || add_desc.returns.is_some()
    {
        return Err("component List mutation methods have unexpected metadata".into());
    }
    let element = match add_desc.parameters[0].shape {
        managed_calls::Shape::Reference {
            class,
            string: false,
        } => class,
        _ => return Err("component result List<T> element is not a reference class".into()),
    };
    let pin = callbacks::with_metadata_service(|route, memory| {
        route.root_container(list, memory)?;
        let roots = route
            .objects
            .as_ref()
            .ok_or("component list root service absent")?;
        roots.api.worker_root(list)
    })?;
    Ok(Some(List {
        pin,
        element,
        clear,
        add,
    }))
}
struct List {
    pin: super::instances::WorkerRoot,
    element: Object,
    clear: Object,
    add: Object,
}
fn fill_list(b: &Binding, list: List, array: Object) -> Result<(), String> {
    let api = managed_calls::Api::load(b.library as Object)?;
    let length = unsafe { (b.api.length)(array) };
    if length > 4096 {
        return Err("component result list exceeds bounded traversal".into());
    }
    // Validate the complete batch before clearing the caller's List. Unity's
    // native list helper can assign Component[] to List<Derived>._items; use
    // its actual typed array query and the real List<T> IL instead of allowing
    // that incompatible managed reference into the native VM.
    let values: Vec<_> = (0..length)
        .map(|at| unsafe {
            (b.api.array_address)(array, 8, at)
                .cast::<Object>()
                .read_unaligned()
        })
        .collect();
    let invoke = |method, args: *mut Object| -> Result<(), String> {
        let mut exception = std::ptr::null_mut();
        let value = unsafe { (api.invoke)(method, list.pin.target()?, args, &mut exception) };
        if !exception.is_null() || !value.is_null() {
            return Err("component List mutation IL failed".into());
        }
        Ok(())
    };
    project_list(
        &values,
        |value| {
            api.compatible(
                value,
                managed_calls::Shape::Reference {
                    class: list.element,
                    string: false,
                },
            )
            .map(|_| ())
        },
        || invoke(list.clear, std::ptr::null_mut()),
        |mut value| invoke(list.add, &mut value),
    )?;
    callbacks::with_metadata_service(|route, memory| {
        route.root_container(list.pin.target()?, memory).map(|_| ())
    })
}
fn project_list(
    values: &[Object],
    mut validate: impl FnMut(Object) -> Result<(), String>,
    mut clear: impl FnMut() -> Result<(), String>,
    mut add: impl FnMut(Object) -> Result<(), String>,
) -> Result<(), String> {
    for &value in values {
        validate(value)?;
    }
    clear()?;
    for &value in values {
        add(value)?;
    }
    Ok(())
}
fn array_query(
    b: &Binding,
    pointer: u64,
    ty: u64,
    class: Object,
    flags: [u8; 4],
) -> Result<Object, String> {
    let [typed, recursive, inactive, reverse] = flags;
    let token = callbacks::guest_arguments(
        b.targets[0],
        &[
            pointer,
            ty,
            typed as u64,
            recursive as u64,
            inactive as u64,
            reverse as u64,
            0,
        ],
    )?;
    let native = callbacks::with_metadata_service(|route, memory| {
        route.sync_guest(memory)?;
        route.rooted_native(token)
    })?;
    if native.is_null() {
        return Err("component array query returned NULL without a result list".into());
    }
    let element = if typed == 1 {
        class
    } else {
        b.component as Object
    };
    let array_class = unsafe { (b.api.array_class)(element, 1) };
    managed_calls::Api::load(b.library as Object)?.compatible(
        native,
        managed_calls::Shape::Reference {
            class: array_class,
            string: false,
        },
    )?;
    Ok(native)
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
        let native = array_query(
            b,
            pointer,
            ty,
            class,
            [
                if list.is_some() { 1 } else { typed },
                recursive,
                inactive,
                reverse,
            ],
        )?;
        let native = if let Some(list) = list {
            fill_list(b, list, native)?;
            std::ptr::null_mut()
        } else {
            native
        };
        note(0);
        Ok(native)
    })
}
unsafe extern "C" fn component_list(pointer: usize, ty: Object, list: Object) {
    checked(|| {
        let b = binding()?;
        let pointer = engine_objects::engine_pointer(pointer)?;
        let (ty, class) = query_type(b, ty)?;
        if list.is_null() {
            return Err("component list destination is NULL".into());
        }
        let list = result_list(b, list)?.ok_or("component list destination is NULL")?;
        let handle = callbacks::guest_arguments(b.component_game_object, &[pointer])?;
        let pointer = callbacks::with_metadata_service(|route, memory| {
            route.sync_guest(memory)?;
            let roots = route
                .objects
                .as_mut()
                .ok_or("component GameObject root service absent")?;
            let token = roots.handles.1.target(roots.handles.0, handle)?;
            roots
                .roots
                .compatible(roots.api, token, b.game_object as Object)?;
            roots
                .roots
                .publish_engine_object(roots.api, memory, token)?;
            memory.read_int(token + 16, 8)
        })?;
        let pointer = engine_objects::engine_pointer(pointer as usize)?;
        let array = array_query(b, pointer, ty, class, [1, 0, 0, 0])?;
        fill_list(b, list, array)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn typed_list_projection_rejects_late_mismatch_before_mutation_and_preserves_order() {
        let values = [2usize as Object, std::ptr::null_mut(), 9usize as Object];
        let list = RefCell::new(vec![7usize]);
        let run = |values: &[Object]| {
            project_list(
                values,
                |value| {
                    if value as usize <= 2 {
                        Ok(())
                    } else {
                        Err("incompatible component".into())
                    }
                },
                || {
                    list.borrow_mut().clear();
                    Ok(())
                },
                |value| {
                    list.borrow_mut().push(value as usize);
                    Ok(())
                },
            )
        };
        assert!(run(&values).is_err());
        assert_eq!(*list.borrow(), [7]);
        run(&values[..2]).unwrap();
        assert_eq!(*list.borrow(), [2, 0]);
        run(&[]).unwrap();
        assert!(list.borrow().is_empty());
        let added = Cell::new(0);
        assert!(project_list(
            &values[..1],
            |_| Ok(()),
            || Err("native Clear failed".into()),
            |_| {
                added.set(added.get() + 1);
                Ok(())
            }
        )
        .is_err());
        assert_eq!(added.get(), 0);
    }
}
