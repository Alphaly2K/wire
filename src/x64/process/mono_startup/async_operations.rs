//! Unity async operations retain original engine state behind never-reused
//! identities. Creation, attachment, queries and owner-thread destruction share
//! one lifecycle; no completion state is synthesized by the bridge.
use super::{bindings, callbacks, engine_objects, managed_calls, value_layout, Object};
use crate::x64::memory::{READ, WRITE};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::CStr,
    sync::{Mutex, OnceLock},
    thread::{self, ThreadId},
};

const BASE: u64 = 0x6fea00000000;
const BUFFER_BASE: u64 = 0x6feb00000000;
const LIMIT: usize = 4096;
const BUFFER_LIMIT: usize = 64 * 1024 * 1024;
const METHODS: [(&CStr, bool); 13] = [
    (c"InternalDestroy", false),
    (c"InternalSetManagedObject", false),
    (c"get_isDone_Injected", false),
    (c"get_progress_Injected", false),
    (c"get_priority_Injected", false),
    (c"set_priority_Injected", false),
    (c"get_allowSceneActivation_Injected", false),
    (c"set_allowSceneActivation_Injected", false),
    (c"IsWaitingForSceneActivation_Injected", true),
    (c"WaitForCompletion_Injected", true),
    (c"Cancel_Injected", true),
    (c"get_IntegrationTimeMS", true),
    (c"set_IntegrationTimeMS", true),
];
struct Binding {
    library: usize,
    class: usize,
    instantiate_class: usize,
    pointer_field: usize,
    targets: [u64; 13],
    create: u64,
    field_get: unsafe extern "C" fn(Object, Object, Object),
    field_set: unsafe extern "C" fn(Object, Object, Object),
}
static BINDING: OnceLock<Binding> = OnceLock::new();
thread_local! { static PARAMETERS: RefCell<Option<value_layout::Layout>> = const { RefCell::new(None) }; }
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
    instantiate: bool,
    attachment: Option<u64>,
    phase: Phase,
}
struct State {
    owner: ThreadId,
    records: BTreeMap<u64, Record>,
    buffers: Vec<(u64, u64)>,
    retained: usize,
    closing: bool,
    closed: bool,
    released: usize,
    queued: usize,
    drained: usize,
}
impl State {
    fn new() -> Self {
        Self {
            owner: thread::current().id(),
            records: BTreeMap::new(),
            buffers: Vec::new(),
            retained: 0,
            closing: false,
            closed: false,
            released: 0,
            queued: 0,
            drained: 0,
        }
    }
    fn owner(&self) -> Result<(), String> {
        if self.owner != thread::current().id() || self.closed {
            return Err("async operation requires live guest owner thread".into());
        }
        Ok(())
    }
    fn insert(&mut self, pointer: u64, instantiate: bool) -> Result<u64, String> {
        self.owner()?;
        if pointer == 0
            || self.closing
            || self.records.len() >= LIMIT
            || self
                .records
                .values()
                .any(|r| r.pointer == pointer && r.phase != Phase::Released)
        {
            return Err("async operation duplicate live pointer or capacity exceeded".into());
        }
        let token = BASE + (self.records.len() as u64 + 1) * 16;
        self.records.insert(
            token,
            Record {
                pointer,
                instantiate,
                attachment: None,
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
            .filter(|r| r.phase == Phase::Live)
            .ok_or_else(|| "unknown, queued or released async operation".into())
    }
    fn attach(&mut self, token: u64, object: u64) -> Result<(), String> {
        let record = self.live(token)?;
        if self.closing || object == 0 || record.attachment.is_some() {
            return Err("async operation attachment is absent or already assigned".into());
        }
        self.records.get_mut(&token).unwrap().attachment = Some(object);
        Ok(())
    }
    fn queue(&mut self, token: u64, finalizing: bool) -> Result<(), String> {
        if !finalizing || self.closed {
            return Err("async release lacks live finalizer scope".into());
        }
        let record = self
            .records
            .get_mut(&token)
            .ok_or("unknown async finalizer identity")?;
        // Explicit owner quiescence cleared the real m_Ptr and already released
        // this exact record. A previously entered finalizer may still arrive.
        if self.closing && matches!(record.phase, Phase::Releasing | Phase::Released) {
            return Ok(());
        }
        if record.phase != Phase::Live {
            return Err("duplicate async finalizer release".into());
        }
        record.phase = Phase::Queued;
        self.queued += 1;
        Ok(())
    }
    fn begin_release(&mut self, token: u64) -> Result<Record, String> {
        self.owner()?;
        let record = self
            .records
            .get_mut(&token)
            .ok_or("unknown async release identity")?;
        if !matches!(record.phase, Phase::Live | Phase::Queued) {
            return Err("async operation released more than once".into());
        }
        let prior = *record;
        record.phase = Phase::Releasing;
        Ok(prior)
    }
    fn released(&mut self, token: u64, queued: bool) -> Result<(), String> {
        self.owner()?;
        let record = self
            .records
            .get_mut(&token)
            .ok_or("unknown async release commit")?;
        if record.phase != Phase::Releasing {
            return Err("async release was not started".into());
        }
        record.phase = Phase::Released;
        self.released += 1;
        self.drained += usize::from(queued);
        Ok(())
    }
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
fn state<T>(action: impl FnOnce(&mut State) -> Result<T, String>) -> Result<T, String> {
    let mut guard = STATE
        .get()
        .ok_or("async operation registry absent")?
        .lock()
        .map_err(|_| "async operation registry poisoned")?;
    action(&mut guard)
}
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
                return Err(concat!("missing async operation API: ", $name).into());
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
    let field = sym!(
        "mono_class_get_field_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char) -> Object
    );
    let parent = sym!(
        "mono_field_get_parent",
        unsafe extern "C" fn(Object) -> Object
    );
    let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
    let field_type = sym!(
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
    let unity = |name: &CStr| unsafe { find(core, c"UnityEngine".as_ptr(), name.as_ptr()) };
    let corlib = unsafe { corlib() };
    let system = |name: &CStr| unsafe { find(corlib, c"System".as_ptr(), name.as_ptr()) };
    let class = unity(c"AsyncOperation");
    let instantiate_class = unity(c"AsyncInstantiateOperation");
    let parameters = unity(c"InstantiateParameters");
    let object = unity(c"Object");
    let pointer = (0x18, system(c"IntPtr"), false);
    let void = (1, system(c"Void"), false);
    let boolean = (2, system(c"Boolean"), false);
    let single = (0xc, system(c"Single"), false);
    let integer = (8, system(c"Int32"), false);
    if [
        class,
        instantiate_class,
        parameters,
        object,
        pointer.1,
        void.1,
        boolean.1,
        single.1,
        integer.1,
    ]
    .iter()
    .any(|p| p.is_null())
    {
        return Err("async operation actual metadata absent".into());
    }
    let pointer_field = unsafe { field(class, c"m_Ptr".as_ptr()) };
    if pointer_field.is_null()
        || unsafe { parent(pointer_field) } != class
        || unsafe { flags(pointer_field) } & (0x10 | 0x40) != 0
    {
        return Err("async operation pointer field is not mutable instance storage".into());
    }
    let ty = unsafe { field_type(pointer_field) };
    if ty.is_null()
        || unsafe { kind(ty) } != pointer.0
        || unsafe { byref(ty) } != 0
        || unsafe { type_class(ty) } != pointer.1
    {
        return Err("async operation pointer field type mismatch".into());
    }
    let signatures = [
        vec![void, pointer],
        vec![void, pointer, (0x12, class, false)],
        vec![boolean, pointer],
        vec![single, pointer],
        vec![integer, pointer],
        vec![void, pointer, integer],
        vec![boolean, pointer],
        vec![void, pointer, boolean],
        vec![boolean, pointer],
        vec![void, pointer],
        vec![void, pointer],
        vec![single],
        vec![void, single],
    ];
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; 13];
    let mut names = Vec::new();
    for (index, ((member, instantiate), signature)) in METHODS.iter().zip(&signatures).enumerate() {
        let owner = if *instantiate {
            instantiate_class
        } else {
            class
        };
        api.fixed_typed_icall(
            unsafe { method(owner, member.as_ptr(), (signature.len() - 1) as i32) },
            owner,
            signature,
            images,
        )?;
        let name = format!(
            "UnityEngine.{}::{}",
            if *instantiate {
                "AsyncInstantiateOperation"
            } else {
                "AsyncOperation"
            },
            member.to_str().unwrap()
        );
        targets[index] = registry
            .registered_target(&name)?
            .ok_or_else(|| format!("async operation engine registration absent: {name}"))?;
        names.push(std::ffi::CString::new(name).map_err(|e| e.to_string())?);
    }
    let create_name = c"UnityEngine.Object::Internal_InstantiateAsyncWithParams_Injected";
    api.fixed_typed_icall(
        unsafe {
            method(
                object,
                c"Internal_InstantiateAsyncWithParams_Injected".as_ptr(),
                7,
            )
        },
        object,
        &[
            pointer,
            pointer,
            integer,
            (0x11, parameters, true),
            pointer,
            integer,
            pointer,
            integer,
        ],
        images,
    )?;
    let layout = value_layout::Api::load(library, core)?.layout_in(parameters, images)?;
    // This protocol copies values/references; raw capabilities need another
    // semantic protocol even if they happen to have the same machine width.
    if layout
        .slots
        .iter()
        .any(|s| matches!(s.shape, value_layout::Shape::EngineAddress { .. }))
    {
        return Err("async parameters contain an unsupported native capability".into());
    }
    for (name, width) in [(c"Vector3", 12), (c"Quaternion", 16)] {
        if value_layout::Api::load(library, core)?
            .layout_in(unity(name), images)?
            .scalar_inline()?
            != (width, 4)
        {
            return Err("async position/rotation native element layout mismatch".into());
        }
    }
    let create = registry
        .registered_target(create_name.to_str().unwrap())?
        .ok_or("async instantiate engine registration absent")?;
    BINDING
        .set(Binding {
            library: library as usize,
            class: class as usize,
            instantiate_class: instantiate_class as usize,
            pointer_field: pointer_field as usize,
            targets,
            create,
            field_get: sym!(
                "mono_field_get_value",
                unsafe extern "C" fn(Object, Object, Object)
            ),
            field_set: sym!(
                "mono_field_set_value",
                unsafe extern "C" fn(Object, Object, Object)
            ),
        })
        .map_err(|_| "async operation bridge already installed")?;
    STATE
        .set(Mutex::new(State::new()))
        .map_err(|_| "async registry already installed")?;
    PARAMETERS.with(|slot| *slot.borrow_mut() = Some(layout));
    for (name, adapter) in names.iter().zip([
        destroy as *const (),
        attach as *const (),
        done as *const (),
        progress as *const (),
        priority as *const (),
        set_priority as *const (),
        activation as *const (),
        set_activation as *const (),
        waiting as *const (),
        wait as *const (),
        cancel as *const (),
        integration as *const (),
        set_integration as *const (),
    ]) {
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    unsafe { add(create_name.as_ptr(), create_as_adapter()) };
    Ok(())
}
fn create_as_adapter() -> Object {
    instantiate as *const () as Object
}
fn checked<T>(action: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_ASYNC_OPERATION_REJECT: {error}");
            std::process::abort();
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_ASYNC_OPERATION_REJECT: adapter panicked");
            std::process::abort();
        }
    }
}
pub(super) fn adopt(pointer: u64, instantiate: bool) -> Result<usize, String> {
    if pointer == 0 {
        return Ok(0);
    }
    engine_objects::engine_pointer(pointer as usize)?;
    state(|s| s.insert(pointer, instantiate)).map(|token| token as usize)
}
pub(super) fn pointer(token: usize) -> Result<u64, String> {
    let record = state(|s| s.live(token as u64))?;
    engine_objects::engine_pointer(record.pointer as usize)
}
unsafe extern "C" fn attach(token: usize, object: Object) {
    checked(|| {
        let b = BINDING.get().ok_or("async attachment absent")?;
        let record = state(|s| s.live(token as u64))?;
        let class = if record.instantiate {
            b.instantiate_class
        } else {
            b.class
        };
        managed_calls::Api::load(b.library as Object)?.compatible(
            object,
            managed_calls::Shape::Reference {
                class: class as Object,
                string: false,
            },
        )?;
        if object.is_null() {
            return Err("async attachment object is NULL".into());
        }
        let object_token =
            callbacks::with_metadata_service(|route, memory| route.root_reference(object, memory))?;
        state(|s| s.attach(token as u64, object_token))?;
        callbacks::guest_arguments(b.targets[1], &[pointer(token)?, object_token])?;
        eprintln!(
            "NATIVE_MONO_ASYNC_OPERATION_ATTACHED: identity={token:#x}; original-engine=true"
        );
        Ok(())
    });
}
fn query(index: usize, token: usize, tail: &[u64]) -> (u64, u64) {
    checked(|| {
        let b = BINDING.get().ok_or("async query absent")?;
        let record = state(|s| s.live(token as u64))?;
        if (8..=10).contains(&index) && !record.instantiate {
            return Err("instantiate query on another async operation".into());
        }
        let mut args = vec![pointer(token)?];
        args.extend_from_slice(tail);
        let value = callbacks::guest_scalars(b.targets[index], &args, &[])?;
        if matches!(index, 2 | 6 | 8) && value.0 & 0xff > 1 {
            return Err("async engine Boolean is not canonical".into());
        }
        Ok(value)
    })
}
unsafe extern "C" fn done(t: usize) -> u8 {
    query(2, t, &[]).0 as u8
}
unsafe extern "C" fn progress(t: usize) -> f32 {
    f32::from_bits(query(3, t, &[]).1 as u32)
}
unsafe extern "C" fn priority(t: usize) -> i32 {
    query(4, t, &[]).0 as i32
}
unsafe extern "C" fn set_priority(t: usize, v: i32) {
    query(5, t, &[v as u32 as u64]);
}
unsafe extern "C" fn activation(t: usize) -> u8 {
    query(6, t, &[]).0 as u8
}
unsafe extern "C" fn set_activation(t: usize, v: u8) {
    checked(|| {
        if v <= 1 {
            query(7, t, &[v as u64]);
            Ok(())
        } else {
            Err("async activation Boolean is not canonical".into())
        }
    });
}
unsafe extern "C" fn waiting(t: usize) -> u8 {
    query(8, t, &[]).0 as u8
}
unsafe extern "C" fn wait(t: usize) {
    query(9, t, &[]);
}
unsafe extern "C" fn cancel(t: usize) {
    query(10, t, &[]);
}
unsafe extern "C" fn integration() -> f32 {
    checked(|| {
        Ok(f32::from_bits(
            callbacks::guest_scalars(
                BINDING.get().ok_or("async integration absent")?.targets[11],
                &[],
                &[],
            )?
            .1 as u32,
        ))
    })
}
unsafe extern "C" fn set_integration(v: f32) {
    checked(|| {
        callbacks::guest_scalars(
            BINDING.get().ok_or("async integration absent")?.targets[12],
            &[0],
            &[(0, v.to_bits() as u64)],
        )?;
        Ok(())
    });
}
unsafe extern "C" fn destroy(token: usize) {
    checked(|| {
        // The original engine defines InternalDestroy(NULL) as a no-op.
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
        release(token as u64, false)
    });
}
fn release(token: u64, cancel_pending: bool) -> Result<(), String> {
    let b = BINDING.get().ok_or("async release absent")?;
    let record = state(|s| s.begin_release(token))?;
    engine_objects::engine_pointer(record.pointer as usize)?;
    if cancel_pending && record.instantiate {
        let done = callbacks::guest_arguments(b.targets[2], &[record.pointer])? as u8;
        if done > 1 {
            return Err("async close Boolean is not canonical".into());
        }
        if done == 0 {
            callbacks::guest_arguments(b.targets[10], &[record.pointer])?;
        }
    }
    if let Some(object) = record.attachment {
        callbacks::with_metadata_service(|route, _| {
            let native = route.rooted_native(object)?;
            if native.is_null() {
                return Err("async attached root lost target".into());
            }
            let mut actual = 0usize;
            unsafe {
                (b.field_get)(
                    native,
                    b.pointer_field as Object,
                    (&mut actual as *mut usize).cast(),
                )
            };
            if actual != 0 && actual as u64 != token {
                return Err("async managed pointer identity changed".into());
            }
            let mut zero = 0usize;
            unsafe {
                (b.field_set)(
                    native,
                    b.pointer_field as Object,
                    (&mut zero as *mut usize).cast(),
                )
            };
            Ok(())
        })?;
    }
    // This may free the original engine's transferred Mono handle; its normal
    // metadata/handle route must still be active here.
    callbacks::guest_arguments(b.targets[0], &[record.pointer])?;
    state(|s| s.released(token, record.phase == Phase::Queued))
}
pub(super) fn drain_finalizers() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let pending = state(|s| {
        s.owner()?;
        Ok(s.records
            .iter()
            .filter_map(|(&t, r)| (r.phase == Phase::Queued).then_some(t))
            .collect::<Vec<_>>())
    })?;
    for token in pending {
        release(token, false)?;
    }
    Ok(())
}
/// Stop producers and destroy attachments before the handle registry/VM retires.
pub(super) fn quiesce() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let tokens = state(|s| {
        s.owner()?;
        s.closing = true;
        Ok(s.records
            .iter()
            .filter_map(|(&t, r)| matches!(r.phase, Phase::Live | Phase::Queued).then_some(t))
            .collect::<Vec<_>>())
    })?;
    for token in tokens {
        release(token, true)?;
    }
    Ok(())
}
/// Retained input snapshots survive pending engine work and all VM shutdown
/// callbacks. This provider prohibits guest execution after this final step.
pub(super) fn retire_after_vm_cleanup() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let buffers = state(|s| {
        s.owner()?;
        if !s.closing || s.records.values().any(|r| r.phase != Phase::Released) {
            return Err("async cleanup before owner quiescence".into());
        }
        Ok(s.buffers.clone())
    })?;
    callbacks::with_service(|memory, _| {
        for (base, size) in buffers {
            memory.unmap(base, size)?;
        }
        Ok(())
    })?;
    state(|s| {
        s.buffers.clear();
        s.retained = 0;
        s.closed = true;
        Ok(())
    })
}
/// Copy scalar native ReadOnlySpan inputs into owned guest mappings. These may
/// come from actual IL locals, so UnsafeUtility allocation membership is not
/// required. Storage remains bounded and survives the asynchronous call.
unsafe fn snapshot(address: usize, count: i32, width: usize) -> Result<u64, String> {
    if !(0..=65536).contains(&count) {
        return Err("async native span count exceeds bound".into());
    }
    let size = count as usize * width;
    if size == 0 {
        return Ok(0);
    }
    if address == 0 || !address.is_multiple_of(4) || address.checked_add(size).is_none() {
        return Err("async native span pointer invalid".into());
    }
    let mapped = (size as u64 + 4095) & !4095;
    let base = state(|s| {
        s.owner()?;
        if s.closing
            || s.buffers.len() >= LIMIT
            || s.retained
                .checked_add(mapped as usize)
                .is_none_or(|n| n > BUFFER_LIMIT)
        {
            return Err("async retained snapshot capacity exceeded".into());
        }
        Ok(BUFFER_BASE + s.retained as u64)
    })?;
    let bytes = unsafe { std::slice::from_raw_parts(address as *const u8, size) };
    callbacks::with_service(|memory, _| {
        memory.map(base, mapped, READ | WRITE)?;
        if let Err(error) = memory.write(base, bytes) {
            memory.unmap(base, mapped)?;
            return Err(error);
        }
        Ok(())
    })?;
    state(|s| {
        s.buffers.push((base, mapped));
        s.retained += mapped as usize;
        Ok(())
    })?;
    Ok(base)
}
unsafe extern "C" fn instantiate(
    original: usize,
    count: i32,
    parameters: *const u8,
    positions: usize,
    positions_count: i32,
    rotations: usize,
    rotations_count: i32,
) -> usize {
    checked(|| {
        let b = BINDING.get().ok_or("async creation absent")?;
        state(|s| {
            s.owner()?;
            if s.closing || s.records.len() >= LIMIT {
                return Err("async creation after quiescence or capacity exceeded".into());
            }
            Ok(())
        })?;
        let original = engine_objects::engine_pointer(original)?;
        if !(1..=65536).contains(&count) {
            return Err("async instantiate count exceeds bound".into());
        }
        let mut pins = Vec::new();
        let payload = PARAMETERS.with(|slot| -> Result<Vec<u8>, String> {
            let layout = slot.borrow();
            unsafe {
                layout
                    .as_ref()
                    .ok_or("async parameter layout absent")?
                    .encode_native(parameters as usize, |native, class| {
                        managed_calls::Api::load(b.library as Object)?.compatible(
                            native,
                            managed_calls::Shape::Reference {
                                class,
                                string: false,
                            },
                        )?;
                        if native.is_null() {
                            return Ok(0);
                        }
                        callbacks::with_metadata_service(|route, memory| {
                            let token = route.root_reference(native, memory)?;
                            let roots = route
                                .objects
                                .as_mut()
                                .ok_or("async parameter roots absent")?;
                            roots
                                .roots
                                .publish_engine_object(roots.api, memory, token)?;
                            pins.push(roots.api.worker_root(native)?);
                            Ok(token)
                        })
                    })
            }
        })?;
        let positions = unsafe { snapshot(positions, positions_count, 12) }?;
        let rotations = unsafe { snapshot(rotations, rotations_count, 16) }?;
        let args = [
            original,
            count as u64,
            0,
            positions,
            positions_count as u64,
            rotations,
            rotations_count as u64,
        ];
        let (result, output) = callbacks::guest_byte_arguments(b.create, &args, &[(2, &payload)])?;
        if output != [payload] {
            return Err("async engine modified readonly instantiate parameters".into());
        }
        for pin in pins {
            pin.target()?;
        }
        let token = adopt(result.0, true)?;
        eprintln!("NATIVE_MONO_ASYNC_OPERATION_CREATED: identity={token:#x}; count={count}; original-engine=true");
        Ok(token)
    })
}
pub(super) fn report() -> String {
    if !installed() {
        return String::new();
    }
    state(|s| Ok(format!("native-async-operations-created={}\nnative-async-operations-released={}\nnative-async-operations-live={}\nnative-async-operations-finalizers-queued={}\nnative-async-operations-finalizers-drained={}\nnative-async-operations-retained-bytes={}\nnative-async-operations-closed={}\n",
        s.records.len(), s.released, s.records.values().filter(|r| r.phase != Phase::Released).count(), s.queued, s.drained, s.retained, s.closed))).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_attachment_finalizer_and_quiescence_preserve_exactly_once_release() {
        let mut s = State::new();
        let first = s.insert(0x4000, true).unwrap();
        assert!(s.insert(0x4000, false).is_err());
        assert!(s.live(first + 1).is_err());
        s.attach(first, 0x8000).unwrap();
        assert!(s.attach(first, 0x8000).is_err());
        assert!(s.queue(first, false).is_err());
        s = thread::spawn(move || {
            assert!(s.live(first).is_err());
            s.queue(first, true).unwrap();
            s
        })
        .join()
        .unwrap();
        assert!(s.live(first).is_err());
        assert!(s.queue(first, true).is_err());
        let r = s.begin_release(first).unwrap();
        assert_eq!(
            (r.pointer, r.attachment, r.phase),
            (0x4000, Some(0x8000), Phase::Queued)
        );
        assert!(s.begin_release(first).is_err());
        s.released(first, true).unwrap();
        assert!(s.released(first, true).is_err());
        let second = s.insert(0x4000, false).unwrap();
        assert_ne!(first, second);
        assert!(s.live(first).is_err());
        s.closing = true;
        assert!(s.insert(0x5000, false).is_err());
        s.queue(first, true).unwrap();
        assert!(s.queue(first + 1, true).is_err());
        s.begin_release(second).unwrap();
        s.released(second, false).unwrap();
        assert_eq!((s.released, s.queued, s.drained), (2, 1, 1));
        s.closed = true;
        assert!(s.queue(first, true).is_err());
    }
}
