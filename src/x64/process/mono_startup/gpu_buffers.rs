//! Fixed GPU buffer ABIs. Managed IntPtr stores a never-reused opaque token;
//! only this registry resolves it to an original Unity engine allocation.
use super::{bindings, callbacks, engine_objects, logging, managed_calls, Object};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{CStr, CString},
    sync::OnceLock,
};
const COUNT: usize = 11;
const BASE: u64 = 0x6ff500000000;
const LIMIT: usize = 512 * 1024 * 1024;
struct Binding {
    targets: [[u64; COUNT]; 2],
    library: usize,
    array: usize,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
#[derive(Clone, Copy)]
struct Record {
    guest: u64,
    kind: usize,
    count: usize,
    stride: usize,
}
#[derive(Default)]
struct Arena {
    live: BTreeMap<u64, Record>,
    next: usize,
    closed: bool,
}
thread_local! { static ARENA: RefCell<Arena> = RefCell::new(Arena::default()); }
impl Arena {
    fn insert(&mut self, record: Record) -> Result<u64, String> {
        if self.closed
            || self.next >= 65536
            || self.live.len() >= 4096
            || self.live.values().any(|r| r.guest == record.guest)
        {
            return Err("GPU buffer ownership/capacity mismatch".into());
        }
        self.next += 1;
        let token = BASE + self.next as u64 * 16;
        self.live.insert(token, record);
        Ok(token)
    }
    fn get(&self, token: u64, kind: usize) -> Result<Record, String> {
        self.live
            .get(&token)
            .filter(|r| !self.closed && r.kind == kind)
            .copied()
            .ok_or_else(|| "GPU buffer requires exact live token and allocation family".into())
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
        ($name:literal,$ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing GPU buffer API: ", $name).into());
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
    let name = sym!(
        "mono_class_get_name",
        unsafe extern "C" fn(Object) -> *const std::ffi::c_char
    );
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let primitive = |name: &CStr| unsafe { find(corlib(), c"System".as_ptr(), name.as_ptr()) };
    let unity = |name: &CStr| unsafe { find(core, c"UnityEngine".as_ptr(), name.as_ptr()) };
    let void = (1, primitive(c"Void"), false);
    let intptr = (0x18, primitive(c"IntPtr"), false);
    let int32 = (8, primitive(c"Int32"), false);
    let uint32 = (9, primitive(c"UInt32"), false);
    let boolean = (2, primitive(c"Boolean"), false);
    let array = primitive(c"Array");
    let span = unity(c"ManagedSpanWrapper");
    let span = if span.is_null() {
        unsafe {
            find(
                core,
                c"UnityEngine.Bindings".as_ptr(),
                c"ManagedSpanWrapper".as_ptr(),
            )
        }
    } else {
        span
    };
    logging::validate_span_layout(library, span)?;
    let api = managed_calls::Api::load(library)?;
    let mut targets = [[0; COUNT]; 2];
    let mut registrations = Vec::new();
    for kind in 0..2 {
        let owner = unity(if kind == 0 {
            c"ComputeBuffer"
        } else {
            c"GraphicsBuffer"
        });
        let nested_class = |label: &CStr| -> Result<Object, String> {
            let mut iterator = std::ptr::null_mut();
            for _ in 0..16 {
                let class = unsafe { nested(owner, &mut iterator) };
                if class.is_null() {
                    break;
                }
                let n = unsafe { name(class) };
                if !n.is_null() && unsafe { CStr::from_ptr(n) } == label {
                    return Ok(class);
                }
            }
            Err("GPU buffer enum class absent".into())
        };
        let (target, usage) = if kind == 0 {
            (unity(c"ComputeBufferType"), unity(c"ComputeBufferMode"))
        } else {
            (nested_class(c"Target")?, nested_class(c"UsageFlags")?)
        };
        let target = (0x11, target, false);
        let usage = (0x11, usage, false);
        let signatures = [
            if kind == 0 {
                vec![intptr, int32, int32, target, usage]
            } else {
                vec![intptr, target, usage, int32, int32]
            },
            vec![void, intptr],
            vec![boolean, intptr],
            vec![int32, intptr],
            vec![int32, intptr],
            vec![usage, intptr],
            vec![void, intptr, (0x11, span, true)],
            vec![void, intptr, uint32],
            vec![void, intptr, intptr, int32],
            vec![
                void,
                intptr,
                (0x12, array, false),
                int32,
                int32,
                int32,
                int32,
            ],
            vec![
                void,
                intptr,
                (0x12, array, false),
                int32,
                int32,
                int32,
                int32,
            ],
        ];
        for (index, signature) in signatures.iter().enumerate() {
            let member = match (kind, index) {
                (1, 5) => c"GetUsageFlags_Injected",
                (1, 8) => c"CopyCountGG_Injected",
                _ => MEMBERS[index],
            };
            api.fixed_typed_icall(
                unsafe { method(owner, member.as_ptr(), (signature.len() - 1) as i32) },
                owner,
                signature,
                images,
            )?;
            let binding = CString::new(format!(
                "UnityEngine.{}::{}",
                if kind == 0 {
                    "ComputeBuffer"
                } else {
                    "GraphicsBuffer"
                },
                member.to_str().unwrap()
            ))
            .unwrap();
            targets[kind][index] =
                registry.exact_target(binding.to_str().unwrap(), TARGETS[kind][index])?;
            registrations.push((binding, ADAPTERS[kind][index]));
        }
    }
    BINDING
        .set(Binding {
            targets,
            library: library as usize,
            array: array as usize,
        })
        .map_err(|_| "GPU buffers already installed")?;
    for (binding, adapter) in registrations {
        unsafe { add(binding.as_ptr(), adapter as Object) };
    }
    Ok(())
}
const MEMBERS: [&CStr; COUNT] = [
    c"InitBuffer",
    c"DestroyBuffer_Injected",
    c"IsValidBuffer_Injected",
    c"get_count_Injected",
    c"get_stride_Injected",
    c"get_usage_Injected",
    c"SetName_Injected",
    c"SetCounterValue_Injected",
    c"CopyCount_Injected",
    c"InternalSetData_Injected",
    c"InternalGetData_Injected",
];
const TARGETS: [[u64; COUNT]; 2] = [
    [
        0x1800c4720,
        0x1800c4850,
        0x18009b220,
        0x18007ab10,
        0x18007ab50,
        0x1800c4a50,
        0x18009b640,
        0x18009ba00,
        0x1800c4a60,
        0x18009b3d0,
        0x18009b510,
    ],
    [
        0x18009af70,
        0x18009b020,
        0x18009b220,
        0x18007ab10,
        0x18007ab50,
        0x18009b280,
        0x18009b640,
        0x18009ba00,
        0x18009ba50,
        0x18009b3d0,
        0x18009b510,
    ],
];
fn checked<T>(work: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_GPU_BUFFER_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_GPU_BUFFER_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
fn target(kind: usize, index: usize) -> Result<u64, String> {
    Ok(BINDING.get().ok_or("GPU buffer adapters absent")?.targets[kind][index])
}
fn record(token: usize, kind: usize) -> Result<Record, String> {
    ARENA.with(|a| a.borrow().get(token as u64, kind))
}
/// Buffer consumers resolve only live family-qualified tokens. NULL retains
/// Unity's unbind semantics; original allocations never escape to native IL.
pub(super) fn binding_pointer(
    token: usize,
    kind: usize,
    range: Option<(i32, i32)>,
) -> Result<u64, String> {
    callbacks::with_service(|_, _| Ok(()))?;
    if let Some((offset, size)) = range {
        if offset < 0 || size < 0 {
            return Err("GPU constant buffer negative range".into());
        }
    }
    if token == 0 {
        return Ok(0);
    }
    let buffer = record(token, kind)?;
    if let Some((offset, size)) = range {
        if (offset as usize)
            .checked_add(size as usize)
            .is_none_or(|end| end > buffer.count * buffer.stride)
        {
            return Err("GPU constant buffer range outside allocation".into());
        }
    }
    Ok(buffer.guest)
}
fn size(count: i32, stride: i32) -> Result<usize, String> {
    if count <= 0 || stride <= 0 || stride > 2048 || stride % 4 != 0 {
        return Err("GPU buffer count/stride outside bounded ABI".into());
    }
    let size = (count as usize)
        .checked_mul(stride as usize)
        .ok_or("GPU buffer size overflow")?;
    if size > LIMIT {
        return Err("GPU buffer exceeds bounded allocation".into());
    }
    Ok(size)
}
fn init(kind: usize, args: [i32; 4], count: i32, stride: i32) -> usize {
    checked(|| {
        size(count, stride)?;
        let guest = callbacks::guest_arguments(target(kind, 0)?, &args.map(|a| a as u32 as u64))?;
        if guest == 0 {
            return Ok(0);
        }
        engine_objects::engine_pointer(guest as usize)?;
        let registered = ARENA.with(|a| {
            a.borrow_mut().insert(Record {
                guest,
                kind,
                count: count as usize,
                stride: stride as usize,
            })
        });
        let token = match registered {
            Ok(token) => token,
            Err(error) => {
                callbacks::guest_arguments(target(kind, 1)?, &[guest])?;
                return Err(error);
            }
        };
        eprintln!("NATIVE_MONO_GPU_BUFFER_CREATED: kind={kind}; count={count}; stride={stride}; original-engine=true; opaque-token={token:#x}");
        Ok(token as usize)
    })
}
fn destroy(kind: usize, token: usize) {
    checked(|| {
        let guest = if token == 0 {
            0
        } else {
            record(token, kind)?.guest
        };
        callbacks::guest_arguments(target(kind, 1)?, &[guest])?;
        if token != 0 {
            ARENA.with(|a| a.borrow_mut().live.remove(&(token as u64)));
        }
        Ok(())
    });
}
fn scalar(kind: usize, index: usize, token: usize) -> u64 {
    checked(|| {
        let guest = if token == 0 && index == 2 {
            0
        } else {
            record(token, kind)?.guest
        };
        let result = callbacks::guest_arguments(target(kind, index)?, &[guest])?;
        if index == 2 && result & 255 > 1 {
            return Err("GPU buffer validity Boolean is not canonical".into());
        }
        Ok(result)
    })
}
unsafe fn set_name(kind: usize, token: usize, span: *const logging::Span) {
    checked(|| {
        let units = unsafe { logging::owned_span(span)? };
        callbacks::guest_positioned_string_span(
            target(kind, 6)?,
            units.as_deref(),
            &[record(token, kind)?.guest],
            &[],
        )?;
        Ok(())
    });
}
fn set_counter(kind: usize, token: usize, value: u32) {
    checked(|| {
        callbacks::guest_arguments(
            target(kind, 7)?,
            &[record(token, kind)?.guest, value as u64],
        )?;
        Ok(())
    });
}
fn copy_count(kind: usize, src: usize, dst: usize, offset: i32) {
    checked(|| {
        let src = record(src, kind)?;
        let dst = record(dst, kind)?;
        if offset < 0 || offset % 4 != 0 || offset as usize + 4 > dst.count * dst.stride {
            return Err("GPU counter destination out of range".into());
        }
        callbacks::guest_arguments(target(kind, 8)?, &[src.guest, dst.guest, offset as u64])?;
        Ok(())
    });
}
unsafe fn data(kind: usize, index: usize, token: usize, array: Object, ranges: [i32; 4]) {
    checked(|| {
        let b = BINDING.get().ok_or("GPU data adapters absent")?;
        let buffer = record(token, kind)?;
        managed_calls::Api::load(b.library as Object)?.compatible(
            array,
            managed_calls::Shape::Reference {
                class: b.array as Object,
                string: false,
            },
        )?;
        let (array, length, width) = callbacks::with_metadata_service(|route, memory| {
            route.root_blittable_array(array, memory)
        })?;
        let [start, destination, count, stride] = ranges;
        if start < 0
            || destination < 0
            || count < 0
            || stride <= 0
            || stride as usize != width
            || start as usize + count as usize > length
            || (destination as usize + count as usize)
                .checked_mul(width)
                .is_none_or(|end| end > buffer.count * buffer.stride)
        {
            return Err("GPU data source/destination/element size out of range".into());
        }
        callbacks::guest_arguments(
            target(kind, index)?,
            &[
                buffer.guest,
                array,
                start as u64,
                destination as u64,
                count as u64,
                stride as u64,
            ],
        )?;
        eprintln!("NATIVE_MONO_GPU_BUFFER_DATA: kind={kind}; readback={}; count={count}; element-bytes={stride}; original-engine=true", index == 10);
        Ok(())
    });
}
macro_rules! adapters {
    ($kind:literal,$init:ident,$destroy:ident,$valid:ident,$count:ident,$stride:ident,$usage:ident,$name:ident,$counter:ident,$copy:ident,$set:ident,$get:ident) => {
        unsafe extern "C" fn $init(a: i32, b: i32, c: i32, d: i32) -> usize {
            if $kind == 0 {
                init($kind, [a, b, c, d], a, b)
            } else {
                init($kind, [a, b, c, d], c, d)
            }
        }
        unsafe extern "C" fn $destroy(token: usize) {
            destroy($kind, token)
        }
        unsafe extern "C" fn $valid(token: usize) -> u8 {
            scalar($kind, 2, token) as u8
        }
        unsafe extern "C" fn $count(token: usize) -> i32 {
            scalar($kind, 3, token) as i32
        }
        unsafe extern "C" fn $stride(token: usize) -> i32 {
            scalar($kind, 4, token) as i32
        }
        unsafe extern "C" fn $usage(token: usize) -> i32 {
            scalar($kind, 5, token) as i32
        }
        unsafe extern "C" fn $name(token: usize, span: *const logging::Span) {
            unsafe { set_name($kind, token, span) }
        }
        unsafe extern "C" fn $counter(token: usize, value: u32) {
            set_counter($kind, token, value)
        }
        unsafe extern "C" fn $copy(src: usize, dst: usize, offset: i32) {
            copy_count($kind, src, dst, offset)
        }
        unsafe extern "C" fn $set(token: usize, array: Object, a: i32, b: i32, c: i32, d: i32) {
            unsafe { data($kind, 9, token, array, [a, b, c, d]) }
        }
        unsafe extern "C" fn $get(token: usize, array: Object, a: i32, b: i32, c: i32, d: i32) {
            unsafe { data($kind, 10, token, array, [a, b, c, d]) }
        }
    };
}
adapters!(
    0, c_init, c_destroy, c_valid, c_count, c_stride, c_usage, c_name, c_counter, c_copy, c_set,
    c_get
);
adapters!(
    1, g_init, g_destroy, g_valid, g_count, g_stride, g_usage, g_name, g_counter, g_copy, g_set,
    g_get
);
const ADAPTERS: [[*const (); COUNT]; 2] = [
    [
        c_init as *const (),
        c_destroy as *const (),
        c_valid as *const (),
        c_count as *const (),
        c_stride as *const (),
        c_usage as *const (),
        c_name as *const (),
        c_counter as *const (),
        c_copy as *const (),
        c_set as *const (),
        c_get as *const (),
    ],
    [
        g_init as *const (),
        g_destroy as *const (),
        g_valid as *const (),
        g_count as *const (),
        g_stride as *const (),
        g_usage as *const (),
        g_name as *const (),
        g_counter as *const (),
        g_copy as *const (),
        g_set as *const (),
        g_get as *const (),
    ],
];
pub(super) fn retire_after_vm_cleanup() -> Result<(), String> {
    let records = ARENA.with(|a| {
        a.borrow()
            .live
            .iter()
            .map(|(&t, &r)| (t, r))
            .collect::<Vec<_>>()
    });
    for (token, r) in records {
        callbacks::guest_arguments(target(r.kind, 1)?, &[r.guest])?;
        ARENA.with(|a| a.borrow_mut().live.remove(&token));
    }
    ARENA.with(|a| a.borrow_mut().closed = true);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buffer_tokens_do_not_alias_after_engine_reuse_and_keep_family() {
        let mut arena = Arena::default();
        let r = Record {
            guest: 0x2000,
            kind: 0,
            count: 4,
            stride: 16,
        };
        let old = arena.insert(r).unwrap();
        assert!(arena.get(old, 1).is_err());
        assert!(arena.get(old + 1, 0).is_err());
        assert!(arena.insert(r).is_err());
        arena.live.remove(&old);
        let next = arena.insert(r).unwrap();
        assert_ne!(old, next);
        assert!(arena.get(old, 0).is_err());
        arena.closed = true;
        assert!(arena.get(next, 0).is_err());
        for (count, stride) in [(0, 16), (4, 3), (-1, 16), (i32::MAX, 2048)] {
            assert!(size(count, stride).is_err());
        }
    }
}
