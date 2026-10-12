//! Owned output-string adapters. Unity allocates/frees its original UTF16
//! buffer; managed IL receives a private copy, never a guest or host capability.
use super::{bindings, callbacks, engine_objects, logging, managed_calls, Memory, Object};
use std::{cell::RefCell, collections::BTreeMap, ffi::CStr, ptr, sync::OnceLock};
const COUNT: usize = 11;
const LIMIT: usize = 64 * 1024 * 1024;
const SPECS: [(&CStr, &CStr, &CStr, u64, bool); COUNT] = [
    (
        c"UnityEngine.Object::ToString_Injected",
        c"Object",
        c"ToString_Injected",
        0x1800c3be0,
        true,
    ),
    (
        c"UnityEngine.Object::GetName_Injected",
        c"Object",
        c"GetName_Injected",
        0x1800c4100,
        true,
    ),
    (
        c"UnityEngine.Application::get_companyName_Injected",
        c"Application",
        c"get_companyName_Injected",
        0x18006f620,
        false,
    ),
    (
        c"UnityEngine.Application::get_productName_Injected",
        c"Application",
        c"get_productName_Injected",
        0x18006f5c0,
        false,
    ),
    (
        c"UnityEngine.Application::get_version_Injected",
        c"Application",
        c"get_version_Injected",
        0x18006edd0,
        false,
    ),
    (
        c"UnityEngine.Application::get_identifier_Injected",
        c"Application",
        c"get_identifier_Injected",
        0x18006f1d0,
        false,
    ),
    (
        c"UnityEngine.Application::get_dataPath_Injected",
        c"Application",
        c"get_dataPath_Injected",
        0x18006e570,
        false,
    ),
    (
        c"UnityEngine.Application::get_streamingAssetsPath_Injected",
        c"Application",
        c"get_streamingAssetsPath_Injected",
        0x18006e650,
        false,
    ),
    (
        c"UnityEngine.Application::get_persistentDataPath_Injected",
        c"Application",
        c"get_persistentDataPath_Injected",
        0x18006e720,
        false,
    ),
    (
        c"UnityEngine.Application::get_temporaryCachePath_Injected",
        c"Application",
        c"get_temporaryCachePath_Injected",
        0x18006e7f0,
        false,
    ),
    (
        c"UnityEngine.Application::get_consoleLogPath_Injected",
        c"Application",
        c"get_consoleLogPath_Injected",
        0x18006fc50,
        false,
    ),
];
struct Binding {
    targets: [u64; COUNT],
    free: u64,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
struct Entry {
    guest: u64,
    units: Box<[u16]>,
    length: usize,
}
#[derive(Default)]
struct Arena {
    live: BTreeMap<usize, Entry>,
    retired: Vec<Box<[u16]>>,
    retained: usize,
    calls: usize,
    frees: usize,
    closed: bool,
}
thread_local! { static ARENA: RefCell<Arena> = RefCell::new(Arena::default()); }
impl Arena {
    fn insert(&mut self, guest: u64, units: Vec<u16>, length: usize) -> Result<usize, String> {
        if self.closed
            || self.live.len() + self.retired.len() >= 4096
            || length > 32768
            || units.len() != length.max(1)
            || self
                .retained
                .checked_add(units.len() * 2)
                .is_none_or(|end| end > LIMIT)
        {
            return Err("output string owner/capacity mismatch".into());
        }
        let units = units.into_boxed_slice();
        let native = units.as_ptr() as usize;
        if self.live.contains_key(&native) {
            return Err("output string native address reused".into());
        }
        self.retained += units.len() * 2;
        self.live.insert(
            native,
            Entry {
                guest,
                units,
                length,
            },
        );
        self.calls += 1;
        Ok(native)
    }
    fn lookup(&self, native: usize) -> Result<u64, String> {
        self.live
            .get(&native)
            .filter(|entry| entry.length != 0)
            .map(|entry| entry.guest)
            .ok_or_else(|| "binding Free requires exact live allocated output string".into())
    }
    fn retire(&mut self, native: usize) -> Result<(), String> {
        self.lookup(native)?;
        let entry = self.live.remove(&native).unwrap();
        self.retired.push(entry.units);
        self.frees += 1;
        Ok(())
    }
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
                return Err(concat!("missing output string API: ", $name).into());
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
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let void = unsafe { find(corlib(), c"System".as_ptr(), c"Void".as_ptr()) };
    let intptr = unsafe { find(corlib(), c"System".as_ptr(), c"IntPtr".as_ptr()) };
    let span = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"ManagedSpanWrapper".as_ptr(),
        )
    };
    logging::validate_span_layout(library, span)?;
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; COUNT];
    for (index, &(binding, owner, member, target, receiver)) in SPECS.iter().enumerate() {
        let owner = unsafe { find(core, c"UnityEngine".as_ptr(), owner.as_ptr()) };
        let mut types = vec![(1, void, false)];
        if receiver {
            types.push((0x18, intptr, false));
        }
        types.push((0x11, span, true));
        api.fixed_typed_icall(
            unsafe { method(owner, member.as_ptr(), (types.len() - 1) as i32) },
            owner,
            &types,
            images,
        )?;
        targets[index] =
            registry.exact_target(binding.to_str().map_err(|e| e.to_string())?, target)?;
    }
    let allocator = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"BindingsAllocator".as_ptr(),
        )
    };
    api.fixed_typed_icall(
        unsafe { method(allocator, c"Free".as_ptr(), 1) },
        allocator,
        &[(1, void, false), (0xf, void, false)],
        images,
    )?;
    let free =
        registry.exact_target("UnityEngine.Bindings.BindingsAllocator::Free", 0x1800d1930)?;
    BINDING
        .set(Binding { targets, free })
        .map_err(|_| "output string adapters already installed")?;
    let adapters = [
        object_string as *const (),
        object_name as *const (),
        company as *const (),
        product as *const (),
        version as *const (),
        identifier as *const (),
        data_path as *const (),
        streaming_path as *const (),
        persistent_path as *const (),
        cache_path as *const (),
        log_path as *const (),
    ];
    for ((name, ..), adapter) in SPECS.iter().zip(adapters) {
        unsafe { add(name.as_ptr(), adapter as Object) };
    }
    unsafe {
        add(
            c"UnityEngine.Bindings.BindingsAllocator::Free".as_ptr(),
            free_string as *const () as Object,
        )
    };
    Ok(())
}
fn checked<T>(operation: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_OUTPUT_STRING_REJECT: {error}");
            std::process::abort();
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_OUTPUT_STRING_REJECT: adapter panicked");
            std::process::abort();
        }
    }
}
fn read_units(memory: &Memory, guest: u64, length: usize) -> Result<Vec<u16>, String> {
    if length > 32768 || (length != 0 && (guest == 0 || !guest.is_multiple_of(2))) {
        return Err("output string pointer/length mismatch".into());
    }
    if length == 0 {
        return Ok(vec![0]);
    }
    let size = length * 2;
    let mut at = guest;
    let end = guest
        .checked_add(size as u64)
        .ok_or("output string overflow")?;
    while at < end {
        let width = (end - at).min(4096 - (at & 4095)) as usize;
        if !memory.owned_noncode_access(at, width) {
            return Err("output string is not owned noncode engine data".into());
        }
        at += width as u64;
    }
    let mut bytes = vec![0; size];
    memory.read(guest, &mut bytes)?;
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect())
}
unsafe fn output(index: usize, pointer: usize, span: *mut logging::Span) {
    checked(|| {
        if span.is_null() || !(span as usize).is_multiple_of(8) {
            return Err("output string requires aligned native span".into());
        }
        let b = BINDING.get().ok_or("output string adapters absent")?;
        let receiver = SPECS[index].4;
        let pointer = if receiver && pointer != 0 {
            engine_objects::engine_pointer(pointer)?
        } else {
            0
        };
        let initial = [0u8; 16];
        let args = if receiver { vec![pointer, 0] } else { vec![0] };
        let (_, outputs) = callbacks::guest_byte_arguments(
            b.targets[index],
            &args,
            &[(usize::from(receiver), &initial)],
        )?;
        let length = unsafe { publish(&outputs[0], span)? };
        eprintln!(
            "NATIVE_MONO_OUTPUT_STRING: member={:?}; utf16-units={length}; original-engine=true",
            SPECS[index].0
        );
        Ok(())
    });
}
/// Publish only the owned copy of an original engine output. The same arena
/// and exact original allocator free serve every injected string family.
pub(super) unsafe fn publish(bytes: &[u8], span: *mut logging::Span) -> Result<usize, String> {
    if bytes.len() != 16 || span.is_null() || !(span as usize).is_multiple_of(8) {
        return Err("output string requires a 16B aligned native span".into());
    }
    let guest = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    let length = usize::try_from(i32::from_le_bytes(bytes[8..12].try_into().unwrap()))
        .map_err(|_| "negative output string length")?;
    let native = if guest == 0 && length == 0 {
        0
    } else {
        let units = callbacks::with_service(|memory, _| read_units(memory, guest, length))?;
        ARENA.with(|arena| arena.borrow_mut().insert(guest, units, length))?
    };
    let mut result = [0u8; 16];
    result[..8].copy_from_slice(&(native as u64).to_le_bytes());
    result[8..12].copy_from_slice(&(length as i32).to_le_bytes());
    unsafe { ptr::copy_nonoverlapping(result.as_ptr(), span.cast(), 16) };
    Ok(length)
}
unsafe extern "C" fn free_string(pointer: Object) {
    checked(|| {
        let b = BINDING.get().ok_or("output string free adapter absent")?;
        if pointer.is_null() {
            callbacks::guest_arguments(b.free, &[0])?;
            return Ok(());
        }
        let guest = ARENA.with(|arena| arena.borrow().lookup(pointer as usize))?;
        callbacks::guest_arguments(b.free, &[guest])?;
        ARENA.with(|arena| arena.borrow_mut().retire(pointer as usize))
    });
}
macro_rules! adapter {
    ($name:ident, $index:literal) => {
        unsafe extern "C" fn $name(span: *mut logging::Span) {
            unsafe { output($index, 0, span) }
        }
    };
}
unsafe extern "C" fn object_string(pointer: usize, span: *mut logging::Span) {
    unsafe { output(0, pointer, span) }
}
unsafe extern "C" fn object_name(pointer: usize, span: *mut logging::Span) {
    unsafe { output(1, pointer, span) }
}
adapter!(company, 2);
adapter!(product, 3);
adapter!(version, 4);
adapter!(identifier, 5);
adapter!(data_path, 6);
adapter!(streaming_path, 7);
adapter!(persistent_path, 8);
adapter!(cache_path, 9);
adapter!(log_path, 10);
pub(super) fn retire_after_vm_cleanup() -> Result<(), String> {
    let b = BINDING.get();
    let remaining = ARENA.with(|arena| {
        arena
            .borrow()
            .live
            .iter()
            .filter(|(_, e)| e.length != 0)
            .map(|(&pointer, e)| (pointer, e.guest))
            .collect::<Vec<_>>()
    });
    for (pointer, guest) in remaining {
        callbacks::guest_arguments(
            b.ok_or("output string cleanup adapter absent")?.free,
            &[guest],
        )?;
        ARENA.with(|arena| arena.borrow_mut().retire(pointer))?;
    }
    ARENA.with(|arena| {
        let mut arena = arena.borrow_mut();
        arena.live.clear();
        arena.retired.clear();
        arena.retained = 0;
        arena.closed = true;
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn string_buffers_keep_utf16_null_empty_and_exact_free_lifetime() {
        let mut arena = Arena::default();
        let native = arena
            .insert(0x1000, vec![0x3042, 0xd83d, 0xde00], 3)
            .unwrap();
        assert_eq!(arena.lookup(native).unwrap(), 0x1000);
        assert!(arena.lookup(native + 2).is_err());
        assert_eq!(
            arena.live[&native].units.as_ref(),
            &[0x3042, 0xd83d, 0xde00]
        );
        arena.retire(native).unwrap();
        assert!(arena.lookup(native).is_err());
        assert!(arena.retire(native).is_err());
        let empty = arena.insert(1, vec![0], 0).unwrap();
        assert!(arena.lookup(empty).is_err());
        assert_ne!(empty, native);
        arena.closed = true;
        assert!(arena.insert(0x2000, vec![65], 1).is_err());
    }
    #[test]
    fn string_reads_reject_code_misalignment_and_oversize_without_dereference() {
        use crate::x64::memory::{EXECUTE, READ, WRITE};
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        memory.map(0x2000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.write_int(0x1000, 2, 0x3042).unwrap();
        assert_eq!(read_units(&memory, 0x1000, 1).unwrap(), vec![0x3042]);
        for (pointer, length) in [(0, 1), (0x1001, 1), (0x2000, 1), (0x1000, 32769)] {
            assert!(read_units(&memory, pointer, length).is_err());
        }
        assert_eq!(read_units(&memory, 1, 0).unwrap(), vec![0]);
    }
}
