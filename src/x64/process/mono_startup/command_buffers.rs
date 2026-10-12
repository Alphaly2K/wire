//! CommandBuffer ownership is separate from UnityEngine.Object and scoped
//! render contexts. Managed m_Ptr holds a never-reused identity, while the
//! original engine owns the actual command storage and its release operation.
use super::{bindings, callbacks, engine_objects, managed_calls, Object};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    ptr,
    sync::{Mutex, OnceLock},
    thread::{self, ThreadId},
};

const BASE: u64 = 0x6fed00000000;
const LIVE_LIMIT: usize = 4096;
const IDENTITY_LIMIT: usize = 65536;
struct Binding {
    class: usize,
    converter: usize,
    create: u64,
    release: u64,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Live,
    Queued,
    Releasing,
    Released,
}
#[derive(Clone, Copy, Debug)]
struct Record {
    pointer: u64,
    phase: Phase,
}
struct State {
    owner: ThreadId,
    records: BTreeMap<u64, Record>,
    closing: bool,
    closed: bool,
}
impl State {
    fn new() -> Self {
        Self {
            owner: thread::current().id(),
            records: BTreeMap::new(),
            closing: false,
            closed: false,
        }
    }
    fn owner(&self) -> Result<(), String> {
        if self.closed || self.owner != thread::current().id() {
            return Err("command buffer requires live original engine owner thread".into());
        }
        Ok(())
    }
    fn insert(&mut self, pointer: u64) -> Result<u64, String> {
        self.owner()?;
        if pointer == 0
            || self.closing
            || self.records.len() >= IDENTITY_LIMIT
            || self
                .records
                .values()
                .filter(|r| r.phase != Phase::Released)
                .count()
                >= LIVE_LIMIT
            || self
                .records
                .values()
                .any(|r| r.pointer == pointer && r.phase != Phase::Released)
        {
            return Err("command buffer creation ownership/capacity mismatch".into());
        }
        let token = BASE + (self.records.len() as u64 + 1) * 16;
        self.records.insert(
            token,
            Record {
                pointer,
                phase: Phase::Live,
            },
        );
        Ok(token)
    }
    fn live(&self, token: u64) -> Result<Record, String> {
        self.owner()?;
        self.records
            .get(&token)
            .copied()
            .filter(|r| !self.closing && r.phase == Phase::Live)
            .ok_or_else(|| "command buffer identity unknown, queued or released".into())
    }
    fn queue(&mut self, token: u64, finalizing: bool) -> Result<(), String> {
        if !finalizing || self.closed {
            return Err("command buffer release on foreign thread without finalizer scope".into());
        }
        let record = self
            .records
            .get_mut(&token)
            .ok_or("unknown command buffer finalizer")?;
        if self.closing && matches!(record.phase, Phase::Releasing | Phase::Released) {
            return Ok(());
        }
        if record.phase != Phase::Live {
            return Err("duplicate command buffer finalizer".into());
        }
        record.phase = Phase::Queued;
        Ok(())
    }
    fn begin_release(&mut self, token: u64) -> Result<Record, String> {
        self.owner()?;
        let record = self
            .records
            .get_mut(&token)
            .ok_or("unknown command buffer release")?;
        if !matches!(record.phase, Phase::Live | Phase::Queued) {
            return Err("command buffer released more than once".into());
        }
        let prior = *record;
        record.phase = Phase::Releasing;
        Ok(prior)
    }
    fn released(&mut self, token: u64) -> Result<(), String> {
        self.owner()?;
        let record = self
            .records
            .get_mut(&token)
            .ok_or("unknown command buffer release commit")?;
        if record.phase != Phase::Releasing {
            return Err("command buffer release was not started".into());
        }
        record.phase = Phase::Released;
        Ok(())
    }
    fn pending(&self) -> Result<Vec<u64>, String> {
        if self.owner != thread::current().id() {
            return Err("command buffer finalizer drain on foreign owner".into());
        }
        if self.closed {
            return Ok(Vec::new());
        }
        Ok(self
            .records
            .iter()
            .filter_map(|(&token, r)| (r.phase == Phase::Queued).then_some(token))
            .collect())
    }
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
fn state<T>(action: impl FnOnce(&mut State) -> Result<T, String>) -> Result<T, String> {
    let mut state = STATE
        .get()
        .ok_or("command buffer registry absent")?
        .lock()
        .map_err(|_| "command buffer registry poisoned")?;
    action(&mut state)
}
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn class(class: Object) -> bool {
    BINDING.get().is_some_and(|b| b.class == class as usize)
}
pub(super) fn converter(source: Object, method: Object) -> bool {
    BINDING
        .get()
        .is_some_and(|b| b.class == source as usize && b.converter == method as usize)
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
                return Err(concat!("missing command buffer metadata API: ", $name).into());
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
    let nested = sym!(
        "mono_class_get_nested_types",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let name = sym!(
        "mono_class_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let field = sym!(
        "mono_class_get_field_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char) -> Object
    );
    let parent = sym!(
        "mono_field_get_parent",
        unsafe extern "C" fn(Object) -> Object
    );
    let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
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
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let class = unsafe {
        find(
            core,
            c"UnityEngine.Rendering".as_ptr(),
            c"CommandBuffer".as_ptr(),
        )
    };
    let corlib = unsafe { corlib() };
    let intptr = unsafe { find(corlib, c"System".as_ptr(), c"IntPtr".as_ptr()) };
    let void = unsafe { find(corlib, c"System".as_ptr(), c"Void".as_ptr()) };
    if [class, intptr, void].iter().any(|p| p.is_null()) {
        return Err("command buffer actual metadata absent".into());
    }
    let pointer = unsafe { field(class, c"m_Ptr".as_ptr()) };
    if pointer.is_null() {
        return Err("command buffer actual pointer field absent".into());
    }
    let pointer_type = unsafe { ty(pointer) };
    if unsafe { parent(pointer) } != class
        || unsafe { flags(pointer) } & (0x10 | 0x40) != 0
        || pointer_type.is_null()
        || unsafe { kind(pointer_type) } != 0x18
        || unsafe { byref(pointer_type) } != 0
        || unsafe { type_class(pointer_type) } != intptr
    {
        return Err("command buffer storage differs from actual instance IntPtr".into());
    }
    let mut iterator = ptr::null_mut();
    let mut marshaller: Object = ptr::null_mut();
    loop {
        let candidate = unsafe { nested(class, &mut iterator) };
        if candidate.is_null() {
            break;
        }
        let text = unsafe { name(candidate) };
        if !text.is_null() && unsafe { CStr::from_ptr(text) } == c"BindingsMarshaller" {
            if !marshaller.is_null() {
                return Err("ambiguous command buffer marshaller".into());
            }
            marshaller = candidate;
        }
    }
    if marshaller.is_null() {
        return Err("command buffer actual marshaller absent".into());
    }
    let converter = unsafe { method(marshaller, c"ConvertToNative".as_ptr(), 1) };
    let api = managed_calls::Api::load(library)?;
    api.fixed_typed_managed(
        converter,
        marshaller,
        &[(0x18, intptr, false), (0x12, class, false)],
        images,
    )?;
    api.pointer_field_converter(converter, pointer)?;
    let create = unsafe { method(class, c"InitBuffer".as_ptr(), 0) };
    let release = unsafe { method(class, c"ReleaseBuffer_Injected".as_ptr(), 1) };
    api.fixed_typed_icall(create, class, &[(0x18, intptr, false)], images)?;
    api.fixed_typed_icall(
        release,
        class,
        &[(1, void, false), (0x18, intptr, false)],
        images,
    )?;
    let create_name = CString::new("UnityEngine.Rendering.CommandBuffer::InitBuffer").unwrap();
    let release_name =
        CString::new("UnityEngine.Rendering.CommandBuffer::ReleaseBuffer_Injected").unwrap();
    let targets = [create_name.as_c_str(), release_name.as_c_str()].map(|name| {
        registry
            .registered_target(name.to_str().unwrap())?
            .ok_or_else(|| format!("command buffer engine registration absent: {name:?}"))
    });
    let [create_target, release_target] = targets;
    BINDING
        .set(Binding {
            class: class as usize,
            converter: converter as usize,
            create: create_target?,
            release: release_target?,
        })
        .map_err(|_| "command buffer bridge already installed")?;
    STATE
        .set(Mutex::new(State::new()))
        .map_err(|_| "command buffer registry already installed")?;
    unsafe {
        add(create_name.as_ptr(), create_buffer as Object);
        add(release_name.as_ptr(), release_buffer as Object);
    }
    Ok(())
}
pub(super) fn pointer(token: usize) -> Result<u64, String> {
    let record = state(|s| s.live(token as u64))?;
    engine_objects::engine_pointer(record.pointer as usize)
}
fn checked<T>(action: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_COMMAND_BUFFER_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_COMMAND_BUFFER_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
unsafe extern "C" fn create_buffer() -> usize {
    checked(|| {
        state(|s| {
            s.owner()?;
            if s.closing {
                return Err("command buffer creation during shutdown".into());
            }
            Ok(())
        })?;
        let b = BINDING.get().ok_or("command buffer creation absent")?;
        let pointer = callbacks::guest_arguments(b.create, &[])?;
        if pointer == 0 {
            return Ok(0);
        }
        engine_objects::engine_pointer(pointer as usize)?;
        let token = match state(|s| s.insert(pointer)) {
            Ok(token) => token,
            Err(error) => {
                // A duplicate engine result belongs to an existing live
                // identity. Releasing it would invalidate that owner.
                if state(|s| {
                    Ok(!s
                        .records
                        .values()
                        .any(|r| r.pointer == pointer && r.phase != Phase::Released))
                })? {
                    callbacks::guest_arguments(b.release, &[pointer])?;
                }
                return Err(error);
            }
        };
        eprintln!("NATIVE_MONO_COMMAND_BUFFER_CREATED: token={token:#x}; original-engine=true");
        Ok(token as usize)
    })
}
unsafe extern "C" fn release_buffer(token: usize) {
    checked(|| {
        if token == 0 {
            return Ok(());
        }
        if !callbacks::has_owner() {
            return state(|s| {
                s.queue(
                    token as u64,
                    super::unsafe_buffers::actual_finalizer_scope(),
                )
            });
        }
        // Quiescence released these exact identities while VM/finalizer routes
        // were still active. Late shutdown callbacks may retire their fields.
        if state(|s| {
            s.owner()?;
            Ok(s.closing
                && s.records
                    .get(&(token as u64))
                    .is_some_and(|r| r.phase == Phase::Released))
        })? {
            return Ok(());
        }
        release(token as u64)
    });
}
fn release(token: u64) -> Result<(), String> {
    let record = state(|s| s.begin_release(token))?;
    engine_objects::engine_pointer(record.pointer as usize)?;
    callbacks::guest_arguments(
        BINDING
            .get()
            .ok_or("command buffer release absent")?
            .release,
        &[record.pointer],
    )?;
    state(|s| s.released(token))
}
pub(super) fn drain_finalizers() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let pending = state(|s| s.pending())?;
    for token in pending {
        release(token)?;
    }
    Ok(())
}
pub(super) fn quiesce() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let live = state(|s| {
        s.owner()?;
        s.closing = true;
        Ok(s.records
            .iter()
            .filter_map(|(&token, r)| {
                matches!(r.phase, Phase::Live | Phase::Queued).then_some(token)
            })
            .collect::<Vec<_>>())
    })?;
    for token in live {
        release(token)?;
    }
    Ok(())
}
pub(super) fn retire_after_vm_cleanup() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    state(|s| {
        s.owner()?;
        if !s.closing || s.records.values().any(|r| r.phase != Phase::Released) {
            return Err("command buffer cleanup before owner quiescence".into());
        }
        s.closed = true;
        Ok(())
    })
}
pub(super) fn report() -> String {
    if !installed() {
        return String::new();
    }
    // Reporting also works after retirement, unlike normal ownership queries.
    state(|s| Ok(format!("native-command-buffers-created={}\nnative-command-buffers-live={}\nnative-command-buffers-closed={}\n", s.records.len(), s.records.values().filter(|r| r.phase != Phase::Released).count(), s.closed))).unwrap()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ownership_rejects_forgery_reuse_foreign_threads_and_duplicate_release() {
        let mut s = State::new();
        let token = s.insert(0x1234).unwrap();
        assert!(s.live(0x1234).is_err());
        assert!(s.insert(0x1234).is_err());
        assert!(s.queue(token, false).is_err());
        let mut s = thread::spawn(move || {
            assert!(s.live(token).is_err());
            s.queue(token, true).unwrap();
            assert!(s.queue(token, true).is_err());
            assert!(s.begin_release(token).is_err());
            s
        })
        .join()
        .unwrap();
        assert!(s.live(token).is_err());
        assert_eq!(s.pending().unwrap(), vec![token]);
        assert_eq!(s.begin_release(token).unwrap().pointer, 0x1234);
        assert!(s.begin_release(token).is_err());
        s.released(token).unwrap();
        assert!(s.released(token).is_err());
        assert!(s.live(token).is_err());
        let next = s.insert(0x1234).unwrap();
        assert_ne!(token, next);
        s.closing = true;
        assert!(s.live(next).is_err());
        assert!(s.insert(0x5678).is_err());
        s.begin_release(next).unwrap();
        s.released(next).unwrap();
        s.queue(next, true).unwrap();
        assert!(s.queue(0xdead, true).is_err());
        s.closed = true;
        assert!(s.pending().unwrap().is_empty());
        assert!(s.queue(next, true).is_err());
        assert!(s.owner().is_err());
    }
}
