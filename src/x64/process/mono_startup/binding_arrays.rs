//! Numeric output arrays. Original engine storage is copied, while an exact
//! issued native header mediates the original allocator's ownership transfer.
use super::{bindings, callbacks, managed_calls, Memory, Object};
use std::{cell::RefCell, collections::BTreeMap, ffi::CStr, ptr, sync::OnceLock};
const COUNT_LIMIT: usize = 65536;
const BYTE_LIMIT: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug)]
pub(super) struct Layout {
    pub(super) class: usize,
    pub(super) width: usize,
    pub(super) alignment: usize,
    offsets: [usize; 3],
    owned_width: usize,
    owned_data: usize,
    flags: [i32; 6],
}
struct Binding {
    layout: Layout,
    free: u64,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
struct Entry {
    guest: Option<u64>,
    live: bool,
    _data: Vec<u64>,
    _header: Vec<u64>,
}
#[derive(Default)]
struct Arena {
    entries: BTreeMap<usize, Entry>,
    retained: usize,
    closed: bool,
}
thread_local! { static ARENA: RefCell<Arena> = RefCell::new(Arena::default()); }
impl Arena {
    fn insert(&mut self, layout: &Layout, guest: Option<u64>, bytes: &[u8]) -> Result<u64, String> {
        let cost = bytes.len().max(1).div_ceil(8) * 8 + layout.owned_width;
        if self.closed
            || self.entries.len() >= 4096
            || self
                .retained
                .checked_add(cost)
                .is_none_or(|n| n > BYTE_LIMIT)
        {
            return Err("output-array snapshot capacity exhausted".into());
        }
        let mut data = vec![0u64; bytes.len().max(1).div_ceil(8)];
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), data.as_mut_ptr().cast(), bytes.len()) };
        let mut header = vec![0u64; layout.owned_width.div_ceil(8)];
        let pointer = data.as_ptr() as u64;
        unsafe {
            ptr::copy_nonoverlapping(
                pointer.to_le_bytes().as_ptr(),
                header.as_mut_ptr().cast::<u8>().add(layout.owned_data),
                8,
            )
        };
        let key = header.as_ptr() as usize;
        if self.entries.contains_key(&key) {
            return Err("output-array header address reused".into());
        }
        let published = if guest.is_some() { key as u64 } else { pointer };
        self.entries.insert(
            key,
            Entry {
                guest,
                live: guest.is_some(),
                _data: data,
                _header: header,
            },
        );
        self.retained += cost;
        Ok(published)
    }
    fn lookup(&self, native: usize) -> Result<u64, String> {
        self.entries
            .get(&native)
            .filter(|e| e.live)
            .and_then(|e| e.guest)
            .ok_or_else(|| {
                "array free requires exact issued live owned header on owner thread".into()
            })
    }
    fn retire(&mut self, native: usize) -> Result<(), String> {
        self.lookup(native)?;
        self.entries.get_mut(&native).unwrap().live = false;
        // Keep addresses reserved until VM teardown; escaped unsafe views never
        // point at reused native storage. The finite diagnostic arena is bounded.
        Ok(())
    }
}
macro_rules! sym {
    ($library:expr, $name:literal, $ty:ty) => {{
        let p = unsafe { libc::dlsym($library, concat!($name, "\0").as_ptr().cast()) };
        if p.is_null() {
            return Err(concat!("missing array-output API: ", $name).into());
        }
        unsafe { std::mem::transmute::<Object, $ty>(p) }
    }};
}

