//! Context-owned culling handles and native numeric snapshots. Engine pointers
//! never become native IL pointers; every imported array has actual metadata,
//! a checked guest extent and an owner render-loop lifetime.
use super::super::{bindings, callbacks, closed_types, managed_calls, value_layout, Object};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::CStr,
    ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
};

const BASE: u64 = 0x6fe800000000;
const LIMIT: usize = 4096;
const BYTE_LIMIT: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug)]
pub(in crate::x64::process::mono_startup) struct RecordLayout {
    pub(in crate::x64::process::mono_startup) width: usize,
    alignment: usize,
    fields: Vec<(usize, usize)>,
}
impl RecordLayout {
    fn check(&self) -> Result<(), String> {
        let mut covered = vec![false; self.width];
        if self.width == 0 || self.width > 128 || self.alignment != 8 {
            return Err("culling record size/alignment outside bounded pointer ABI".into());
        }
        for &(offset, width) in &self.fields {
            let end = offset
                .checked_add(width)
                .ok_or("culling field extent overflow")?;
            if !matches!(width, 4 | 8)
                || !offset.is_multiple_of(width)
                || end > self.width
                || covered[offset..end].iter().any(|b| *b)
            {
                return Err("culling field is overlapping, unaligned or outside value".into());
            }
            covered[offset..end].fill(true);
        }
        Ok(())
    }
    fn word(&self, bytes: &[u8], field: usize) -> Result<u64, String> {
        if bytes.len() != self.width {
            return Err("culling record byte size changed".into());
        }
        let (at, width) = self.fields[field];
        let mut word = [0; 8];
        word[..width].copy_from_slice(&bytes[at..at + width]);
        Ok(u64::from_le_bytes(word))
    }
    fn put(&self, bytes: &mut [u8], field: usize, word: u64) {
        let (at, width) = self.fields[field];
        bytes[at..at + width].copy_from_slice(&word.to_le_bytes()[..width]);
    }
    fn canonicalize(&self, bytes: &mut [u8]) -> Result<(), String> {
        if bytes.len() != self.width {
            return Err("culling record byte size changed".into());
        }
        let mut covered = vec![false; self.width];
        for &(at, width) in &self.fields {
            covered[at..at + width].fill(true);
        }
        for (byte, covered) in bytes.iter_mut().zip(covered) {
            if !covered {
                *byte = 0;
            }
        }
        Ok(())
    }
}
struct Binding {
    result_class: usize,
    target: u64,
    result: RecordLayout,
    allocation: RecordLayout,
    parameters: value_layout::NumericLayout,
    elements: [value_layout::NumericLayout; 3],
}
static BINDING: OnceLock<Binding> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Snapshot {
    words: Vec<u64>,
    length: usize,
}
impl Snapshot {
    fn new(bytes: &[u8]) -> Self {
        let mut words = vec![0; bytes.len().max(1).div_ceil(8)];
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), words.as_mut_ptr().cast(), bytes.len()) };
        Self {
            words,
            length: bytes.len(),
        }
    }
    fn pointer(&self) -> u64 {
        self.words.as_ptr() as u64
    }
}
struct Record {
    context: u64,
    pointer: u64,
    live: bool,
    _allocation: Snapshot,
    _arrays: [Snapshot; 3],
}
#[derive(Default)]
struct State {
    records: BTreeMap<u64, Record>,
    retained: usize,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

pub(in crate::x64::process::mono_startup) fn result_class(class: Object) -> bool {
    BINDING
        .get()
        .is_some_and(|b| b.result_class == class as usize)
}
pub(in crate::x64::process::mono_startup) fn pointer(token: u64) -> Result<u64, String> {
    let (context, pointer) = STATE.with(|state| {
        let state = state.borrow();
        let record = state
            .records
            .get(&token)
            .filter(|r| r.live)
            .ok_or("culling handle unknown, retired or on foreign thread")?;
        Ok::<_, String>((record.context, record.pointer))
    })?;
    super::resolve(context)?;
    super::super::engine_objects::engine_pointer(pointer as usize)
}
pub(super) fn retire_context(context: u64) {
    STATE.with(|state| {
        for record in state
            .borrow_mut()
            .records
            .values_mut()
            .filter(|r| r.context == context)
        {
            record.live = false;
        }
    });
    // Retain private numeric snapshots until VM cleanup, as with the existing
    // unsafe-buffer arena. Escaped native unsafe views cannot dangle or reuse
    // storage; expired handles cannot enter the engine again. Capacity is finite.
}
pub(in crate::x64::process::mono_startup) fn retire_after_vm_cleanup() -> Result<(), String> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.records.values().any(|r| r.live) {
            return Err("culling views still live at VM cleanup".into());
        }
        state.records.clear();
        state.retained = 0;
        Ok(())
    })
}

