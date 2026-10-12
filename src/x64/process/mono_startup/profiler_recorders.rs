//! Fixed ProfilerRecorder adapters. Engine handles and samples stay guest
//! scalars; declared byref bytes use private guest slots, never native addresses.
use super::{animation, bindings, callbacks, managed_calls, value_layout, Object};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{CStr, CString},
    ptr,
    sync::{Mutex, OnceLock},
    thread::{self, ThreadId},
};
const NAMES: [(&str, u64); 13] = [
    ("Create_Injected", 0x180060710),
    ("Control_Injected", 0x180060780),
    ("GetValueUnitType_Injected", 0x1800608c0),
    ("GetValueDataType_Injected", 0x1800608f0),
    ("GetCurrentValue_Injected", 0x180060920),
    ("GetCurrentValueAsDouble_Injected", 0x180060930),
    ("GetLastValue_Injected", 0x180060940),
    ("GetLastValueAsDouble_Injected", 0x180060950),
    ("GetCount_Injected", 0x180060960),
    ("GetValid_Injected", 0x180060980),
    ("GetWrapped_Injected", 0x1800609b0),
    ("GetRunning_Injected", 0x1800609c0),
    ("GetSampleInternal_Injected", 0x1800609d0),
];
static TARGETS: OnceLock<[u64; 13]> = OnceLock::new();
static DESCRIPTION_TARGET: OnceLock<u64> = OnceLock::new();
#[derive(Debug)]
struct Record {
    references: usize,
    queued: usize,
    shared: bool,
}
struct State {
    owner: ThreadId,
    live: BTreeMap<u64, Record>,
    calls: usize,
    created: usize,
    released: usize,
    queued: usize,
    drained: usize,
    closed: bool,
}
impl State {
    fn new() -> Self {
        Self {
            owner: thread::current().id(),
            live: BTreeMap::new(),
            calls: 0,
            created: 0,
            released: 0,
            queued: 0,
            drained: 0,
            closed: false,
        }
    }
    fn register(&mut self, handle: u64, shared: bool) -> Result<(), String> {
        if self.owner != thread::current().id()
            || self.closed
            || handle == 0
            || self.live.values().map(|r| r.references).sum::<usize>() >= 1024
        {
            return Err("recorder registration owner/capacity mismatch".into());
        }
        if let Some(record) = self.live.get_mut(&handle) {
            if !shared || !record.shared || record.queued != 0 {
                return Err("recorder duplicated a nonshared/pending ownership".into());
            }
            record.references += 1;
        } else {
            self.live.insert(
                handle,
                Record {
                    references: 1,
                    queued: 0,
                    shared,
                },
            );
        }
        self.created += 1;
        Ok(())
    }
    fn queue(&mut self, handle: u64, finalizing: bool) -> Result<(), String> {
        if self.owner == thread::current().id() || self.closed || !finalizing {
            return Err("foreign recorder release requires actual Mono finalizer scope".into());
        }
        let r = self
            .live
            .get_mut(&handle)
            .filter(|r| r.references > r.queued)
            .ok_or("recorder finalizer release has no live reference")?;
        r.queued += 1;
        self.queued += 1;
        Ok(())
    }
    fn begin_release(&mut self, handle: u64, queued: bool) -> Result<(), String> {
        if self.owner != thread::current().id() || self.closed {
            return Err("recorder release outside owning thread".into());
        }
        let r = self
            .live
            .get_mut(&handle)
            .filter(|r| {
                if queued {
                    r.queued > 0
                } else {
                    r.references > r.queued
                }
            })
            .ok_or("recorder release is duplicate, pending or retired")?;
        r.references -= 1;
        r.queued -= usize::from(queued);
        if r.references == 0 {
            self.live.remove(&handle);
        }
        Ok(())
    }
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
// Description names are owned native bytes, private to IL, retained until the
// VM has joined its finalizers. Their addresses never enter guest execution.
thread_local! { static NAMES_UTF8: RefCell<BTreeMap<u64, Box<[u8]>>> = const { RefCell::new(BTreeMap::new()) }; }
fn state<T>(operation: impl FnOnce(&mut State) -> Result<T, String>) -> Result<T, String> {
    let mut state = STATE
        .get()
        .ok_or("recorder ownership registry absent")?
        .lock()
        .map_err(|_| "recorder ownership registry poisoned")?;
    operation(&mut state)
}
pub(super) fn installed() -> bool {
    TARGETS.get().is_some()
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
                return Err(concat!("missing recorder API: ", $name).into());
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
    let nested = sym!(
        "mono_class_get_nested_types",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let class_name = sym!(
        "mono_class_get_name",
        unsafe extern "C" fn(Object) -> *const std::ffi::c_char
    );
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let corlib = unsafe { corlib() };
    if !images.contains(&core) || !images.contains(&corlib) {
        return Err("recorder images are not retained".into());
    }
    let class = |ns: &CStr, name: &CStr| unsafe {
        find(
            if ns == c"System" { corlib } else { core },
            ns.as_ptr(),
            name.as_ptr(),
        )
    };
    let recorder = class(c"Unity.Profiling", c"ProfilerRecorder");
    let stat = class(
        c"Unity.Profiling.LowLevel.Unsafe",
        c"ProfilerRecorderHandle",
    );
    let sample = class(c"Unity.Profiling", c"ProfilerRecorderSample");
    for (cls, size) in [(recorder, 8), (stat, 8), (sample, 24)] {
        if cls.is_null()
            || value_layout::Api::load(library, core)?
                .layout(cls)?
                .scalar_inline()?
                != (size, 8)
        {
            return Err("recorder declared scalar layout mismatch".into());
        }
    }
    let mut control = ptr::null_mut();
    let mut count = ptr::null_mut();
    let mut iterator = ptr::null_mut();
    for _ in 0..16 {
        let cls = unsafe { nested(recorder, &mut iterator) };
        if cls.is_null() {
            break;
        }
        let name = unsafe { class_name(cls) };
        if name.is_null() {
            return Err("recorder nested enum name absent".into());
        }
        match unsafe { CStr::from_ptr(name) }.to_bytes() {
            b"ControlOptions" => control = cls,
            b"CountOptions" => count = cls,
            _ => {}
        }
    }
    if control.is_null() || count.is_null() {
        return Err("recorder control/count enum absent".into());
    }
    let void = class(c"System", c"Void");
    let int = class(c"System", c"Int32");
    let long = class(c"System", c"Int64");
    let double = class(c"System", c"Double");
    let boolean = class(c"System", c"Boolean");
    let options = class(c"Unity.Profiling", c"ProfilerRecorderOptions");
    let unit = class(c"Unity.Profiling", c"ProfilerMarkerDataUnit");
    let data = class(c"Unity.Profiling.LowLevel", c"ProfilerMarkerDataType");
    let api = managed_calls::Api::load(library)?;
    let adapters = [
        create as *const (),
        control_adapter as *const (),
        unit_adapter as *const (),
        data_adapter as *const (),
        current as *const (),
        current_double as *const (),
        last as *const (),
        last_double as *const (),
        count_adapter as *const (),
        valid as *const (),
        wrapped as *const (),
        running as *const (),
        sample_adapter as *const (),
    ];
    let mut targets = [0; 13];
    let mut names = Vec::new();
    for (slot, &(name, target)) in NAMES.iter().enumerate() {
        let full = CString::new(format!("Unity.Profiling.ProfilerRecorder::{name}"))
            .map_err(|e| e.to_string())?;
        targets[slot] = registry.exact_target(full.to_str().map_err(|e| e.to_string())?, target)?;
        let member = CString::new(name).map_err(|e| e.to_string())?;
        let signature = match slot {
            0 => vec![
                (1, void, false),
                (0x11, stat, true),
                (8, int, false),
                (0x11, options, false),
                (0x11, recorder, true),
            ],
            1 => vec![
                (1, void, false),
                (0x11, recorder, true),
                (0x11, control, false),
            ],
            2 => vec![(0x11, unit, false), (0x11, recorder, true)],
            3 => vec![(0x11, data, false), (0x11, recorder, true)],
            4 | 6 => vec![(0xa, long, false), (0x11, recorder, true)],
            5 | 7 => vec![(0xd, double, false), (0x11, recorder, true)],
            8 => vec![
                (8, int, false),
                (0x11, recorder, true),
                (0x11, count, false),
            ],
            9..=11 => vec![(2, boolean, false), (0x11, recorder, true)],
            12 => vec![
                (1, void, false),
                (0x11, recorder, true),
                (8, int, false),
                (0x11, sample, true),
            ],
            _ => unreachable!(),
        };
        let member = unsafe { method(recorder, member.as_ptr(), (signature.len() - 1) as i32) };
        api.fixed_typed_icall_enum_storage(
            member,
            recorder,
            &signature,
            images,
            &[(unit, 5), (data, 5)],
        )?;
        names.push(full);
    }
    TARGETS
        .set(targets)
        .map_err(|_| "recorder adapters already installed")?;
    install_description(library, stat, core, images, registry, api)?;
    STATE
        .set(Mutex::new(State::new()))
        .map_err(|_| "recorder registry already installed")?;
    for (name, adapter) in names.iter().zip(adapters) {
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    Ok(())
}
fn install_description(
    library: Object,
    stat: Object,
    core: Object,
    images: &[Object],
    registry: &bindings::Registry,
    api: managed_calls::Api,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing description API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const std::ffi::c_char, *const std::ffi::c_char) -> Object
    );
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const std::ffi::c_char, i32) -> Object
    );
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
        unsafe extern "C" fn(Object) -> *const std::ffi::c_char
    );
    let offset = sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> u32);
    let field_type = sym!(
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> i32);
    let type_class = sym!(
        "mono_class_from_mono_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let pointee = sym!(
        "mono_type_get_ptr_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let corlib = unsafe { corlib() };
    let class = |image, ns: &CStr, name: &CStr| unsafe { find(image, ns.as_ptr(), name.as_ptr()) };
    let description = class(
        core,
        c"Unity.Profiling.LowLevel.Unsafe",
        c"ProfilerRecorderDescription",
    );
    let category = class(core, c"Unity.Profiling", c"ProfilerCategory");
    let marker_flags = class(core, c"Unity.Profiling.LowLevel", c"MarkerFlags");
    let data = class(core, c"Unity.Profiling.LowLevel", c"ProfilerMarkerDataType");
    let unit = class(core, c"Unity.Profiling", c"ProfilerMarkerDataUnit");
    let int = class(corlib, c"System", c"Int32");
    let byte = class(corlib, c"System", c"Byte");
    let void = class(corlib, c"System", c"Void");
    let mut alignment = 0;
    if description.is_null() || unsafe { size(description, &mut alignment) } != 24 || alignment != 8
    {
        return Err("recorder description size/alignment mismatch".into());
    }
    for (cls, width) in [(category, 2), (marker_flags, 2), (data, 1), (unit, 1)] {
        if cls.is_null()
            || value_layout::Api::load(library, core)?
                .layout(cls)?
                .scalar_inline()?
                != (width, width)
        {
            return Err("recorder description declared scalar layout mismatch".into());
        }
    }
    let specs = [
        (c"category", 0, 0x11, category),
        (c"flags", 2, 0x11, marker_flags),
        (c"dataType", 4, 0x11, data),
        (c"unitType", 5, 0x11, unit),
        (c"reserved0", 8, 8, int),
        (c"nameUtf8Len", 12, 8, int),
        (c"nameUtf8", 16, 0xf, byte),
    ];
    let mut seen = [false; 7];
    let mut iter = ptr::null_mut();
    for _ in 0..16 {
        let field = unsafe { fields(description, &mut iter) };
        if field.is_null() {
            break;
        }
        if unsafe { flags(field) } & 0x10 != 0 {
            continue;
        }
        let field_name = unsafe { name(field) };
        if field_name.is_null() {
            return Err("description field name absent".into());
        }
        let index = specs
            .iter()
            .position(|(n, _, _, _)| *n == unsafe { CStr::from_ptr(field_name) })
            .ok_or("description has an unknown field")?;
        let (_, off, expected_kind, expected_class) = specs[index];
        let ty = unsafe { field_type(field) };
        if seen[index]
            || ty.is_null()
            || unsafe { offset(field) } != 16 + off
            || unsafe { kind(ty) } != expected_kind
            || expected_class.is_null()
        {
            return Err("description field offset/type mismatch".into());
        }
        let element = if expected_kind == 0xf {
            unsafe { pointee(ty) }
        } else {
            ty
        };
        if element.is_null() || unsafe { type_class(element) } != expected_class {
            return Err("description field exact class mismatch".into());
        }
        seen[index] = true;
    }
    if seen != [true; 7] {
        return Err("description incomplete field layout".into());
    }
    let member = unsafe { method(stat, c"GetDescriptionInternal_Injected".as_ptr(), 2) };
    api.fixed_typed_icall(
        member,
        stat,
        &[
            (1, void, false),
            (0x11, stat, true),
            (0x11, description, true),
        ],
        images,
    )?;
    let target = registry.exact_target(
        "Unity.Profiling.LowLevel.Unsafe.ProfilerRecorderHandle::GetDescriptionInternal_Injected",
        0x1800613e0,
    )?;
    DESCRIPTION_TARGET
        .set(target)
        .map_err(|_| "description adapter already installed")?;
    unsafe {
        add(c"Unity.Profiling.LowLevel.Unsafe.ProfilerRecorderHandle::GetDescriptionInternal_Injected".as_ptr(), description_adapter as *const () as Object)
    };
    Ok(())
}
unsafe extern "C" fn description_adapter(handle: *const u64, output: *mut u8) {
    checked(|| {
        let marker = read_handle(handle)?;
        if !animation::owns_marker(marker)
            || output.is_null()
            || !(output as usize).is_multiple_of(8)
        {
            return Err("description requires actual marker and aligned output".into());
        }
        let (_, bytes) = callbacks::guest_byte_arguments(
            *DESCRIPTION_TARGET
                .get()
                .ok_or("description target absent")?,
            &[0, 0],
            &[(0, &marker.to_le_bytes()), (1, &[0; 24])],
        )?;
        if bytes[0] != marker.to_le_bytes() {
            return Err("description changed marker input".into());
        }
        let mut payload = [0u8; 24];
        // Preserve declared fields only. Bytes 6..8 are padding.
        payload[..6].copy_from_slice(&bytes[1][..6]);
        payload[8..16].copy_from_slice(&bytes[1][8..16]);
        let length = i32::from_le_bytes(payload[12..16].try_into().unwrap());
        let address = u64::from_le_bytes(bytes[1][16..24].try_into().unwrap());
        if !(0..=4096).contains(&length) || address == 0 {
            return Err("description name pointer/length invalid".into());
        }
        let mut name = vec![0; length as usize + 1];
        callbacks::with_service(|memory, _| {
            let end = address
                .checked_add(name.len() as u64)
                .ok_or("description name overflow")?;
            let mut at = address;
            while at < end {
                let width = (end - at).min(4096 - (at & 4095)) as usize;
                if !memory.owned_noncode_access(at, width) {
                    return Err("description name is not owned noncode data".into());
                }
                at += width as u64;
            }
            memory.read(address, &mut name)
        })?;
        if name.last() != Some(&0) || name[..length as usize].contains(&0) {
            return Err("description name length/terminator mismatch".into());
        }
        std::str::from_utf8(&name[..length as usize]).map_err(|_| "description name not UTF8")?;
        let pointer = NAMES_UTF8.with(|names| -> Result<u64, String> {
            let mut names = names.borrow_mut();
            if let Some(previous) = names.get(&marker) {
                if previous.as_ref() != name {
                    return Err("description marker name changed".into());
                }
            } else {
                if names.len() >= 1024 {
                    return Err("description name capacity exceeded".into());
                }
                names.insert(marker, name.into_boxed_slice());
            }
            Ok(names[&marker].as_ptr() as u64)
        })?;
        payload[16..24].copy_from_slice(&pointer.to_le_bytes());
        unsafe { ptr::copy_nonoverlapping(payload.as_ptr(), output, 24) };
        state(|s| {
            s.calls += 1;
            Ok(())
        })?;
        Ok(())
    });
}
fn read_handle(pointer: *const u64) -> Result<u64, String> {
    if pointer.is_null() || !(pointer as usize).is_multiple_of(8) {
        return Err("recorder native byref alignment/null mismatch".into());
    }
    Ok(unsafe { pointer.read() })
}
fn owns(handle: u64, allow_zero: bool) -> Result<(), String> {
    if handle == 0 && allow_zero {
        return Ok(());
    }
    if !state(|s| {
        Ok(s.owner == thread::current().id()
            && !s.closed
            && s.live.get(&handle).is_some_and(|r| r.references > r.queued))
    })? {
        return Err("recorder handle is not an actual live engine result".into());
    }
    callbacks::with_service(|memory, runtime| {
        if !memory.owned_noncode_access(handle, 1)
            || !(runtime.live_data_slot(handle, 1) || memory.guest_private_data_slot(handle, 1))
        {
            return Err("engine recorder handle storage retired".into());
        }
        Ok(())
    })
}
fn checked<T: std::fmt::Debug>(operation: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(value)) => value,
        other => {
            eprintln!("NATIVE_MONO_PROFILER_RECORDER_REJECT: {other:?}");
            std::process::abort();
        }
    }
}
unsafe extern "C" fn create(stat: *const u64, capacity: i32, options: i32, output: *mut u64) {
    checked(|| {
        let stat = read_handle(stat)?;
        if output.is_null()
            || !(output as usize).is_multiple_of(8)
            || !(0..=4096).contains(&capacity)
            || options & !0xdf != 0
            || !animation::owns_marker(stat)
        {
            return Err(format!("recorder create rejected: marker={stat:#x}; owned-marker={}; capacity={capacity}; options={options:#x}; output-null={}", animation::owns_marker(stat), output.is_null()));
        }
        if state(|s| Ok(s.live.values().map(|r| r.references).sum::<usize>() >= 1024))? {
            return Err("recorder live capacity exceeded".into());
        }
        let (_, bytes) = callbacks::guest_byte_arguments(
            TARGETS.get().ok_or("recorder targets absent")?[0],
            &[0, capacity as u32 as u64, options as u32 as u64, 0],
            &[(0, &stat.to_le_bytes()), (3, &[0; 8])],
        )?;
        let handle = u64::from_le_bytes(bytes[1].as_slice().try_into().unwrap());
        if handle != 0 {
            callbacks::with_service(|memory, runtime| {
                if !memory.owned_noncode_access(handle, 1)
                    || !(runtime.live_data_slot(handle, 1)
                        || memory.guest_private_data_slot(handle, 1))
                {
                    return Err("recorder create returned an unowned handle".into());
                }
                Ok(())
            })?;
            state(|s| s.register(handle, options & 0x80 != 0))?;
        }
        unsafe { output.write(handle) };
        state(|s| {
            s.calls += 1;
            Ok(())
        })?;
        eprintln!("NATIVE_MONO_PROFILER_RECORDER_CREATED: marker={stat:#x}; capacity={capacity}; options={options:#x}; original-engine-handle={handle:#x}");
        Ok(())
    });
}
fn query(slot: usize, pointer: *const u64, trailing: &[u64]) -> (u64, u64) {
    checked(|| {
        let handle = read_handle(pointer)?;
        owns(handle, slot == 9)?;
        let mut args = vec![0];
        args.extend_from_slice(trailing);
        let (result, bytes) = callbacks::guest_byte_arguments(
            TARGETS.get().ok_or("recorder targets absent")?[slot],
            &args,
            &[(0, &handle.to_le_bytes())],
        )?;
        if bytes[0] != handle.to_le_bytes() {
            return Err("engine recorder changed immutable handle input".into());
        }
        if (9..=11).contains(&slot) && result.0 & 255 > 1 {
            return Err("recorder Boolean return not canonical".into());
        }
        state(|s| {
            s.calls += 1;
            Ok(())
        })?;
        Ok(result)
    })
}
unsafe extern "C" fn control_adapter(handle: *const u64, options: i32) {
    checked(|| {
        if !matches!(options, 0 | 1 | 2 | 4 | 5 | 6) {
            return Err("recorder control option outside enum".into());
        }
        let value = read_handle(handle)?;
        if options == 4 {
            if state(|s| Ok(s.owner != thread::current().id()))? {
                state(|s| s.queue(value, super::unsafe_buffers::actual_finalizer_scope()))?;
            } else {
                release_owned(value, false)?;
            }
        } else {
            query(1, handle, &[options as u64]);
        }
        Ok(())
    });
}
unsafe extern "C" fn unit_adapter(h: *const u64) -> u8 {
    query(2, h, &[]).0 as u8
}
unsafe extern "C" fn data_adapter(h: *const u64) -> u8 {
    query(3, h, &[]).0 as u8
}
unsafe extern "C" fn current(h: *const u64) -> i64 {
    query(4, h, &[]).0 as i64
}
unsafe extern "C" fn current_double(h: *const u64) -> f64 {
    f64::from_bits(query(5, h, &[]).1)
}
unsafe extern "C" fn last(h: *const u64) -> i64 {
    query(6, h, &[]).0 as i64
}
unsafe extern "C" fn last_double(h: *const u64) -> f64 {
    f64::from_bits(query(7, h, &[]).1)
}
unsafe extern "C" fn count_adapter(h: *const u64, option: i32) -> i32 {
    checked(|| {
        if !(0..=1).contains(&option) {
            return Err("recorder count option outside enum".into());
        }
        Ok(query(8, h, &[option as u64]).0 as i32)
    })
}
unsafe extern "C" fn valid(h: *const u64) -> u8 {
    query(9, h, &[]).0 as u8
}
unsafe extern "C" fn wrapped(h: *const u64) -> u8 {
    query(10, h, &[]).0 as u8
}
unsafe extern "C" fn running(h: *const u64) -> u8 {
    query(11, h, &[]).0 as u8
}
unsafe extern "C" fn sample_adapter(h: *const u64, index: i32, output: *mut u8) {
    checked(|| {
        let handle = read_handle(h)?;
        owns(handle, false)?;
        if output.is_null() || !(output as usize).is_multiple_of(8) || !(0..4096).contains(&index) {
            return Err("recorder sample output/index invalid".into());
        }
        let (_, bytes) = callbacks::guest_byte_arguments(
            TARGETS.get().ok_or("recorder targets absent")?[12],
            &[0, index as u64, 0],
            &[(0, &handle.to_le_bytes()), (2, &[0; 24])],
        )?;
        if bytes[0] != handle.to_le_bytes() {
            return Err("recorder sample mutated handle".into());
        }
        unsafe { ptr::copy_nonoverlapping(bytes[1].as_ptr(), output, 24) };
        state(|s| {
            s.calls += 1;
            Ok(())
        })?;
        Ok(())
    });
}
fn release_owned(handle: u64, queued: bool) -> Result<(), String> {
    callbacks::with_service(|memory, runtime| {
        if !memory.owned_noncode_access(handle, 1)
            || !(runtime.live_data_slot(handle, 1) || memory.guest_private_data_slot(handle, 1))
        {
            return Err("recorder release guest storage retired".into());
        }
        Ok(())
    })?;
    state(|s| s.begin_release(handle, queued))?;
    let (_, bytes) = callbacks::guest_byte_arguments(
        TARGETS.get().ok_or("recorder targets absent")?[1],
        &[0, 4],
        &[(0, &handle.to_le_bytes())],
    )?;
    if bytes[0] != handle.to_le_bytes() {
        return Err("recorder release changed handle input".into());
    }
    state(|s| {
        s.calls += 1;
        s.released += 1;
        s.drained += usize::from(queued);
        Ok(())
    })
}
pub(super) fn drain_finalizers() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let handles = state(|s| {
        if s.owner != thread::current().id() {
            return Err("recorder drain on foreign thread".into());
        }
        Ok(s.live
            .iter()
            .flat_map(|(&h, r)| std::iter::repeat_n(h, r.queued))
            .collect::<Vec<_>>())
    })?;
    for handle in handles {
        release_owned(handle, true)?;
    }
    Ok(())
}
pub(super) fn retire_after_vm_cleanup() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    drain_finalizers()?;
    let handles = state(|s| {
        Ok(s.live
            .iter()
            .flat_map(|(&h, r)| std::iter::repeat_n(h, r.references))
            .collect::<Vec<_>>())
    })?;
    for handle in handles {
        release_owned(handle, false)?;
    }
    state(|s| {
        s.closed = true;
        Ok(())
    })?;
    NAMES_UTF8.with(|names| names.borrow_mut().clear());
    Ok(())
}
pub(super) fn report() -> String {
    if STATE.get().is_none() {
        return String::new();
    }
    state(|s| Ok(format!("native-profiler-recorder-calls={}\nnative-profiler-recorders-created={}\nnative-profiler-recorders-released={}\nnative-profiler-recorders-live={}\nnative-profiler-recorder-finalizers-queued={}\nnative-profiler-recorder-finalizers-drained={}\nnative-profiler-recorders-closed={}\n", s.calls, s.created, s.released, s.live.values().map(|r| r.references).sum::<usize>(), s.queued, s.drained, s.closed))).unwrap()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_references_and_authenticated_finalizers_release_once_per_creation() {
        let mut s = State::new();
        s.register(0x1000, true).unwrap();
        s.register(0x1000, true).unwrap();
        assert!(s.register(0x1000, false).is_err());
        let shared = std::sync::Arc::new(Mutex::new(s));
        let foreign = shared.clone();
        thread::spawn(move || {
            let mut s = foreign.lock().unwrap();
            assert!(s.queue(0x1000, false).is_err());
            assert!(s.queue(0x1001, true).is_err());
            s.queue(0x1000, true).unwrap();
            s.queue(0x1000, true).unwrap();
            assert!(s.queue(0x1000, true).is_err());
            assert!(s.begin_release(0x1000, true).is_err());
        })
        .join()
        .unwrap();
        let mut s = shared.lock().unwrap();
        assert!(s.begin_release(0x1000, false).is_err());
        s.begin_release(0x1000, true).unwrap();
        assert_eq!(s.live[&0x1000].references, 1);
        s.begin_release(0x1000, true).unwrap();
        assert!(s.live.is_empty());
        assert!(s.begin_release(0x1000, true).is_err());
        assert!(s.queue(0x1000, true).is_err());
        s.closed = true;
        assert!(s.register(0x2000, true).is_err());
    }
}