/// Entire actual record must match: names, field owner, pointee, width,
/// alignment and nonoverlap. Offsets come from current metadata.
fn record(
    library: Object,
    class: Object,
    specs: &[(&CStr, u32, Object, usize)],
) -> Result<(usize, usize, Vec<usize>), String> {
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
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let parent = sym!(
        library,
        "mono_field_get_parent",
        unsafe extern "C" fn(Object) -> Object
    );
    let offset = sym!(
        library,
        "mono_field_get_offset",
        unsafe extern "C" fn(Object) -> i32
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
    let byref = sym!(
        library,
        "mono_type_is_byref",
        unsafe extern "C" fn(Object) -> u32
    );
    let type_class = sym!(
        library,
        "mono_class_from_mono_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let pointee = sym!(
        library,
        "mono_type_get_ptr_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let value = sym!(
        library,
        "mono_class_is_valuetype",
        unsafe extern "C" fn(Object) -> i32
    );
    if class.is_null() || unsafe { value(class) } == 0 {
        return Err("array wrapper record is not a value".into());
    }
    let mut alignment = 0;
    let width = unsafe { size(class, &mut alignment) };
    if !(8..=64).contains(&width) || alignment != 8 {
        return Err("array wrapper record size/alignment incompatible".into());
    }
    let mut offsets = vec![usize::MAX; specs.len()];
    let mut covered = vec![false; width as usize];
    let mut iterator = ptr::null_mut();
    loop {
        let field = unsafe { fields(class, &mut iterator) };
        if field.is_null() {
            break;
        }
        if unsafe { flags(field) } & 0x10 != 0 {
            continue;
        }
        let n = unsafe { name(field) };
        let t = unsafe { ty(field) };
        if n.is_null()
            || t.is_null()
            || unsafe { parent(field) } != class
            || unsafe { byref(t) } != 0
        {
            return Err("array record field declaration absent".into());
        }
        let index = specs
            .iter()
            .position(|s| s.0 == unsafe { CStr::from_ptr(n) })
            .ok_or("unexpected array record field")?;
        let (_, k, expected, bytes) = specs[index];
        let actual = if k == 0xf { unsafe { pointee(t) } } else { t };
        let at = unsafe { offset(field) }
            .checked_sub(16)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("array record field outside unboxed storage")?;
        let end = at.checked_add(bytes).ok_or("array field extent overflow")?;
        if offsets[index] != usize::MAX
            || expected.is_null()
            || actual.is_null()
            || unsafe { kind(t) } != k
            || unsafe { type_class(actual) } != expected
            || !at.is_multiple_of(bytes)
            || end > width as usize
            || covered[at..end].iter().any(|b| *b)
        {
            return Err("array record field type/extent/overlap mismatch".into());
        }
        offsets[index] = at;
        covered[at..end].fill(true);
    }
    if offsets.contains(&usize::MAX) {
        return Err("array record missing required fields".into());
    }
    Ok((width as usize, alignment as usize, offsets))
}

pub(super) fn describe(library: Object, core: Object) -> Result<Layout, String> {
    let find = sym!(
        library,
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, *const libc::c_char) -> Object
    );
    let fields = sym!(
        library,
        "mono_class_get_fields",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let field = sym!(
        library,
        "mono_class_get_field_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char) -> Object
    );
    let ty = sym!(
        library,
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let type_class = sym!(
        library,
        "mono_class_from_mono_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let nesting = sym!(
        library,
        "mono_class_get_nesting_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let nested = sym!(
        library,
        "mono_class_get_nested_types",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let name = sym!(
        library,
        "mono_class_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let field_name = sym!(
        library,
        "mono_field_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let attrs = sym!(
        library,
        "mono_field_get_flags",
        unsafe extern "C" fn(Object) -> u32
    );
    let data = sym!(
        library,
        "mono_field_get_data",
        unsafe extern "C" fn(Object) -> *const u8
    );
    let blob_size = sym!(
        library,
        "mono_metadata_decode_blob_size",
        unsafe extern "C" fn(*const u8, *mut *const u8) -> u32
    );
    let integer = sym!(
        library,
        "mono_get_int32_class",
        unsafe extern "C" fn() -> Object
    );
    let void = sym!(
        library,
        "mono_get_void_class",
        unsafe extern "C" fn() -> Object
    );
    let is_enum = sym!(
        library,
        "mono_class_is_enum",
        unsafe extern "C" fn(Object) -> i32
    );
    let enum_base = sym!(
        library,
        "mono_class_enum_basetype",
        unsafe extern "C" fn(Object) -> Object
    );
    let wrapper = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"BlittableArrayWrapper".as_ptr(),
        )
    };
    let allocator = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"BindingsAllocator".as_ptr(),
        )
    };
    if wrapper.is_null() || allocator.is_null() {
        return Err("actual array-output types absent".into());
    }
    let flag_field = unsafe { field(wrapper, c"updateFlags".as_ptr()) };
    if flag_field.is_null() {
        return Err("actual array update flag field absent".into());
    }
    let enumeration = unsafe { type_class(ty(flag_field)) };
    if enumeration.is_null()
        || unsafe { nesting(enumeration) } != wrapper
        || unsafe { is_enum(enumeration) } == 0
        || unsafe { type_class(enum_base(enumeration)) } != unsafe { integer() }
    {
        return Err("array update flags are not actual nested Int32 enum".into());
    }
    let names = [
        c"NoUpdateNeeded",
        c"SizeChanged",
        c"DataIsNativePointer",
        c"DataIsNativeOwnedMemory",
        c"DataIsEmpty",
        c"DataIsNull",
    ];
    let mut values = [None; 6];
    let mut iterator = ptr::null_mut();
    loop {
        let f = unsafe { fields(enumeration, &mut iterator) };
        if f.is_null() {
            break;
        }
        if unsafe { attrs(f) } & 0x10 == 0 {
            continue;
        }
        let n = unsafe { field_name(f) };
        if n.is_null() {
            return Err("array flag field name absent".into());
        }
        let index = names
            .iter()
            .position(|n2| *n2 == unsafe { CStr::from_ptr(n) })
            .ok_or("unexpected array update flag constant")?;
        let blob = unsafe { data(f) };
        let mut bytes = ptr::null();
        if values[index].is_some()
            || unsafe { attrs(f) } & (0x10 | 0x40 | 0x8000 | 0x100) != 0x8050
            || unsafe { type_class(ty(f)) } != enumeration
            || blob.is_null()
            || unsafe { blob_size(blob, &mut bytes) } != 4
            || bytes.is_null()
        {
            return Err("array update flag is not a unique metadata Int32 constant".into());
        }
        values[index] = Some(i32::from_le_bytes(unsafe { *(bytes.cast::<[u8; 4]>()) }));
    }
    if values.iter().any(Option::is_none) {
        return Err("array update flag constants incomplete".into());
    }
    let flags = values.map(Option::unwrap);
    if flags[0] != 0
        || flags
            .iter()
            .enumerate()
            .any(|(i, n)| flags[..i].contains(n))
    {
        return Err("array update flags ambiguous or default is nonzero".into());
    }
    let (width, alignment, offsets) = record(
        library,
        wrapper,
        &[
            (c"data", 0xf, unsafe { void() }, 8),
            (c"size", 8, unsafe { integer() }, 4),
            (c"updateFlags", 0x11, enumeration, 4),
        ],
    )?;
    let mut owned: Object = ptr::null_mut();
    let mut iterator = ptr::null_mut();
    loop {
        let candidate = unsafe { nested(allocator, &mut iterator) };
        if candidate.is_null() {
            break;
        }
        let n = unsafe { name(candidate) };
        if !n.is_null() && unsafe { CStr::from_ptr(n) } == c"NativeOwnedMemory" {
            if !owned.is_null() {
                return Err("native-owned array header class ambiguous".into());
            }
            owned = candidate;
        }
    }
    let (owned_width, _, owned_offsets) =
        record(library, owned, &[(c"data", 0xf, unsafe { void() }, 8)])?;
    Ok(Layout {
        class: wrapper as usize,
        width,
        alignment,
        offsets: offsets.try_into().unwrap(),
        owned_width,
        owned_data: owned_offsets[0],
        flags,
    })
}
pub(super) fn install(
    library: Object,
    core: Object,
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
    let layout = describe(library, core)?;
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
    let void = sym!(
        library,
        "mono_get_void_class",
        unsafe extern "C" fn() -> Object
    );
    let lookup = sym!(
        library,
        "mono_lookup_internal_call",
        unsafe extern "C" fn(Object) -> Object
    );
    let add = sym!(
        library,
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let owner = unsafe {
        find(
            core,
            c"UnityEngine.Bindings".as_ptr(),
            c"BindingsAllocator".as_ptr(),
        )
    };
    let target = unsafe { method(owner, c"FreeNativeOwnedMemory".as_ptr(), 1) };
    managed_calls::Api::load(library)?.fixed_typed_icall(
        target,
        owner,
        &[
            (1, unsafe { void() }, false),
            (0xf, unsafe { void() }, false),
        ],
        images,
    )?;
    if !unsafe { lookup(target) }.is_null() {
        return Err("array free would replace existing protocol".into());
    }
    let free = registry
        .registered_target("UnityEngine.Bindings.BindingsAllocator::FreeNativeOwnedMemory")?
        .ok_or("original array free target absent")?;
    BINDING
        .set(Binding { layout, free })
        .map_err(|_| "array bindings already installed")?;
    unsafe {
        add(
            c"UnityEngine.Bindings.BindingsAllocator::FreeNativeOwnedMemory".as_ptr(),
            free_owned as *const () as Object,
        )
    };
    Ok(())
}
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn layout() -> Result<&'static Layout, String> {
    Ok(&BINDING.get().ok_or("array output protocol absent")?.layout)
}

