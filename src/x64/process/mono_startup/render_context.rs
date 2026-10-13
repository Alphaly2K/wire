//! Synchronous engine render contexts are capabilities of one actual managed
//! render-loop invocation. Native IL carries an opaque identity; original guest
//! pointers are recovered only by context bindings on the same owner thread.
use super::{bindings, callbacks, engine_objects, managed_calls, Object, Probe};
pub(super) mod culling;
use std::{
    cell::RefCell,
    ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
};

const BASE: u64 = 0x6fec00000000;
// Keep issued identities inside this 4GiB arena, below the next resource arena.
const LIMIT: u64 = (1 << 32) / 16;
struct Binding {
    entry: usize,
    context: usize,
    asset: usize,
    object: usize,
    intptr: usize,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Copy)]
struct Context {
    token: u64,
    pointer: u64,
    stack_rsp: Option<u64>,
}
thread_local! { static SCOPES: RefCell<Vec<Context>> = const { RefCell::new(Vec::new()) }; }
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn context_class(class: Object) -> bool {
    BINDING.get().is_some_and(|b| b.context == class as usize)
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
                return Err(concat!("missing render context API: ", $name).into());
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
    let size = sym!(
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let fields = sym!(
        "mono_class_get_fields",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
    let name = sym!(
        "mono_field_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let parent = sym!(
        "mono_field_get_parent",
        unsafe extern "C" fn(Object) -> Object
    );
    let offset = sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> i32);
    let ty = sym!(
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let byref = sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32);
    let type_class = sym!(
        "mono_class_from_mono_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let engine = |name: &std::ffi::CStr| unsafe {
        find(core, c"UnityEngine.Rendering".as_ptr(), name.as_ptr())
    };
    let context = engine(c"ScriptableRenderContext");
    let owner = engine(c"RenderPipelineManager");
    let asset = engine(c"RenderPipelineAsset");
    let object = unsafe { find(core, c"UnityEngine".as_ptr(), c"Object".as_ptr()) };
    let corlib = unsafe { corlib() };
    let intptr = unsafe { find(corlib, c"System".as_ptr(), c"IntPtr".as_ptr()) };
    let void = unsafe { find(corlib, c"System".as_ptr(), c"Void".as_ptr()) };
    if [context, owner, asset, object, intptr, void]
        .iter()
        .any(|p| p.is_null())
    {
        return Err("render context actual metadata classes absent".into());
    }
    let mut alignment = 0;
    if unsafe { size(context, &mut alignment) } != 8 || alignment != 8 {
        return Err("render context requires exact single IntPtr value storage".into());
    }
    let mut iterator = ptr::null_mut();
    let mut seen = false;
    loop {
        let field = unsafe { fields(context, &mut iterator) };
        if field.is_null() {
            break;
        }
        if unsafe { flags(field) } & (0x10 | 0x40) != 0 {
            continue;
        }
        let text = unsafe { name(field) };
        let ty = unsafe { ty(field) };
        if seen
            || text.is_null()
            || ty.is_null()
            || unsafe { parent(field) } != context
            || unsafe { std::ffi::CStr::from_ptr(text) } != c"m_Ptr"
            || unsafe { offset(field) } != 16
            || unsafe { kind(ty) } != 0x18
            || unsafe { byref(ty) } != 0
            || unsafe { type_class(ty) } != intptr
        {
            return Err("render context field declaration differs from capability storage".into());
        }
        seen = true;
    }
    if !seen {
        return Err("render context pointer field absent".into());
    }
    let entry = unsafe { method(owner, c"DoRenderLoop_Internal".as_ptr(), 3) };
    managed_calls::Api::load(library)?.fixed_typed_managed(
        entry,
        owner,
        &[
            (1, void, false),
            (0x12, asset, false),
            (0x18, intptr, false),
            (0x12, object, false),
        ],
        images,
    )?;
    BINDING
        .set(Binding {
            entry: entry as usize,
            context: context as usize,
            asset: asset as usize,
            object: object as usize,
            intptr: intptr as usize,
        })
        .map_err(|_| "render context bridge already installed")?;
    culling::install(library, core, images, registry)
}
fn pointer_shape(b: &Binding) -> managed_calls::Shape {
    managed_calls::Shape::Scalar {
        class: b.intptr as Object,
        width: 8,
        alignment: 8,
        boolean: false,
    }
}
pub(super) fn descriptor(method: Object) -> Option<managed_calls::Descriptor> {
    let b = BINDING.get().filter(|b| b.entry == method as usize)?;
    Some(managed_calls::Descriptor {
        receiver: None,
        parameters: vec![
            managed_calls::Parameter {
                shape: managed_calls::Shape::Reference {
                    class: b.asset as Object,
                    string: false,
                },
                byref: false,
            },
            managed_calls::Parameter {
                shape: pointer_shape(b),
                byref: false,
            },
            managed_calls::Parameter {
                shape: managed_calls::Shape::Reference {
                    class: b.object as Object,
                    string: false,
                },
                byref: false,
            },
        ],
        returns: None,
        name: "UnityEngine.Rendering.RenderPipelineManager::DoRenderLoop_Internal".into(),
    })
}
pub(super) struct Scope {
    token: u64,
    owner: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Scope {
    fn enter(pointer: u64, stack_rsp: Option<u64>) -> Result<Self, String> {
        let serial = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < LIMIT).then_some(n + 1)
            })
            .map_err(|_| "render context identity space exhausted")?;
        let token = BASE + serial * 16;
        SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            if pointer == 0 || scopes.len() >= 8 {
                return Err("render context scope NULL or nesting limit".into());
            }
            scopes.push(Context {
                token,
                pointer,
                stack_rsp,
            });
            Ok(Self {
                token,
                owner: std::marker::PhantomData,
            })
        })
    }
    pub(super) fn prepare(
        method: Object,
        frame: &mut managed_calls::Frame,
        rsp: u64,
    ) -> Result<Option<Self>, String> {
        let Some(b) = BINDING.get().filter(|b| b.entry == method as usize) else {
            return Ok(None);
        };
        let shape = pointer_shape(b);
        let original = frame.scalar_argument(1, shape)?;
        // The native C++ render loop can borrow a stack context. Its lifetime
        // is this suspended original invocation, rather than a heap owner.
        let stack_rsp = callbacks::with_service(|memory, runtime| {
            if original == 0
                || !original.is_multiple_of(8)
                || !Probe::temporary_call_slot(memory, runtime, rsp, original, 8)
            {
                return Err(format!(
                    "render context is outside live original data/borrowed stack: {original:#x}"
                ));
            }
            memory.check(original, 8, crate::x64::memory::READ)?;
            let stack = original >= rsp.saturating_add(8)
                && original >= super::super::STACK
                && original + 8 <= super::super::STACK + 64 * 1024 * 1024;
            Ok(stack.then_some(rsp))
        })?;
        let scope = Self::enter(original, stack_rsp)?;
        frame.replace_scalar_argument(1, shape, scope.token)?;
        Ok(Some(scope))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        culling::retire_context(self.token);
        SCOPES.with(|scopes| {
            if scopes.borrow_mut().pop().map(|p| p.token) != Some(self.token) {
                eprintln!("NATIVE_MONO_RENDER_CONTEXT_REJECT: scope retirement order changed");
                std::process::abort();
            }
        });
    }
}
fn resolve(token: u64) -> Result<Context, String> {
    SCOPES.with(|scopes| {
        scopes
            .borrow()
            .iter()
            .find(|r| r.token == token)
            .copied()
            .ok_or_else(|| {
                "render context capability is absent, retired or on another thread".into()
            })
    })
}
#[cfg(test)]
pub(super) fn test_scope(pointer: u64) -> Result<Scope, String> {
    Scope::enter(pointer, None)
}
#[cfg(test)]
pub(super) fn test_stack_scope(pointer: u64, rsp: u64) -> Result<Scope, String> {
    Scope::enter(pointer, Some(rsp))
}
#[cfg(test)]
pub(super) fn test_token(scope: &Scope) -> u64 {
    scope.token
}
/// This address is an actual compiled Mono value receiver/byref, owned by its
/// caller for this synchronous dispatch. Its contents must be an issued token.
pub(super) unsafe fn native_payload(address: usize) -> Result<u64, String> {
    if address == 0 || !address.is_multiple_of(8) || address.checked_add(8).is_none() {
        return Err("render context native value storage invalid".into());
    }
    let token = unsafe { (address as *const u64).read() };
    let context = resolve(token)?;
    if let Some(rsp) = context.stack_rsp {
        callbacks::with_service(|memory, runtime| {
            if !Probe::temporary_call_slot(memory, runtime, rsp, context.pointer, 8) {
                return Err(
                    "borrowed render context stack is no longer live original storage".into(),
                );
            }
            memory.check(context.pointer, 8, crate::x64::memory::READ)?;
            Ok(context.pointer)
        })
    } else {
        engine_objects::engine_pointer(context.pointer as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_contexts_reject_retirement_foreign_threads_and_pointer_forgery() {
        let first = Scope::enter(0x1000, None).unwrap();
        let token = first.token;
        assert_eq!(resolve(token).unwrap().pointer, 0x1000);
        assert!(resolve(0x1000).is_err());
        assert!(std::thread::spawn(move || resolve(token))
            .join()
            .unwrap()
            .is_err());
        {
            let second = Scope::enter(0x2000, None).unwrap();
            assert_ne!(first.token, second.token);
            assert_eq!(resolve(first.token).unwrap().pointer, 0x1000);
            assert_eq!(resolve(second.token).unwrap().pointer, 0x2000);
        }
        drop(first);
        assert!(resolve(token).is_err());
        let third = Scope::enter(0x1000, None).unwrap();
        assert_ne!(third.token, token);
        assert!(Scope::enter(0, None).is_err());
    }
}
