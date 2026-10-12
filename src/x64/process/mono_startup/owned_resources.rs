//! Owned engine resources share typed, never-reused identities and owner-thread
//! release. Their ordinary methods use the metadata-derived ABI pool. These
//! handles remain separate from UnityEngine.Object and borrowed render contexts.
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
// Semantic lifecycle roles, not method Specs. Actual signatures, fields,
// converter bodies and targets are checked in the loaded metadata/registry.
const POLICIES: &[(&CStr, &CStr, &CStr, &CStr)] = &[
    (
        c"UnityEngine.Rendering",
        c"CommandBuffer",
        c"InitBuffer",
        c"ReleaseBuffer_Injected",
    ),
    (
        c"UnityEngine",
        c"MaterialPropertyBlock",
        c"CreateImpl",
        c"DestroyImpl",
    ),
    (
        c"UnityEngine",
        c"RectOffset",
        c"InternalCreate",
        c"InternalDestroy",
    ),
    (
        c"UnityEngine",
        c"ObjectGUIState",
        c"Internal_Create",
        c"Internal_Destroy",
    ),
    (
        c"UnityEngine",
        c"TextGenerator",
        c"Internal_Create",
        c"Internal_Destroy",
    ),
];
struct Binding {
    class: usize,
    converter: usize,
    create: u64,
    release: u64,
    label: String,
}
static BINDING: OnceLock<Vec<Binding>> = OnceLock::new();
const CREATE_ADAPTERS: [Object; 5] = [
    create_resource::<0> as Object,
    create_resource::<1> as Object,
    create_resource::<2> as Object,
    create_resource::<3> as Object,
    create_resource::<4> as Object,
];
const RELEASE_ADAPTERS: [Object; 5] = [
    release_resource::<0> as Object,
    release_resource::<1> as Object,
    release_resource::<2> as Object,
    release_resource::<3> as Object,
    release_resource::<4> as Object,
];
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
    kind: usize,
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
            return Err("owned resource requires live original engine owner thread".into());
        }
        Ok(())
    }
    fn insert(&mut self, pointer: u64, kind: usize) -> Result<u64, String> {
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
            return Err("owned resource creation ownership/capacity mismatch".into());
        }
        let token = BASE + (self.records.len() as u64 + 1) * 16;
        self.records.insert(
            token,
            Record {
                pointer,
                kind,
                phase: Phase::Live,
            },
        );
        Ok(token)
    }
    fn live(&self, token: u64, kind: usize) -> Result<Record, String> {
        self.owner()?;
        self.records
            .get(&token)
            .copied()
            .filter(|r| !self.closing && r.phase == Phase::Live && r.kind == kind)
            .ok_or_else(|| "owned resource identity unknown, mistyped, queued or released".into())
    }
    fn queue(&mut self, token: u64, kind: usize, finalizing: bool) -> Result<(), String> {
        if !finalizing || self.closed {
            return Err("owned resource release on foreign thread without finalizer scope".into());
        }
        let record = self
            .records
            .get_mut(&token)
            .ok_or("unknown owned resource finalizer")?;
        if record.kind != kind {
            return Err("owned resource finalizer class mismatch".into());
        }
        if self.closing && matches!(record.phase, Phase::Releasing | Phase::Released) {
            return Ok(());
        }
        if record.phase != Phase::Live {
            return Err("duplicate owned resource finalizer".into());
        }
        record.phase = Phase::Queued;
        Ok(())
    }
    fn begin_release(&mut self, token: u64, kind: usize) -> Result<Record, String> {
        self.owner()?;
        let record = self
            .records
            .get_mut(&token)
            .ok_or("unknown owned resource release")?;
        if !matches!(record.phase, Phase::Live | Phase::Queued) || record.kind != kind {
            return Err("owned resource released more than once or mistyped".into());
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
            .ok_or("unknown owned resource release commit")?;
        if record.phase != Phase::Releasing {
            return Err("owned resource release was not started".into());
        }
        record.phase = Phase::Released;
        Ok(())
    }
    fn pending(&self) -> Result<Vec<u64>, String> {
        if self.owner != thread::current().id() {
            return Err("owned resource finalizer drain on foreign owner".into());
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
        .ok_or("owned resource registry absent")?
        .lock()
        .map_err(|_| "owned resource registry poisoned")?;
    action(&mut state)
}
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn class(class: Object) -> bool {
    BINDING
        .get()
        .is_some_and(|bindings| bindings.iter().any(|b| b.class == class as usize))
}
pub(super) fn classes() -> impl Iterator<Item = Object> {
    BINDING
        .get()
        .into_iter()
        .flatten()
        .map(|b| b.class as Object)
}
pub(super) fn converter(source: Object, method: Object) -> bool {
    BINDING.get().is_some_and(|bindings| {
        bindings.iter().any(|b| {
            b.class == source as usize && b.converter != 0 && b.converter == method as usize
        })
    })
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
                return Err(concat!("missing owned resource metadata API: ", $name).into());
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
    let lookup = sym!(
        "mono_lookup_internal_call",
        unsafe extern "C" fn(Object) -> Object
    );
    let corlib = unsafe { corlib() };
    let intptr = unsafe { find(corlib, c"System".as_ptr(), c"IntPtr".as_ptr()) };
    let void = unsafe { find(corlib, c"System".as_ptr(), c"Void".as_ptr()) };
    if [intptr, void].iter().any(|p| p.is_null()) {
        return Err("owned resource primitive metadata absent".into());
    }
    let api = managed_calls::Api::load(library)?;
    let mut discovered = Vec::new();
    let mut registrations = Vec::new();
    for &(namespace, label, factory, destructor) in POLICIES {
        let class = unsafe { find(core, namespace.as_ptr(), label.as_ptr()) };
        if class.is_null() {
            continue;
        }
        let create = unsafe { method(class, factory.as_ptr(), 0) };
        let release = unsafe { method(class, destructor.as_ptr(), 1) };
        if create.is_null() || release.is_null() {
            continue;
        }
        api.fixed_typed_icall(create, class, &[(0x18, intptr, false)], images)?;
        api.fixed_typed_icall(
            release,
            class,
            &[(1, void, false), (0x18, intptr, false)],
            images,
        )?;
        // Existing lifecycle adapters own their own token protocol. Never mix
        // their fields with this registry or replace half of a lifecycle pair.
        if !unsafe { lookup(create) }.is_null() || !unsafe { lookup(release) }.is_null() {
            continue;
        }
        let pointer = unsafe { field(class, c"m_Ptr".as_ptr()) };
        if pointer.is_null() {
            return Err("owned resource actual pointer field absent".into());
        }
        let pointer_type = unsafe { ty(pointer) };
        if unsafe { parent(pointer) } != class
            || unsafe { flags(pointer) } & (0x10 | 0x40) != 0
            || pointer_type.is_null()
            || unsafe { kind(pointer_type) } != 0x18
            || unsafe { byref(pointer_type) } != 0
            || unsafe { type_class(pointer_type) } != intptr
        {
            return Err("owned resource storage differs from actual instance IntPtr".into());
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
                    return Err("ambiguous owned resource marshaller".into());
                }
                marshaller = candidate;
            }
        }
        let converter = if marshaller.is_null() {
            // No external converter provenance is admitted in this case.
            ptr::null_mut()
        } else {
            let converter = unsafe { method(marshaller, c"ConvertToNative".as_ptr(), 1) };
            api.fixed_typed_managed(
                converter,
                marshaller,
                &[(0x18, intptr, false), (0x12, class, false)],
                images,
            )?;
            api.pointer_field_converter(converter, pointer)?;
            converter
        };
        let owner = format!(
            "{}.{}",
            namespace.to_str().unwrap(),
            label.to_str().unwrap()
        );
        let create_name = CString::new(format!("{owner}::{}", factory.to_str().unwrap())).unwrap();
        let release_name =
            CString::new(format!("{owner}::{}", destructor.to_str().unwrap())).unwrap();
        let targets = [create_name.as_c_str(), release_name.as_c_str()].map(|name| {
            registry
                .registered_target(name.to_str().unwrap())?
                .ok_or_else(|| format!("owned resource engine registration absent: {name:?}"))
        });
        let [create_target, release_target] = targets;
        let slot = discovered.len();
        if slot >= CREATE_ADAPTERS.len() {
            return Err("owned resource compiled entry pool exhausted".into());
        }
        discovered.push(Binding {
            class: class as usize,
            converter: converter as usize,
            create: create_target?,
            release: release_target?,
            label: owner,
        });
        registrations.push((create_name, CREATE_ADAPTERS[slot]));
        registrations.push((release_name, RELEASE_ADAPTERS[slot]));
    }
    BINDING
        .set(discovered)
        .map_err(|_| "owned resource bridge already installed")?;
    STATE
        .set(Mutex::new(State::new()))
        .map_err(|_| "owned resource registry already installed")?;
    for (name, adapter) in registrations {
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    Ok(())
}
pub(super) fn pointer(token: usize, class: usize) -> Result<u64, String> {
    let kind = BINDING
        .get()
        .and_then(|bindings| bindings.iter().position(|b| b.class == class))
        .ok_or("owned resource class not installed")?;
    let record = state(|s| s.live(token as u64, kind))?;
    engine_objects::engine_pointer(record.pointer as usize)
}
fn checked<T>(action: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_OWNED_RESOURCE_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_OWNED_RESOURCE_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
unsafe extern "C" fn create_resource<const KIND: usize>() -> usize {
    checked(|| {
        state(|s| {
            s.owner()?;
            if s.closing {
                return Err("owned resource creation during shutdown".into());
            }
            Ok(())
        })?;
        let b = BINDING
            .get()
            .and_then(|b| b.get(KIND))
            .ok_or("owned resource creation absent")?;
        let pointer = callbacks::guest_arguments(b.create, &[])?;
        if pointer == 0 {
            return Ok(0);
        }
        engine_objects::engine_pointer(pointer as usize)?;
        let token = match state(|s| s.insert(pointer, KIND)) {
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
        eprintln!(
            "NATIVE_MONO_OWNED_RESOURCE_CREATED: class={}; token={token:#x}; original-engine=true",
            b.label
        );
        Ok(token as usize)
    })
}
unsafe extern "C" fn release_resource<const KIND: usize>(token: usize) {
    checked(|| {
        if token == 0 {
            return Ok(());
        }
        if !callbacks::has_owner() {
            return state(|s| {
                s.queue(
                    token as u64,
                    KIND,
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
                    .is_some_and(|r| r.kind == KIND && r.phase == Phase::Released))
        })? {
            return Ok(());
        }
        release(token as u64, Some(KIND))
    });
}
fn release(token: u64, expected: Option<usize>) -> Result<(), String> {
    let record = state(|s| {
        let kind = s
            .records
            .get(&token)
            .ok_or("unknown owned resource release")?
            .kind;
        if expected.is_some_and(|expected| expected != kind) {
            return Err("owned resource release class mismatch".into());
        }
        s.begin_release(token, kind)
    })?;
    engine_objects::engine_pointer(record.pointer as usize)?;
    callbacks::guest_arguments(
        BINDING
            .get()
            .and_then(|b| b.get(record.kind))
            .ok_or("owned resource release absent")?
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
        release(token, None)?;
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
        release(token, None)?;
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
            return Err("owned resource cleanup before owner quiescence".into());
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
    state(|s| {
        let mut report = format!("native-owned-resources-created={}\nnative-owned-resources-live={}\nnative-owned-resources-closed={}\n", s.records.len(), s.records.values().filter(|r| r.phase != Phase::Released).count(), s.closed);
        for (class, marker) in [
            ("UnityEngine.Rendering.CommandBuffer", "command-buffers"),
            ("UnityEngine.MaterialPropertyBlock", "material-property-blocks"),
        ] {
            let kind = BINDING.get().unwrap().iter().position(|b| b.label == class);
            let records: Vec<_> = s.records.values().filter(|r| Some(r.kind) == kind).collect();
            report.push_str(&format!("native-{marker}-created={}\nnative-{marker}-live={}\nnative-{marker}-closed={}\n", records.len(), records.iter().filter(|r| r.phase != Phase::Released).count(), s.closed));
        }
        Ok(report)
    }).unwrap()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ownership_rejects_forgery_reuse_foreign_threads_and_duplicate_release() {
        let mut s = State::new();
        let token = s.insert(0x1234, 0).unwrap();
        assert!(s.live(0x1234, 0).is_err());
        assert!(s.live(token, 1).is_err());
        assert!(s.begin_release(token, 1).is_err());
        assert!(s.queue(token, 1, true).is_err());
        assert!(s.insert(0x1234, 0).is_err());
        assert!(s.insert(0x1234, 1).is_err());
        assert!(s.queue(token, 0, false).is_err());
        let mut s = thread::spawn(move || {
            assert!(s.live(token, 0).is_err());
            s.queue(token, 0, true).unwrap();
            assert!(s.queue(token, 0, true).is_err());
            assert!(s.begin_release(token, 0).is_err());
            s
        })
        .join()
        .unwrap();
        assert!(s.live(token, 0).is_err());
        assert_eq!(s.pending().unwrap(), vec![token]);
        assert_eq!(s.begin_release(token, 0).unwrap().pointer, 0x1234);
        assert!(s.begin_release(token, 0).is_err());
        s.released(token).unwrap();
        assert!(s.released(token).is_err());
        assert!(s.live(token, 0).is_err());
        let next = s.insert(0x1234, 1).unwrap();
        assert_ne!(token, next);
        assert!(s.live(next, 0).is_err());
        assert_eq!(s.live(next, 1).unwrap().pointer, 0x1234);
        s.closing = true;
        assert!(s.live(next, 1).is_err());
        assert!(s.insert(0x5678, 1).is_err());
        s.begin_release(next, 1).unwrap();
        s.released(next).unwrap();
        s.queue(next, 1, true).unwrap();
        assert!(s.queue(0xdead, 1, true).is_err());
        s.closed = true;
        assert!(s.pending().unwrap().is_empty());
        assert!(s.queue(next, 1, true).is_err());
        assert!(s.owner().is_err());
    }
}