fn read(memory: &Memory, pointer: u64, bytes: usize, alignment: usize) -> Result<Vec<u8>, String> {
    if bytes == 0 {
        return Ok(Vec::new());
    }
    if bytes > 2 * 1024 * 1024
        || pointer == 0
        || !pointer.is_multiple_of(alignment as u64)
        || !memory.guest_private_data_range(pointer, bytes)
    {
        return Err("array output span is not bounded aligned live private engine data".into());
    }
    let mut result = vec![0; bytes];
    memory.read(pointer, &mut result)?;
    Ok(result)
}
type Snapshot = (Vec<u8>, Option<u64>, Vec<u8>);
fn snapshot(
    layout: &Layout,
    memory: &Memory,
    bytes: &[u8],
    element: managed_calls::NumericSpanLayout,
) -> Result<Snapshot, String> {
    if bytes.len() != layout.width
        || !matches!(element.alignment, 1 | 2 | 4 | 8)
        || element.stride == 0
        || !element.stride.is_multiple_of(element.alignment)
    {
        return Err("array output layout mismatch".into());
    }
    let [d, s, f] = layout.offsets;
    let pointer = u64::from_le_bytes(bytes[d..d + 8].try_into().unwrap());
    let count = usize::try_from(i32::from_le_bytes(bytes[s..s + 4].try_into().unwrap()))
        .map_err(|_| "negative array output count")?;
    let flag = i32::from_le_bytes(bytes[f..f + 4].try_into().unwrap());
    let state = layout
        .flags
        .iter()
        .position(|&v| v == flag)
        .ok_or("unknown array output update flag")?;
    let size = count
        .checked_mul(element.stride)
        .ok_or("array output extent overflow")?;
    if count > COUNT_LIMIT || size > 2 * 1024 * 1024 {
        return Err("array output count/extent exceeds limit".into());
    }
    let (guest, data) = match state {
        0 | 4 | 5 => {
            if pointer != 0 || count != 0 {
                return Err("empty/null/unchanged array output contains storage".into());
            }
            (None, Vec::new())
        }
        1 | 2 => (None, read(memory, pointer, size, element.alignment)?),
        3 => {
            let header = read(memory, pointer, layout.owned_width, 8)?;
            let at = layout.owned_data;
            let data = u64::from_le_bytes(header[at..at + 8].try_into().unwrap());
            (Some(pointer), read(memory, data, size, element.alignment)?)
        }
        _ => unreachable!(),
    };
    let mut header = vec![0; layout.width];
    header[s..s + 4].copy_from_slice(&(count as i32).to_le_bytes());
    header[f..f + 4].copy_from_slice(&flag.to_le_bytes());
    Ok((header, guest, data))
}
pub(super) fn prepare_output(
    bytes: &[u8],
    element: managed_calls::NumericSpanLayout,
) -> Result<Vec<u8>, String> {
    let layout = layout()?;
    let (mut header, guest, data) =
        callbacks::with_service(|memory, _| snapshot(layout, memory, bytes, element))?;
    let flag = i32::from_le_bytes(
        header[layout.offsets[2]..layout.offsets[2] + 4]
            .try_into()
            .unwrap(),
    );
    if guest.is_some() || !data.is_empty() {
        let native = ARENA.with(|arena| arena.borrow_mut().insert(layout, guest, &data))?;
        let at = layout.offsets[0];
        header[at..at + 8].copy_from_slice(&native.to_le_bytes());
    }
    eprintln!("NATIVE_MONO_ARRAY_OUTPUT: element={:#x}; stride={}; bytes={}; flags={flag}; original-engine=true", element.element, element.stride, data.len());
    Ok(header)
}
fn release(native: usize, target: u64) -> Result<(), String> {
    let guest = ARENA.with(|arena| arena.borrow().lookup(native))?;
    callbacks::guest_arguments(target, &[guest])?;
    ARENA.with(|arena| arena.borrow_mut().retire(native))
}
unsafe extern "C" fn free_owned(pointer: Object) {
    let result = std::panic::catch_unwind(|| {
        release(
            pointer as usize,
            BINDING.get().ok_or("array free protocol absent")?.free,
        )
    });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("NATIVE_MONO_ARRAY_OUTPUT_REJECT: {e}");
            std::process::abort();
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_ARRAY_OUTPUT_REJECT: panic");
            std::process::abort();
        }
    }
}
pub(super) fn retire_after_vm_cleanup() -> Result<(), String> {
    let pending = ARENA.with(|a| {
        a.borrow()
            .entries
            .iter()
            .filter(|(_, e)| e.live)
            .map(|(&p, _)| p)
            .collect::<Vec<_>>()
    });
    for native in pending {
        release(
            native,
            BINDING.get().ok_or("array cleanup protocol absent")?.free,
        )?;
    }
    ARENA.with(|a| {
        let mut a = a.borrow_mut();
        a.closed = true;
        a.entries.clear();
        a.retained = 0;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::{
        memory::{EXECUTE, READ, WRITE},
        runtime::Runtime64,
    };
    fn layout() -> Layout {
        Layout {
            class: 1,
            width: 16,
            alignment: 8,
            offsets: [0, 8, 12],
            owned_width: 8,
            owned_data: 0,
            flags: [0, 1, 2, 3, 4, 5],
        }
    }
    fn element() -> managed_calls::NumericSpanLayout {
        managed_calls::NumericSpanLayout {
            element: 2,
            stride: 4,
            alignment: 4,
        }
    }
    fn header(pointer: u64, count: i32, flag: i32) -> Vec<u8> {
        let mut h = pointer.to_le_bytes().to_vec();
        h.extend(count.to_le_bytes());
        h.extend(flag.to_le_bytes());
        h
    }
    #[test]
    fn numeric_array_snapshots_preserve_flags_and_reject_invalid_spans() {
        let mut memory = Memory::default();
        let base = memory.virtual_alloc(0, 16384, 0x3000, 4).unwrap();
        let pointer = base + 4092;
        memory.write(pointer, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let mut l = layout();
        for flag in [1, 2] {
            let (h, guest, bytes) =
                snapshot(&l, &memory, &header(pointer, 2, flag), element()).unwrap();
            assert_eq!(&h[..8], &[0; 8]);
            assert!(guest.is_none());
            assert_eq!(bytes, [1, 2, 3, 4, 5, 6, 7, 8]);
        }
        let owned = base + 128;
        memory.write_int(owned, 8, pointer).unwrap();
        let (_, guest, bytes) = snapshot(&l, &memory, &header(owned, 2, 3), element()).unwrap();
        assert_eq!(guest, Some(owned));
        memory.write(pointer, &[0; 8]).unwrap();
        assert_eq!(bytes, [1, 2, 3, 4, 5, 6, 7, 8]);
        for flag in [0, 4, 5] {
            assert!(snapshot(&l, &memory, &header(0, 0, flag), element())
                .unwrap()
                .2
                .is_empty());
            assert!(snapshot(&l, &memory, &header(pointer, 0, flag), element()).is_err());
        }
        for h in [
            header(0, 2, 1),
            header(pointer + 1, 2, 1),
            header(pointer, -1, 1),
            header(pointer, 65537, 1),
            header(pointer, 2, 99),
            header(base + 16380, 2, 1),
            header(0, 0, 3),
        ] {
            assert!(snapshot(&l, &memory, &h, element()).is_err());
        }
        // Symbolic constants may vary; the actual metadata values are used.
        l.flags[2] = 17;
        assert!(snapshot(&l, &memory, &header(pointer, 2, 17), element()).is_ok());
        assert!(snapshot(&l, &memory, &header(pointer, 2, 2), element()).is_err());
        memory.virtual_protect(base + 4096, 4096, 0x40).unwrap();
        memory.virtual_protect(base + 4096, 4096, 4).unwrap();
        assert!(snapshot(&l, &memory, &header(pointer, 2, 1), element()).is_err());
    }
    #[test]
    fn issued_array_free_restores_original_guest_header_and_rejects_reuse() {
        let l = layout();
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.map(0x2000, 4096, READ | WRITE).unwrap();
        let stack = crate::x64::process::STACK;
        memory.map(stack, 65536, READ | WRITE).unwrap();
        let mut runtime =
            Runtime64::new(&mut memory, 0x140000000, stack, 65536, "array-output-test").unwrap();
        // Original engine allocator stub records its actual Win64 RCX value.
        let mut code = vec![0x48, 0xb8];
        code.extend(0x2000u64.to_le_bytes());
        code.extend([0x48, 0x89, 0x08, 0xc3]);
        memory.write(0x1000, &code).unwrap();
        ARENA.with(|a| *a.borrow_mut() = Arena::default());
        let native = ARENA
            .with(|a| a.borrow_mut().insert(&l, Some(0x12345000), &[1, 2, 3, 4]))
            .unwrap();
        let pointer = unsafe { (native as *const u64).read() };
        assert_eq!(
            unsafe { std::slice::from_raw_parts(pointer as *const u8, 4) },
            &[1, 2, 3, 4]
        );
        assert!(ARENA
            .with(|a| a.borrow().lookup(native as usize + 8))
            .is_err());
        assert!(
            std::thread::spawn(move || ARENA.with(|a| a.borrow().lookup(native as usize)))
                .join()
                .unwrap()
                .is_err()
        );
        callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            release(native as usize, 0x1000)
        })
        .unwrap()
        .0
        .unwrap();
        assert_eq!(memory.read_int(0x2000, 8).unwrap(), 0x12345000);
        assert!(ARENA.with(|a| a.borrow().lookup(native as usize)).is_err());
        assert!(
            callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || release(
                native as usize,
                0x1000
            ))
            .unwrap()
            .0
            .is_err()
        );
        let next = ARENA
            .with(|a| a.borrow_mut().insert(&l, Some(0x5555), &[5; 4]))
            .unwrap();
        assert_ne!(next, native);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(pointer as *const u8, 4) },
            &[1, 2, 3, 4]
        );
        ARENA.with(|a| {
            let mut a = a.borrow_mut();
            a.closed = true;
            assert!(a.insert(&l, None, &[0]).is_err());
            *a = Arena::default();
        });
    }
}
