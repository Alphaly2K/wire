//! Fixed InputModule pointer ABIs. Event bytes stay in private, aligned native
//! snapshots; only checked guest addresses and copied scalar data go to Unity.
use super::{bindings, callbacks, managed_calls, unsafe_buffers, Memory, Object};
use crate::x64::memory::{READ, WRITE};
use crate::x64::runtime::Runtime64;
use std::{ffi::CStr, ptr, sync::OnceLock};
const LIMIT: usize = 1024 * 1024;
struct Binding {
    library: usize,
    update: usize,
    ioctl: u64,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
macro_rules! sym {
    ($library:expr, $name:literal, $ty:ty) => {{
        let p = unsafe { libc::dlsym($library, concat!($name, "\0").as_ptr().cast()) };
        if p.is_null() {
            return Err(concat!("missing input ABI API: ", $name).into());
        }
        unsafe { std::mem::transmute::<Object, $ty>(p) }
    }};
}
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn install(
    library: Object,
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
    let find = sym!(
        library,
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const std::ffi::c_char, *const std::ffi::c_char) -> Object
    );
    let method = sym!(
        library,
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const std::ffi::c_char, i32) -> Object
    );
    let class = |namespace: &CStr, name: &CStr| -> Result<Object, String> {
        let mut matches: Vec<_> = images
            .iter()
            .map(|&image| unsafe { find(image, namespace.as_ptr(), name.as_ptr()) })
            .filter(|p| !p.is_null())
            .collect();
        matches.sort_unstable_by_key(|p| *p as usize);
        matches.dedup();
        if matches.len() != 1 {
            return Err(format!(
                "input ABI class is not unique: {namespace:?}.{name:?}"
            ));
        }
        Ok(matches[0])
    };
    let owner = class(c"UnityEngineInternal.Input", c"NativeInputSystem")?;
    let event = class(c"UnityEngineInternal.Input", c"NativeInputEventBuffer")?;
    let update_type = class(c"UnityEngineInternal.Input", c"NativeInputUpdateType")?;
    let void = class(c"System", c"Void")?;
    let intptr = class(c"System", c"IntPtr")?;
    let int32 = class(c"System", c"Int32")?;
    let int64 = class(c"System", c"Int64")?;
    let api = managed_calls::Api::load(library)?;
    let update = unsafe { method(owner, c"NotifyUpdate".as_ptr(), 2) };
    api.fixed_typed_managed(
        update,
        owner,
        &[
            (1, void, false),
            (0x11, update_type, false),
            (0x18, intptr, false),
        ],
        images,
    )?;
    let ioctl = registry.exact_target(
        "UnityEngineInternal.Input.NativeInputSystem::IOCTL",
        0x180132940,
    )?;
    api.fixed_typed_icall(
        unsafe { method(owner, c"IOCTL".as_ptr(), 4) },
        owner,
        &[
            (0xa, int64, false),
            (8, int32, false),
            (8, int32, false),
            (0x18, intptr, false),
            (8, int32, false),
        ],
        images,
    )?;
    let size = sym!(
        library,
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let fields = sym!(
        library,
        "mono_class_get_fields",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let flags = sym!(
        library,
        "mono_field_get_flags",
        unsafe extern "C" fn(Object) -> u32
    );
    let name = sym!(
        library,
        "mono_field_get_name",
        unsafe extern "C" fn(Object) -> *const std::ffi::c_char
    );
    let offset = sym!(
        library,
        "mono_field_get_offset",
        unsafe extern "C" fn(Object) -> u32
    );
    let ty = sym!(
        library,
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!(
        library,
        "mono_type_get_type",
        unsafe extern "C" fn(Object) -> u32
    );
    let pointee = sym!(
        library,
        "mono_type_get_ptr_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let mut alignment = 0;
    if unsafe { size(event, &mut alignment) } != 20 {
        return Err("input event buffer is not the packed 20B ABI".into());
    }
    let specs = [
        (b"eventBuffer".as_slice(), 0, 0xf),
        (b"eventCount".as_slice(), 8, 8),
        (b"sizeInBytes".as_slice(), 12, 8),
        (b"capacityInBytes".as_slice(), 16, 8),
    ];
    let mut iterator = ptr::null_mut();
    let mut seen = [false; 4];
    loop {
        let field = unsafe { fields(event, &mut iterator) };
        if field.is_null() {
            break;
        }
        if unsafe { flags(field) } & 0x10 != 0 {
            continue;
        }
        let n = unsafe { name(field) };
        let t = unsafe { ty(field) };
        if n.is_null() || t.is_null() {
            return Err("input event field missing metadata".into());
        }
        let index = specs
            .iter()
            .position(|spec| unsafe { CStr::from_ptr(n) }.to_bytes() == spec.0)
            .ok_or("unknown input event buffer field")?;
        if seen[index]
            || unsafe { offset(field) } != 16 + specs[index].1
            || unsafe { kind(t) } != specs[index].2
            || (index == 0 && (unsafe { pointee(t) }.is_null() || unsafe { kind(pointee(t)) } != 1))
        {
            return Err("input event field layout/type mismatch".into());
        }
        seen[index] = true;
    }
    if seen != [true; 4] {
        return Err("input event buffer fields incomplete".into());
    }
    BINDING
        .set(Binding {
            library: library as usize,
            update: update as usize,
            ioctl,
        })
        .map_err(|_| "input bindings already installed")?;
    let add = sym!(
        library,
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    unsafe {
        add(
            c"UnityEngineInternal.Input.NativeInputSystem::IOCTL".as_ptr(),
            device_command as *const () as Object,
        )
    };
    Ok(())
}
fn checked<T>(operation: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_INPUT_REJECT: {error}");
            std::process::abort();
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_INPUT_REJECT: adapter panicked");
            std::process::abort();
        }
    }
}
fn command_size(code: u32, size: usize) -> Result<(), String> {
    // Frozen InputSystem commands with inline scalar/byte payloads. No command
    // containing a nested native pointer is copied to the original engine.
    let valid = match code.to_be_bytes() {
        [b'Q', b'E', b'N', b'B'] | [b'Q', b'B', b'G', b'D'] => size == 1,
        [b'E', b'N', b'B', b'L']
        | [b'D', b'S', b'B', b'L']
        | [b'R', b'S', b'E', b'T']
        | [b'S', b'Y', b'N', b'C']
        | [b'H', b'I', b'D', b'S'] => size == 0,
        [b'K', b'B', b'L', b'T'] => size == 256,
        [b'H', b'I', b'D', b'D'] => (1..=65536).contains(&size),
        _ => false,
    };
    if !valid {
        return Err(format!(
            "input command requires verified inline layout: code={code:#x}; size={size}"
        ));
    }
    Ok(())
}
unsafe extern "C" fn device_command(device: i32, code: i32, data: usize, size: i32) -> i64 {
    checked(|| {
        let b = BINDING.get().ok_or("input command binding absent")?;
        let size = usize::try_from(size).map_err(|_| "negative input command size")?;
        command_size(code as u32, size)?;
        if size != 0 && data == 0 {
            return Err("nonempty input command has NULL payload".into());
        }
        let mut bytes = vec![0; size.max(1)];
        if size != 0 {
            unsafe { ptr::copy_nonoverlapping(data as *const u8, bytes.as_mut_ptr(), size) };
        }
        let result = if size == 0 && data == 0 {
            callbacks::guest_arguments(b.ioctl, &[device as u32 as u64, code as u32 as u64, 0, 0])?
        } else {
            let (result, outputs) = callbacks::guest_byte_arguments(
                b.ioctl,
                &[device as u32 as u64, code as u32 as u64, 0, size as u64],
                &[(2, &bytes)],
            )?;
            if size != 0 {
                unsafe { ptr::copy_nonoverlapping(outputs[0].as_ptr(), data as *mut u8, size) };
            }
            result.0
        };
        eprintln!("NATIVE_MONO_INPUT_COMMAND: device={device}; code={code:#x}; size={size}; actual-result={:#x}", result);
        Ok(result as i64)
    })
}
#[derive(Clone, Copy)]
struct Header {
    pointer: u64,
    count: usize,
    size: usize,
    capacity: usize,
}
fn header(bytes: &[u8]) -> Result<Header, String> {
    if bytes.len() != 20 {
        return Err("input buffer header length mismatch".into());
    }
    let pointer = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    let count = i32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let size = i32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let capacity = i32::from_le_bytes(bytes[16..20].try_into().unwrap());
    if count < 0
        || size < 0
        || capacity < size
        || capacity as usize > LIMIT
        || count as usize > size as usize / 20
        || (size != 0 && pointer == 0)
    {
        return Err("input buffer count/size/capacity mismatch".into());
    }
    Ok(Header {
        pointer,
        count: count as usize,
        size: size as usize,
        capacity: capacity as usize,
    })
}
fn event_chain(bytes: &[u8], count: usize) -> Result<(), String> {
    let mut at = 0usize;
    for index in 0..count {
        if at.checked_add(20).is_none_or(|end| end > bytes.len()) {
            return Err("input event header exceeds owned payload".into());
        }
        let size = u16::from_le_bytes(bytes[at + 4..at + 6].try_into().unwrap()) as usize;
        let end = at.checked_add(size).ok_or("input event size overflow")?;
        if size < 20 || end > bytes.len() {
            return Err("input event size exceeds owned payload".into());
        }
        at = if index + 1 == count {
            end
        } else {
            (end + 3) & !3
        };
    }
    if at != bytes.len() && ((at + 3) & !3) != bytes.len() {
        return Err("input event count does not cover payload".into());
    }
    Ok(())
}
fn owned(
    memory: &Memory,
    runtime: Option<&Runtime64>,
    address: u64,
    size: usize,
) -> Result<(), String> {
    if size == 0 {
        return Ok(());
    }
    memory.check(address, size, READ | WRITE)?;
    let end = address
        .checked_add(size as u64)
        .ok_or("input buffer address overflow")?;
    let mut at = address;
    while at < end {
        let length = (end - at).min(4096 - (at & 4095)) as usize;
        if !memory.owned_noncode_access(at, length)
            || runtime.is_some_and(|runtime| {
                !(runtime.live_data_slot(at, length as u64)
                    || memory.guest_private_data_slot(at, length))
            })
        {
            return Err(format!("input buffer is not live owned noncode data: address={address:#x}; page={at:#x}; size={size}"));
        }
        at += length as u64;
    }
    Ok(())
}
pub(super) struct Prepared {
    method: Object,
    library: Object,
    guest: u64,
    initial: Header,
    update: u64,
    words: [u64; 3],
    payload: unsafe_buffers::InputSpan,
}
pub(super) fn prepare(
    method: Object,
    memory: &Memory,
    runtime: &Runtime64,
    arguments: u64,
    mut writable: impl FnMut(u64, usize) -> bool,
) -> Result<Option<Prepared>, String> {
    let Some(b) = BINDING.get().filter(|b| b.update == method as usize) else {
        return Ok(None);
    };
    if arguments == 0 || !arguments.is_multiple_of(8) {
        return Err("input callback vector is not aligned".into());
    }
    memory.check(arguments, 16, READ)?;
    let update = memory.read_int(memory.read_int(arguments, 8)?, 4)?;
    let guest = memory.read_int(memory.read_int(arguments + 8, 8)?, 8)?;
    if !writable(guest, 20) {
        return Err("input callback header is not a live callback-stack output".into());
    }
    owned(memory, None, guest, 20)?;
    let mut bytes = [0; 20];
    memory.read(guest, &mut bytes)?;
    let initial = header(&bytes)?;
    owned(memory, Some(runtime), initial.pointer, initial.capacity)?;
    let mut payload = unsafe_buffers::InputSpan::new(initial.capacity.max(1))?;
    if initial.size != 0 {
        memory.read(initial.pointer, payload.bytes_mut(initial.size)?)?;
    }
    event_chain(payload.bytes(initial.size)?, initial.count)?;
    let mut words = [0u64; 3];
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), words.as_mut_ptr().cast(), 20) };
    words[0] = if initial.pointer == 0 {
        0
    } else {
        payload.pointer() as u64
    };
    Ok(Some(Prepared {
        method,
        library: b.library as Object,
        guest,
        initial,
        update,
        words,
        payload,
    }))
}
impl Prepared {
    pub(super) fn output_address(&self) -> u64 {
        self.guest
    }
    pub(super) fn execute(&mut self) -> Result<Object, String> {
        let mut pointer = self.words.as_mut_ptr() as u64;
        let mut arguments = [
            (&mut self.update as *mut u64).cast(),
            (&mut pointer as *mut u64).cast(),
        ];
        let mut exception = ptr::null_mut();
        let result = unsafe {
            (managed_calls::Api::load(self.library)?.invoke)(
                self.method,
                ptr::null_mut(),
                arguments.as_mut_ptr(),
                &mut exception,
            )
        };
        if !exception.is_null() || !result.is_null() {
            return Err("input update IL raised an exception or returned an object".into());
        }
        Ok(result)
    }
    pub(super) fn finish(&self, memory: &mut Memory, runtime: &Runtime64) -> Result<(), String> {
        let bytes = unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast::<u8>(), 20) };
        let output = header(bytes)?;
        let (pointer, data) = if output.pointer == self.payload.pointer() as u64 {
            if output.capacity > self.initial.capacity {
                return Err("input callback enlarged its borrowed buffer".into());
            }
            (
                self.initial.pointer,
                self.payload.bytes(output.size)?.to_vec(),
            )
        } else if output.pointer == 0 && output.size == 0 && output.capacity == 0 {
            (0, Vec::new())
        } else {
            unsafe_buffers::export_input_span(output.pointer as usize, output.capacity).map(
                |(pointer, mut data)| {
                    data.truncate(output.size);
                    (pointer, data)
                },
            )?
        };
        event_chain(&data, output.count)?;
        owned(memory, None, self.guest, 20)?;
        owned(memory, Some(runtime), pointer, output.capacity)?;
        let mut header = bytes.to_vec();
        header[..8].copy_from_slice(&pointer.to_le_bytes());
        if !data.is_empty() {
            memory.write(pointer, &data)?;
        }
        memory.write(self.guest, &header)?;
        eprintln!("NATIVE_MONO_INPUT_EVENTS: actual-IL=true; input-count={}; output-count={}; input-bytes={}; output-bytes={}", self.initial.count, output.count, self.initial.size, output.size);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn event_chain_checks_each_size_and_alignment_before_native_il() {
        let mut bytes = [0u8; 44];
        bytes[4..6].copy_from_slice(&21u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&20u16.to_le_bytes());
        event_chain(&bytes, 2).unwrap();
        assert!(event_chain(&bytes, 1).is_err());
        assert!(event_chain(&bytes, 3).is_err());
        for size in [0u16, 19, 45, u16::MAX] {
            bytes[4..6].copy_from_slice(&size.to_le_bytes());
            assert!(event_chain(&bytes, 2).is_err());
        }
        event_chain(&[], 0).unwrap();
        let mut last = [0u8; 23];
        last[4..6].copy_from_slice(&23u16.to_le_bytes());
        event_chain(&last, 1).unwrap();
        assert!(event_chain(&last[..22], 1).is_err());
    }
    #[test]
    fn input_header_rejects_negative_oversize_and_inconsistent_counts() {
        let mut bytes = [0; 20];
        assert_eq!(header(&bytes).unwrap().size, 0);
        bytes[8..12].copy_from_slice(&1i32.to_le_bytes());
        assert!(header(&bytes).is_err());
        bytes[..8].copy_from_slice(&0x1000u64.to_le_bytes());
        bytes[12..16].copy_from_slice(&20i32.to_le_bytes());
        bytes[16..20].copy_from_slice(&20i32.to_le_bytes());
        assert_eq!(header(&bytes).unwrap().count, 1);
        bytes[16..20].copy_from_slice(&19i32.to_le_bytes());
        assert!(header(&bytes).is_err());
        bytes[16..20].copy_from_slice(&((LIMIT + 1) as i32).to_le_bytes());
        assert!(header(&bytes).is_err());
        bytes[8..12].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(header(&bytes).is_err());
    }
    #[test]
    fn device_commands_reject_unknown_pointer_layout_and_wrong_size() {
        command_size(0x51454e42, 1).unwrap();
        command_size(0x4b424c54, 256).unwrap();
        assert!(command_size(0x51454e42, 8).is_err());
        assert!(command_size(0x50545221, 8).is_err());
    }
}
