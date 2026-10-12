//! Fixed component/GameObject state ABIs. All signatures and original engine
//! targets are validated together; no managed object pointer enters Win64.
use super::{bindings, callbacks, engine_objects, managed_calls, Object};
use std::{
    cell::Cell,
    ffi::{CStr, CString},
    sync::OnceLock,
};

#[derive(Clone, Copy)]
enum Shape {
    Boolean,
    SetBoolean,
    Integer,
    SetInteger,
    Handle(&'static CStr),
}
struct Spec {
    owner: &'static CStr,
    member: &'static CStr,
    target: u64,
    shape: Shape,
}
const SPECS: [Spec; 17] = [
    Spec {
        owner: c"Behaviour",
        member: c"get_enabled_Injected",
        target: 0x18008e390,
        shape: Shape::Boolean,
    },
    Spec {
        owner: c"Behaviour",
        member: c"set_enabled_Injected",
        target: 0x18008e3a0,
        shape: Shape::SetBoolean,
    },
    Spec {
        owner: c"Behaviour",
        member: c"get_isActiveAndEnabled_Injected",
        target: 0x1800ba980,
        shape: Shape::Boolean,
    },
    Spec {
        owner: c"GameObject",
        member: c"get_activeInHierarchy_Injected",
        target: 0x1800bd580,
        shape: Shape::Boolean,
    },
    Spec {
        owner: c"GameObject",
        member: c"get_activeSelf_Injected",
        target: 0x1800bd670,
        shape: Shape::Boolean,
    },
    Spec {
        owner: c"GameObject",
        member: c"SetActive_Injected",
        target: 0x1800bd590,
        shape: Shape::SetBoolean,
    },
    Spec {
        owner: c"GameObject",
        member: c"get_active_Injected",
        target: 0x1800bd580,
        shape: Shape::Boolean,
    },
    Spec {
        owner: c"GameObject",
        member: c"set_active_Injected",
        target: 0x1800bd590,
        shape: Shape::SetBoolean,
    },
    Spec {
        owner: c"GameObject",
        member: c"get_layer_Injected",
        target: 0x180078c60,
        shape: Shape::Integer,
    },
    Spec {
        owner: c"GameObject",
        member: c"set_layer_Injected",
        target: 0x1800bd570,
        shape: Shape::SetInteger,
    },
    Spec {
        owner: c"Component",
        member: c"GetComponentIndex_Injected",
        target: 0x1800bb150,
        shape: Shape::Integer,
    },
    Spec {
        owner: c"Component",
        member: c"get_gameObject_Injected",
        target: 0x1800bab90,
        shape: Shape::Handle(c"GameObject"),
    },
    Spec {
        owner: c"Component",
        member: c"get_transform_Injected",
        target: 0x1800ba990,
        shape: Shape::Handle(c"Transform"),
    },
    Spec {
        owner: c"GameObject",
        member: c"get_transform_Injected",
        target: 0x1800bd490,
        shape: Shape::Handle(c"Transform"),
    },
    Spec {
        owner: c"GameObject",
        member: c"get_isStatic_Injected",
        target: 0x18005fed0,
        shape: Shape::Boolean,
    },
    Spec {
        owner: c"GameObject",
        member: c"set_isStatic_Injected",
        target: 0x18005fee0,
        shape: Shape::SetBoolean,
    },
    Spec {
        owner: c"GameObject",
        member: c"get_isStaticBatchable_Injected",
        target: 0x18005fed0,
        shape: Shape::Boolean,
    },
];
struct Binding {
    library: usize,
    targets: [u64; 17],
    returns: [usize; 17],
}
static BINDING: OnceLock<Binding> = OnceLock::new();
thread_local! { static CALLS: Cell<[usize; 17]> = const { Cell::new([0; 17]) }; }
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn report() -> String {
    format!(
        "native-component-state-calls={}\n",
        CALLS.with(|calls| calls.get().iter().sum::<usize>())
    )
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
                return Err(concat!("missing component API: ", $name).into());
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
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let add = sym!(
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
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; 17];
    let mut returns = [0; 17];
    let mut names = Vec::new();
    for (index, spec) in SPECS.iter().enumerate() {
        let owner = class(core, c"UnityEngine", spec.owner);
        let signature = match spec.shape {
            Shape::Boolean => vec![boolean, pointer],
            Shape::SetBoolean => vec![void, pointer, boolean],
            Shape::Integer => vec![integer, pointer],
            Shape::SetInteger => vec![void, pointer, integer],
            Shape::Handle(name) => {
                returns[index] = class(core, c"UnityEngine", name) as usize;
                if returns[index] == 0 {
                    return Err("component result class missing".into());
                }
                vec![pointer, pointer]
            }
        };
        let name = CString::new(format!(
            "UnityEngine.{}::{}",
            spec.owner.to_str().unwrap(),
            spec.member.to_str().unwrap()
        ))
        .unwrap();
        api.fixed_typed_icall(
            unsafe { method(owner, spec.member.as_ptr(), (signature.len() - 1) as i32) },
            owner,
            &signature,
            images,
        )
        .map_err(|error| format!("{}: {error}", name.to_string_lossy()))?;
        targets[index] = registry.exact_target(name.to_str().unwrap(), spec.target)?;
        names.push(name);
    }
    BINDING
        .set(Binding {
            library: library as usize,
            targets,
            returns,
        })
        .map_err(|_| "component bindings already installed")?;
    for (name, adapter) in names.iter().zip([
        enabled as *const (),
        set_enabled as *const (),
        active_enabled as *const (),
        active_hierarchy as *const (),
        active_self as *const (),
        set_active as *const (),
        active as *const (),
        set_active_legacy as *const (),
        layer as *const (),
        set_layer as *const (),
        component_index as *const (),
        game_object as *const (),
        component_transform as *const (),
        game_object_transform as *const (),
        is_static as *const (),
        set_static as *const (),
        static_batchable as *const (),
    ]) {
        unsafe {
            add(name.as_ptr(), adapter as Object);
        }
    }
    Ok(())
}
fn call(index: usize, pointer: usize, argument: Option<u64>) -> u64 {
    let result = std::panic::catch_unwind(|| -> Result<u64, String> {
        let b = BINDING.get().ok_or("component bindings absent")?;
        let pointer = engine_objects::engine_pointer(pointer)?;
        if matches!(SPECS[index].shape, Shape::SetBoolean) && argument.is_none_or(|value| value > 1)
        {
            return Err("component Boolean is not canonical".into());
        }
        let mut args = vec![pointer];
        args.extend(argument);
        let result = callbacks::guest_arguments(b.targets[index], &args)?;
        let value = match SPECS[index].shape {
            Shape::Boolean => {
                let value = result & 255;
                if value > 1 {
                    return Err("engine component Boolean is not canonical".into());
                }
                value
            }
            Shape::Handle(_) => engine_objects::engine_handle(
                b.library as Object,
                b.returns[index] as Object,
                result,
            )? as u64,
            _ => result,
        };
        CALLS.with(|calls| {
            let mut counts = calls.get();
            if counts[index] == 0 { eprintln!("NATIVE_MONO_COMPONENT_STATE_VERIFIED: owner={:?}; member={:?}; original-engine=true", SPECS[index].owner, SPECS[index].member); }
            counts[index] += 1; calls.set(counts);
        });
        Ok(value)
    });
    match result {
        Ok(Ok(value)) => value,
        other => {
            eprintln!(
                "NATIVE_MONO_COMPONENT_STATE_REJECT: member={:?}; {other:?}",
                SPECS[index].member
            );
            std::process::abort()
        }
    }
}
macro_rules! boolean_getter {
    ($name:ident, $index:literal) => {
        unsafe extern "C" fn $name(pointer: usize) -> u8 {
            call($index, pointer, None) as u8
        }
    };
}
macro_rules! boolean_setter {
    ($name:ident, $index:literal) => {
        unsafe extern "C" fn $name(pointer: usize, value: u8) {
            call($index, pointer, Some(value as u64));
        }
    };
}
macro_rules! integer_getter {
    ($name:ident, $index:literal) => {
        unsafe extern "C" fn $name(pointer: usize) -> i32 {
            call($index, pointer, None) as i32
        }
    };
}
macro_rules! handle_getter {
    ($name:ident, $index:literal) => {
        unsafe extern "C" fn $name(pointer: usize) -> usize {
            call($index, pointer, None) as usize
        }
    };
}
boolean_getter!(enabled, 0);
boolean_setter!(set_enabled, 1);
boolean_getter!(active_enabled, 2);
boolean_getter!(active_hierarchy, 3);
boolean_getter!(active_self, 4);
boolean_setter!(set_active, 5);
boolean_getter!(active, 6);
boolean_setter!(set_active_legacy, 7);
integer_getter!(layer, 8);
unsafe extern "C" fn set_layer(pointer: usize, value: i32) {
    call(9, pointer, Some(value as u32 as u64));
}
integer_getter!(component_index, 10);
handle_getter!(game_object, 11);
handle_getter!(component_transform, 12);
handle_getter!(game_object_transform, 13);
boolean_getter!(is_static, 14);
boolean_setter!(set_static, 15);
boolean_getter!(static_batchable, 16);