/// Verify all nonstatic fields against their actual primitive/pointee classes.
/// Dynamic offsets are accepted; extra fields, unions and incomplete records
/// are rejected before installing any entry.
pub(in crate::x64::process::mono_startup) fn record_layout(
    library: Object,
    class: Object,
    specs: &[(&CStr, u32, Object)],
) -> Result<RecordLayout, String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing culling layout API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    closed_types::Api::load(library)?.concrete(class)?;
    let value = sym!(
        "mono_class_is_valuetype",
        unsafe extern "C" fn(Object) -> i32
    );
    let size = sym!(
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let fields = sym!(
        "mono_class_get_fields",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let name = sym!(
        "mono_field_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
    let parent = sym!(
        "mono_field_get_parent",
        unsafe extern "C" fn(Object) -> Object
    );
    let ty = sym!(
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let byref = sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32);
    let pointee = sym!(
        "mono_type_get_ptr_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let type_class = sym!(
        "mono_class_from_mono_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let offset = sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> i32);
    let mut alignment = 0;
    let width = unsafe { size(class, &mut alignment) };
    if unsafe { value(class) } == 0 || !(1..=128).contains(&width) || alignment != 8 {
        return Err("culling metadata is not an aligned bounded value".into());
    }
    let mut layout = RecordLayout {
        width: width as usize,
        alignment: alignment as usize,
        fields: vec![(0, 0); specs.len()],
    };
    let mut seen = vec![false; specs.len()];
    let mut iterator = ptr::null_mut();
    loop {
        let field = unsafe { fields(class, &mut iterator) };
        if field.is_null() {
            break;
        }
        if unsafe { flags(field) } & (0x10 | 0x40) != 0 {
            continue;
        }
        let text = unsafe { name(field) };
        let ty = unsafe { ty(field) };
        if text.is_null()
            || ty.is_null()
            || unsafe { parent(field) } != class
            || unsafe { byref(ty) } != 0
        {
            return Err("culling field actual declaration absent".into());
        }
        let index = specs
            .iter()
            .position(|s| s.0 == unsafe { CStr::from_ptr(text) })
            .ok_or("unexpected culling field")?;
        let (_, expected_kind, expected_class) = specs[index];
        let actual_kind = unsafe { kind(ty) };
        let actual = if actual_kind == 0xf {
            unsafe { pointee(ty) }
        } else {
            ty
        };
        if seen[index]
            || actual_kind != expected_kind
            || actual.is_null()
            || expected_class.is_null()
            || unsafe { type_class(actual) } != expected_class
        {
            return Err("culling field type/pointee is incompatible or duplicated".into());
        }
        let at = unsafe { offset(field) }
            .checked_sub(16)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("culling field is outside actual unboxed storage")?;
        layout.fields[index] = (at, if expected_kind == 8 { 4 } else { 8 });
        seen[index] = true;
    }
    if seen.contains(&false) {
        return Err("culling record is incomplete".into());
    }
    layout.check()?;
    Ok(layout)
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
                return Err(concat!("missing culling API: ", $name).into());
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
    let intptr = sym!("mono_get_intptr_class", unsafe extern "C" fn() -> Object);
    let integer = sym!("mono_get_int32_class", unsafe extern "C" fn() -> Object);
    let void = sym!("mono_get_void_class", unsafe extern "C" fn() -> Object);
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let lookup = sym!(
        "mono_lookup_internal_call",
        unsafe extern "C" fn(Object) -> Object
    );
    let class =
        |name: &CStr| unsafe { find(core, c"UnityEngine.Rendering".as_ptr(), name.as_ptr()) };
    let result_class = class(c"CullingResults");
    let allocation = class(c"CullingAllocationInfo");
    let light = class(c"VisibleLight");
    let reflection = class(c"VisibleReflectionProbe");
    let parameters = class(c"ScriptableCullingParameters");
    let context = class(c"ScriptableRenderContext");
    let intptr = unsafe { intptr() };
    let integer = unsafe { integer() };
    let void = unsafe { void() };
    if [
        result_class,
        allocation,
        light,
        reflection,
        parameters,
        context,
        intptr,
        integer,
        void,
    ]
    .iter()
    .any(|p| p.is_null())
    {
        return Err("culling protocol actual classes absent".into());
    }
    let result = record_layout(
        library,
        result_class,
        &[
            (c"ptr", 0x18, intptr),
            (c"m_AllocationInfo", 0xf, allocation),
        ],
    )?;
    let allocation = record_layout(
        library,
        allocation,
        &[
            (c"visibleLightsPtr", 0xf, light),
            (c"visibleOffscreenVertexLightsPtr", 0xf, light),
            (c"visibleReflectionProbesPtr", 0xf, reflection),
            (c"visibleLightCount", 8, integer),
            (c"visibleOffscreenVertexLightCount", 8, integer),
            (c"visibleReflectionProbeCount", 8, integer),
        ],
    )?;
    let api = value_layout::Api::load(library, core)?;
    let parameters = api.layout_in(parameters, images)?.numeric()?;
    let elements = [
        api.layout_in(light, images)?.numeric()?,
        api.layout_in(light, images)?.numeric()?,
        api.layout_in(reflection, images)?.numeric()?,
    ];
    let target_method = unsafe { method(context, c"Internal_Cull_Injected".as_ptr(), 3) };
    managed_calls::Api::load(library)?.fixed_typed_icall(
        target_method,
        context,
        &[
            (1, void, false),
            (0x11, class(c"ScriptableCullingParameters"), true),
            (0x11, context, true),
            (0x18, intptr, false),
        ],
        images,
    )?;
    if !unsafe { lookup(target_method) }.is_null() {
        return Err("culling bridge would replace an existing lifetime protocol".into());
    }
    let target = registry
        .registered_target("UnityEngine.Rendering.ScriptableRenderContext::Internal_Cull_Injected")?
        .ok_or("culling original engine target absent")?;
    BINDING
        .set(Binding {
            result_class: result_class as usize,
            target,
            result,
            allocation,
            parameters,
            elements,
        })
        .map_err(|_| "culling bridge already installed")?;
    unsafe {
        add(
            c"UnityEngine.Rendering.ScriptableRenderContext::Internal_Cull_Injected".as_ptr(),
            cull as Object,
        )
    };
    Ok(())
}
fn read_guest(
    memory: &crate::x64::memory::Memory,
    runtime: &crate::x64::runtime::Runtime64,
    address: u64,
    length: usize,
    alignment: usize,
) -> Result<Vec<u8>, String> {
    if length == 0 {
        return Ok(Vec::new());
    }
    if address == 0
        || !address.is_multiple_of(alignment as u64)
        || !(runtime.live_data_slot(address, length as u64)
            || memory.guest_private_data_range(address, length))
    {
        return Err("culling output is outside live original numeric engine storage".into());
    }
    let mut cursor = address;
    let end = address
        .checked_add(length as u64)
        .ok_or("culling buffer address overflow")?;
    while cursor < end {
        let size = (crate::x64::memory::PAGE - (cursor & (crate::x64::memory::PAGE - 1)))
            .min(end - cursor) as usize;
        if !memory.owned_noncode_access(cursor, size) {
            return Err("culling buffer aliases code or shared reference storage".into());
        }
        cursor += size as u64;
    }
    let mut bytes = vec![0; length];
    memory.read(address, &mut bytes)?;
    Ok(bytes)
}
fn snapshot(b: &Binding, result: &[u8], context: u64) -> Result<(u64, u64), String> {
    let pointer = b.result.word(result, 0)?;
    super::super::engine_objects::engine_pointer(pointer as usize)?;
    let allocation_pointer = b.result.word(result, 1)?;
    let (mut allocation, arrays) = callbacks::with_service(|memory, runtime| {
        let allocation = read_guest(
            memory,
            runtime,
            allocation_pointer,
            b.allocation.width,
            b.allocation.alignment,
        )?;
        let mut arrays = Vec::new();
        let mut total = b.allocation.width;
        for (index, layout) in b.elements.iter().enumerate() {
            let count = b.allocation.word(&allocation, index + 3)? as u32 as i32;
            if !(0..=4096).contains(&count) {
                return Err("culling array count outside bounded ABI".into());
            }
            let size = (count as usize)
                .checked_mul(layout.width)
                .ok_or("culling array size overflow")?;
            total += size;
            if total > 2 * 1024 * 1024 {
                return Err("culling output snapshot exceeds per-call capacity".into());
            }
            let mut bytes = read_guest(
                memory,
                runtime,
                b.allocation.word(&allocation, index)?,
                size,
                layout.alignment,
            )?;
            for element in bytes.chunks_exact_mut(layout.width) {
                layout.canonicalize(element)?;
            }
            arrays.push(Snapshot::new(&bytes));
        }
        Ok((
            allocation,
            <[Snapshot; 3]>::try_from(arrays).map_err(|_| "culling array count changed")?,
        ))
    })?;
    for (index, array) in arrays.iter().enumerate() {
        b.allocation.put(
            &mut allocation,
            index,
            if array.length == 0 {
                0
            } else {
                array.pointer()
            },
        );
    }
    b.allocation.canonicalize(&mut allocation)?;
    let allocation = Snapshot::new(&allocation);
    let bytes = allocation.length + arrays.iter().map(|a| a.words.len() * 8).sum::<usize>();
    let allocation_native = allocation.pointer();
    let serial = NEXT
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            (n < (1 << 32) / 16).then_some(n + 1)
        })
        .map_err(|_| "culling identity space exhausted")?;
    let token = BASE + serial * 16;
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.records.len() >= LIMIT || state.retained + bytes > BYTE_LIMIT {
            return Err("culling retained snapshot capacity exceeded".into());
        }
        state.retained += bytes;
        state.records.insert(
            token,
            Record {
                context,
                pointer,
                live: true,
                _allocation: allocation,
                _arrays: arrays,
            },
        );
        Ok((token, allocation_native))
    })
}
unsafe extern "C" fn cull(parameters: *mut u8, context: *const u64, output: usize) {
    match std::panic::catch_unwind(|| unsafe { cull_checked(parameters, context, output) }) {
        Ok(Ok(())) => {}
        error => {
            eprintln!("NATIVE_MONO_CULLING_REJECT: {error:?}");
            std::process::abort();
        }
    }
}
unsafe fn cull_checked(
    parameters: *mut u8,
    context: *const u64,
    output: usize,
) -> Result<(), String> {
    let b = BINDING.get().ok_or("culling protocol absent")?;
    if parameters.is_null()
        || !(parameters as usize).is_multiple_of(b.parameters.alignment)
        || context.is_null()
        || !(context as usize).is_multiple_of(8)
        || output == 0
        || !output.is_multiple_of(b.result.alignment)
        || output.checked_add(b.result.width).is_none()
    {
        return Err("culling native storage NULL/unaligned/overflow".into());
    }
    let context_token = unsafe { context.read() };
    let original_context = unsafe { super::native_payload(context as usize) }?;
    distinct_native_ranges(&[
        (parameters as usize, b.parameters.width),
        (context as usize, 8),
        (output, b.result.width),
    ])?;
    let mut input = unsafe { std::slice::from_raw_parts(parameters, b.parameters.width) }.to_vec();
    b.parameters.canonicalize(&mut input)?;
    let context_bytes = original_context.to_le_bytes();
    let output_bytes = vec![0; b.result.width];
    let (_, outputs) = callbacks::guest_value_arguments(
        b.target,
        &[0, 0, 0],
        &[],
        &[(0, &input), (1, &context_bytes), (2, &output_bytes)],
        &[],
    )?;
    if outputs.len() != 3 || outputs[0] != input || outputs[1] != context_bytes {
        return Err("engine modified readonly culling parameters/context".into());
    }
    let (token, allocation) = snapshot(b, &outputs[2], context_token)?;
    let mut native = vec![0; b.result.width];
    b.result.put(&mut native, 0, token);
    b.result.put(&mut native, 1, allocation);
    unsafe { ptr::copy_nonoverlapping(native.as_ptr(), output as *mut u8, native.len()) };
    eprintln!("NATIVE_MONO_CULLING_VERIFIED: context={context_token:#x}; result={token:#x}; original-engine=true; native-numeric-snapshots=true");
    Ok(())
}

fn distinct_native_ranges(ranges: &[(usize, usize)]) -> Result<(), String> {
    for (index, &(start, length)) in ranges.iter().enumerate() {
        let end = start
            .checked_add(length)
            .ok_or("culling native storage extent overflow")?;
        for &(other, size) in &ranges[..index] {
            if start
                < other
                    .checked_add(size)
                    .ok_or("culling native storage extent overflow")?
                && other < end
            {
                return Err("culling native parameters/context/output overlap".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_guest_cull_publishes_owned_numeric_arrays_and_commits_only_valid_outputs() {
        use crate::x64::{
            memory::{Memory, EXECUTE, READ, WRITE},
            runtime::Runtime64,
        };
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        let base = memory.virtual_alloc(0, 16384, 0x3000, 4).unwrap();
        let allocation = base + 128;
        let array = base + 4088;
        // Return real guest-private pointers in the scoped output slot. The
        // array straddles pages; there is no native pointer in guest Memory.
        let mut code = vec![0x48, 0xb8];
        code.extend(base.to_le_bytes());
        code.extend([0x49, 0x89, 0x00, 0x48, 0xb8]);
        code.extend(allocation.to_le_bytes());
        code.extend([0x49, 0x89, 0x40, 0x08, 0x31, 0xc0, 0xc3]);
        memory.write(0x1000, &code).unwrap();
        memory.write_int(allocation, 8, array).unwrap();
        memory.write_int(allocation + 24, 4, 2).unwrap();
        memory
            .write(
                array,
                &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            )
            .unwrap();
        memory
            .map(super::super::super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            super::super::super::super::STACK,
            65536,
            "culling-fixture",
        )
        .unwrap();
        let numeric = value_layout::Layout {
            class: 1usize as Object,
            width: 8,
            alignment: 8,
            value: true,
            slots: vec![value_layout::Slot {
                nested: true,
                offset: 0,
                field: ptr::null_mut(),
                shape: value_layout::Shape::Inline {
                    width: 8,
                    alignment: 8,
                },
            }],
        }
        .numeric()
        .unwrap();
        assert!(BINDING
            .set(Binding {
                result_class: 1,
                target: 0x1000,
                result: RecordLayout {
                    width: 16,
                    alignment: 8,
                    fields: vec![(0, 8), (8, 8)]
                },
                allocation: RecordLayout {
                    width: 40,
                    alignment: 8,
                    fields: vec![(0, 8), (8, 8), (16, 8), (24, 4), (28, 4), (32, 4)]
                },
                parameters: numeric.clone(),
                elements: std::array::from_fn(|_| numeric.clone())
            })
            .is_ok());
        let (checked, _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            let scope = super::super::test_scope(base).unwrap();
            let context = super::super::test_token(&scope);
            let mut parameters = 0u64;
            let mut output = [0xccccccccccccccccu64; 2];
            unsafe {
                cull_checked(
                    (&mut parameters as *mut u64).cast(),
                    &context,
                    output.as_mut_ptr() as usize,
                )
            }?;
            let token = output[0];
            assert_eq!(pointer(token)?, base);
            assert_ne!(output[1], allocation);
            let array_native = unsafe { (output[1] as *const u64).read() };
            let owned = unsafe { std::slice::from_raw_parts(array_native as *const u8, 16) };
            assert_eq!(
                owned,
                &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
            );
            callbacks::with_service(|memory, _| {
                memory.write(array, &[0; 16])?;
                assert!(memory.check(array_native, 1, READ).is_err());
                memory.write_int(allocation + 24, 4, u32::MAX as u64)
            })?;
            assert_eq!(
                owned,
                &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
            );
            let prior = output;
            assert!(unsafe {
                cull_checked(
                    (&mut parameters as *mut u64).cast(),
                    &context,
                    output.as_mut_ptr() as usize,
                )
            }
            .is_err());
            assert_eq!(output, prior);
            callbacks::with_service(|memory, _| memory.write_int(allocation + 24, 4, 4097))?;
            assert!(unsafe {
                cull_checked(
                    (&mut parameters as *mut u64).cast(),
                    &context,
                    output.as_mut_ptr() as usize,
                )
            }
            .is_err());
            assert_eq!(output, prior);
            drop(scope);
            assert!(pointer(token).is_err());
            retire_after_vm_cleanup()?;
            Ok::<_, String>(())
        })
        .unwrap();
        checked.unwrap();
        assert!(memory.guest_private_data_range(base + 4088, 16));
        assert!(!memory.guest_private_data_range(base + 16376, 16));
        memory.virtual_protect(base + 4096, 4096, 0x40).unwrap();
        memory.virtual_protect(base + 4096, 4096, 4).unwrap();
        assert!(!memory.guest_private_data_range(base + 4088, 16));
    }
    #[test]
    fn semantic_record_rejects_overlap_unaligned_and_out_of_extent_fields() {
        let mut record = RecordLayout {
            width: 24,
            alignment: 8,
            fields: vec![(0, 8), (8, 8), (16, 4)],
        };
        record.check().unwrap();
        let mut bytes = vec![0; 24];
        record.put(&mut bytes, 1, 0xfedcba9876543210);
        assert_eq!(record.word(&bytes, 1).unwrap(), 0xfedcba9876543210);
        bytes[20..24].fill(0xcc);
        record.canonicalize(&mut bytes).unwrap();
        assert_eq!(&bytes[20..24], &[0; 4]);
        record.fields[2] = (12, 4);
        assert!(record.check().is_err());
        record.fields[2] = (17, 4);
        assert!(record.check().is_err());
        record.fields[2] = (24, 4);
        assert!(record.check().is_err());
        assert!(record.word(&bytes[..23], 0).is_err());
        distinct_native_ranges(&[(0x1000, 24), (0x1020, 8)]).unwrap();
        assert!(distinct_native_ranges(&[(0x1000, 24), (0x1010, 8)]).is_err());
        assert!(distinct_native_ranges(&[(0x1000, 8), (0xffc, 8)]).is_err());
        assert!(distinct_native_ranges(&[(usize::MAX - 7, 16)]).is_err());
    }
    #[test]
    fn borrowed_handle_rejects_forgery_context_retirement_and_foreign_thread() {
        let scope = super::super::test_scope(0x1000).unwrap();
        let context = super::super::test_token(&scope);
        let token = BASE + 0x123400;
        STATE.with(|state| {
            state.borrow_mut().records.insert(
                token,
                Record {
                    context,
                    pointer: 0x2000,
                    live: true,
                    _allocation: Snapshot::new(&[0; 40]),
                    _arrays: std::array::from_fn(|_| Snapshot::new(&[])),
                },
            )
        });
        assert!(pointer(0x2000).is_err());
        assert!(std::thread::spawn(move || pointer(token))
            .join()
            .unwrap()
            .is_err());
        retire_context(context);
        assert!(pointer(token).is_err());
        drop(scope);
        retire_after_vm_cleanup().unwrap();
    }
}
