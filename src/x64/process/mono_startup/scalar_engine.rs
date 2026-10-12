//! Metadata-discovered scalar/value engine bridge. The signed binary contains
//! one fixed ARM64 register-capture entry pool, independent of method names,
//! MethodDef tokens, assembly hashes, signature combinations and guest RVAs.
//! Engine references/arrays use the existing rooted container protocol. Other
//! pointer/callback/lifetime shapes stay outside the ordinary value ABI.
use super::{bindings, callbacks, engine_objects, images, managed_calls, value_layout, Object};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString},
    ptr,
    sync::OnceLock,
};
const POOL_CAPACITY: usize = 8192;
const ARGUMENT_CAPACITY: usize = 16;
// A semantic policy, not per-method ABI specifications. These families operate
// on ordinary numeric state; capability-like Int64/IntPtr APIs are not inferred.
const FAMILIES: &[(&CStr, &CStr)] = &[
    (c"UnityEngine", c"Application"),
    (c"UnityEngine", c"SystemInfo"),
    (c"UnityEngine", c"QualitySettings"),
    (c"UnityEngine", c"Time"),
    (c"UnityEngine", c"Screen"),
    (c"UnityEngine", c"Display"),
    (c"UnityEngine", c"HDROutputSettings"),
    (c"UnityEngine", c"Cursor"),
    (c"UnityEngine", c"AudioSettings"),
    (c"UnityEngine", c"AudioListener"),
    (c"UnityEngine", c"Input"),
    (c"UnityEngine", c"CameraRaycastHelper"),
    (c"UnityEngine", c"Physics"),
    (c"UnityEngine", c"Physics2D"),
    (c"UnityEngine", c"Random"),
    (c"UnityEngine", c"HumanTrait"),
    (c"UnityEngine", c"DynamicGI"),
    (c"UnityEngine", c"Debug"),
    (c"UnityEngine", c"DebugLogHandler"),
    (c"UnityEngine", c"GUI"),
    (c"UnityEngine", c"GUIUtility"),
    (c"UnityEngine", c"Mathf"),
    (c"UnityEngine", c"GL"),
    (c"UnityEngine", c"Graphics"),
    (c"UnityEngine", c"Canvas"),
    (c"UnityEngine", c"UISystemProfilerApi"),
    (c"UnityEngine.TextCore.LowLevel", c"FontEngine"),
    (c"UnityEngine.Rendering", c"GraphicsSettings"),
    (c"UnityEngine.Rendering", c"SplashScreen"),
    (
        c"UnityEngine.Experimental.Rendering",
        c"BuiltinRuntimeReflectionSystem",
    ),
    (
        c"UnityEngine.Experimental.Rendering",
        c"GraphicsFormatUtility",
    ),
    (c"UnityEngine", c"Bounds"),
    (c"UnityEngine", c"Matrix4x4"),
    (c"UnityEngine", c"Vector3"),
    (c"UnityEngine", c"Quaternion"),
];
#[derive(Clone, Copy, Debug)]
struct Scalar {
    kind: u32,
    width: usize,
}
impl Scalar {
    fn from_kind(kind: u32) -> Option<Self> {
        let width = match kind {
            1 => 0,
            2 => 1,
            3 | 7 => 2,
            8 | 9 | 12 => 4,
            13 => 8,
            _ => return None,
        };
        Some(Self { kind, width })
    }
    fn floating(self) -> bool {
        matches!(self.kind, 12 | 13)
    }
    fn normalize(self, value: u64) -> Result<u64, String> {
        let value = match self.width {
            0 => 0,
            8 => value,
            width => value & ((1u64 << (width * 8)) - 1),
        };
        if self.kind == 2 && value > 1 {
            return Err("engine Boolean is not canonical".into());
        }
        Ok(value)
    }
}
#[derive(Clone, Copy, Debug)]
struct Payload {
    width: usize,
    alignment: usize,
    output: bool,
}
#[derive(Clone, Copy, Debug)]
enum Argument {
    Scalar(Scalar),
    Value(Payload),
    EngineSelf,
    EnginePointer { nullable: bool },
    EngineObject { class: usize },
    EngineArray { class: usize },
    Exception { class: usize },
    DataArray { class: usize },
    Utf16Span,
}
struct Binding {
    library: usize,
    name: CString,
    target: u64,
    arguments: Vec<Argument>,
    returns: Return,
}
enum Return {
    Scalar(Scalar),
    EngineHandle { class: usize },
}
impl Return {
    fn floating(&self) -> bool {
        matches!(self, Self::Scalar(scalar) if scalar.floating())
    }
}
static BINDINGS: OnceLock<Vec<Binding>> = OnceLock::new();
thread_local! { static SEEN: RefCell<BTreeSet<usize>> = const { RefCell::new(BTreeSet::new()) }; }
#[repr(C)]
#[derive(Default)]
struct Capture {
    integer: [u64; 8],
    floating: [u64; 8],
    stack: usize,
}
#[repr(C)]
#[derive(Debug)]
struct Registers {
    integer: u64,
    floating: u64,
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod native_pool {
    use super::*;
    // Each entry is two compile-time instructions. Nothing is emitted, patched
    // or made executable at runtime. Only the private descriptor table varies.
    std::arch::global_asm!(r#"
        .section __TEXT,__text,regular,pure_instructions
        .p2align 3
        .globl _wire_mono_engine_entry_pool
        .private_extern _wire_mono_engine_entry_pool
    _wire_mono_engine_entry_pool:
        .set wire_engine_slot, 0
        .rept {capacity}
        mov x16, #wire_engine_slot
        b Lwire_engine_capture
        .set wire_engine_slot, wire_engine_slot + 1
        .endr
    Lwire_engine_capture:
        sub sp, sp, #176
        stp x29, x30, [sp]
        mov x29, sp
        stp x0, x1, [sp, #16]
        stp x2, x3, [sp, #32]
        stp x4, x5, [sp, #48]
        stp x6, x7, [sp, #64]
        stp d0, d1, [sp, #80]
        stp d2, d3, [sp, #96]
        stp d4, d5, [sp, #112]
        stp d6, d7, [sp, #128]
        add x17, sp, #176
        str x17, [sp, #144]
        mov x0, x16
        add x1, sp, #16
        bl _wire_mono_engine_pool_dispatch
        fmov d0, x1
        ldp x29, x30, [sp]
        add sp, sp, #176
        ret
    "#, capacity = const POOL_CAPACITY);
    extern "C" {
        fn wire_mono_engine_entry_pool();
    }
    pub(super) fn entry(index: usize) -> Result<Object, String> {
        if index >= POOL_CAPACITY {
            return Err("compiled engine entry pool exhausted".into());
        }
        Ok(unsafe { (wire_mono_engine_entry_pool as *const u8).add(index * 8) } as Object)
    }
}
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod native_pool {
    use super::*;
    pub(super) fn entry(_: usize) -> Result<Object, String> {
        Err("compiled engine bridge requires macOS ARM64".into())
    }
}
#[no_mangle]
unsafe extern "C" fn wire_mono_engine_pool_dispatch(
    index: usize,
    capture: *const Capture,
) -> Registers {
    match std::panic::catch_unwind(|| -> Result<Registers, String> {
        let bindings = BINDINGS.get().ok_or("metadata engine bridge absent")?;
        let binding = bindings.get(index).ok_or("unassigned engine entry slot")?;
        if capture.is_null() {
            return Err("engine register capture absent".into());
        }
        let arguments = unsafe { decode(binding, &*capture) }?;
        let result = execute(binding, &arguments)?;
        if SEEN.with(|seen| seen.borrow_mut().insert(index)) {
            eprintln!("NATIVE_MONO_SCALAR_ENGINE_VERIFIED: binding={:?}; metadata-discovered=true; original-engine=true", binding.name);
        }
        Ok(Registers {
            integer: result.0,
            floating: result.1,
        })
    }) {
        Ok(Ok(value)) => value,
        other => {
            let name = BINDINGS
                .get()
                .and_then(|bindings| bindings.get(index))
                .map(|binding| &binding.name);
            eprintln!(
                "NATIVE_MONO_SCALAR_ENGINE_REJECT: slot={index}; binding={name:?}; {other:?}"
            );
            std::process::abort();
        }
    }
}
// Darwin ARM64 packs spilled scalar arguments at their natural alignment;
// Win64 always uses positional eight-byte stack slots. The verified Mono call
// owns this stack span for the complete synchronous register-capture dispatch.
unsafe fn decode(binding: &Binding, capture: &Capture) -> Result<Vec<u64>, String> {
    if binding.arguments.len() > ARGUMENT_CAPACITY {
        return Err("engine signature exceeds compiled register capture".into());
    }
    let (mut integer, mut floating, mut stack_offset) = (0, 0, 0usize);
    binding
        .arguments
        .iter()
        .map(|argument| {
            let scalar = match argument {
                Argument::Scalar(s) => Some(*s),
                _ => None,
            };
            if scalar.is_some_and(|s| s.width == 0) {
                return Err("void is not an engine argument".into());
            }
            let floating_lane = scalar.is_some_and(Scalar::floating);
            let lane = if floating_lane {
                &mut floating
            } else {
                &mut integer
            };
            let word = if *lane < 8 {
                let word = if floating_lane {
                    capture.floating[*lane]
                } else {
                    capture.integer[*lane]
                };
                *lane += 1;
                word
            } else {
                let width = scalar.map_or(8, |s| s.width);
                stack_offset = (stack_offset + width - 1) & !(width - 1);
                if capture.stack == 0 || stack_offset + width > ARGUMENT_CAPACITY * 8 {
                    return Err("engine spilled argument stack absent or exceeds bound".into());
                }
                let address = capture
                    .stack
                    .checked_add(stack_offset)
                    .ok_or("engine stack argument overflow")?;
                let mut bytes = [0; 8];
                unsafe {
                    ptr::copy_nonoverlapping(address as *const u8, bytes.as_mut_ptr(), width)
                };
                stack_offset += width;
                u64::from_le_bytes(bytes)
            };
            scalar.map_or(Ok(word), |s| s.normalize(word))
        })
        .collect()
}
pub(super) fn installed() -> bool {
    BINDINGS.get().is_some()
}
pub(super) fn install(
    library: Object,
    images: &images::Registry,
    registry: &bindings::Registry,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing metadata engine API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, *const libc::c_char) -> Object
    );
    let methods = sym!(
        "mono_class_get_methods",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let flags = sym!(
        "mono_method_get_flags",
        unsafe extern "C" fn(Object, *mut u32) -> u32
    );
    let owner_of = sym!(
        "mono_method_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let member = sym!(
        "mono_method_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let param_names = sym!(
        "mono_method_get_param_names",
        unsafe extern "C" fn(Object, *mut *const libc::c_char)
    );
    let class_get = sym!(
        "mono_class_get",
        unsafe extern "C" fn(Object, u32) -> Object
    );
    let class_type = sym!(
        "mono_class_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let type_name = sym!(
        "mono_type_get_name_full",
        unsafe extern "C" fn(Object, i32) -> *mut libc::c_char
    );
    let image_name = sym!(
        "mono_image_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let assignable = sym!(
        "mono_class_is_assignable_from",
        unsafe extern "C" fn(Object, Object) -> i32
    );
    let element_class = sym!(
        "mono_class_get_element_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let rank = sym!("mono_class_get_rank", unsafe extern "C" fn(Object) -> u32);
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let free = sym!("mono_free", unsafe extern "C" fn(Object));
    let signature = sym!(
        "mono_method_signature",
        unsafe extern "C" fn(Object) -> Object
    );
    let count = sym!(
        "mono_signature_get_param_count",
        unsafe extern "C" fn(Object) -> u32
    );
    let returns = sym!(
        "mono_signature_get_return_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let parameter = sym!(
        "mono_signature_get_params",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let byref = sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32);
    let type_class = sym!(
        "mono_class_from_mono_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let enum_base = sym!(
        "mono_class_enum_basetype",
        unsafe extern "C" fn(Object) -> Object
    );
    let is_enum = sym!("mono_class_is_enum", unsafe extern "C" fn(Object) -> i32);
    let image_of = sym!(
        "mono_class_get_image",
        unsafe extern "C" fn(Object) -> Object
    );
    let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
    let table = sym!(
        "mono_image_get_table_info",
        unsafe extern "C" fn(Object, i32) -> Object
    );
    let rows = sym!(
        "mono_table_info_get_rows",
        unsafe extern "C" fn(Object) -> i32
    );
    let row_col = sym!(
        "mono_metadata_decode_row_col",
        unsafe extern "C" fn(Object, i32, u32) -> u32
    );
    let lookup = sym!(
        "mono_lookup_internal_call",
        unsafe extern "C" fn(Object) -> Object
    );
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let family = images.live_native_images()?;
    let api = managed_calls::Api::load(library)?;
    let intptr = unsafe { find(corlib(), c"System".as_ptr(), c"IntPtr".as_ptr()) };
    let exception = unsafe { find(corlib(), c"System".as_ptr(), c"Exception".as_ptr()) };
    let array = unsafe { find(corlib(), c"System".as_ptr(), c"Array".as_ptr()) };
    let mut engine_bases: Vec<_> = family
        .iter()
        .map(|&image| unsafe { find(image, c"UnityEngine".as_ptr(), c"Object".as_ptr()) })
        .filter(|class| !class.is_null())
        .collect();
    engine_bases.sort_unstable();
    engine_bases.dedup();
    if engine_bases.len() != 1 || intptr.is_null() || exception.is_null() {
        return Err("engine Object/IntPtr/Exception metadata identity absent or ambiguous".into());
    }
    let engine_base = engine_bases[0];
    let span = unsafe {
        find(
            image_of(engine_base),
            c"UnityEngine.Bindings".as_ptr(),
            c"ManagedSpanWrapper".as_ptr(),
        )
    };
    if array.is_null() || span.is_null() {
        return Err("engine Array/ManagedSpanWrapper metadata absent".into());
    }
    let mut visited = BTreeSet::new();
    let mut owners = Vec::new();
    for &(namespace, owner_name) in FAMILIES {
        for &image in &family {
            let owner = unsafe { find(image, namespace.as_ptr(), owner_name.as_ptr()) };
            if owner.is_null() || !visited.insert(owner as usize) {
                continue;
            }
            owners.push((
                owner,
                format!(
                    "{}.{}",
                    namespace.to_str().unwrap(),
                    owner_name.to_str().unwrap()
                ),
                unsafe { assignable(engine_base, owner) } != 0,
            ));
        }
    }
    // Discover engine object types by their actual inheritance, not a list of
    // Camera/Transform/Renderer method names. Non-engine assemblies stay out.
    for &image in &family {
        let name = unsafe { image_name(image) };
        if name.is_null() {
            return Err("owned engine image name absent".into());
        }
        if !unsafe { CStr::from_ptr(name) }
            .to_bytes()
            .starts_with(b"UnityEngine.")
        {
            continue;
        }
        let definitions = unsafe { table(image, 2) };
        if definitions.is_null() {
            return Err("engine TypeDef table absent".into());
        }
        let count = unsafe { rows(definitions) };
        if !(0..=100_000).contains(&count) {
            return Err("engine TypeDef table exceeds bound".into());
        }
        for index in 1..=count {
            let owner = unsafe { class_get(image, 0x02000000 | index as u32) };
            if owner.is_null()
                || visited.contains(&(owner as usize))
                || unsafe { assignable(engine_base, owner) } == 0
            {
                continue;
            }
            let text = unsafe { type_name(class_type(owner), 2) };
            if text.is_null() {
                return Err("engine object type name absent".into());
            }
            let name = unsafe { CStr::from_ptr(text) }
                .to_str()
                .map(|name| name.replace('+', "."))
                .map_err(|e| e.to_string());
            unsafe { free(text.cast()) };
            visited.insert(owner as usize);
            owners.push((owner, name?, true));
        }
    }
    let mut discovered = Vec::new();
    let mut deferred = 0;
    for (owner, owner_name, engine_object) in owners {
        let owner_image = unsafe { image_of(owner) };
        if !family.contains(&owner_image) {
            return Err("engine declaring image is not owned".into());
        }
        // Include every icall overload in the ambiguity check, even if
        // another overload has an unsupported signature.
        let mut groups: BTreeMap<String, Vec<(Object, u32)>> = BTreeMap::new();
        let mut iter = ptr::null_mut();
        loop {
            let method = unsafe { methods(owner, &mut iter) };
            if method.is_null() {
                break;
            }
            let mut implementation = 0;
            let method_flags = unsafe { flags(method, &mut implementation) };
            if implementation & 0x1000 == 0 {
                continue;
            }
            let name = unsafe { member(method) };
            if name.is_null() || unsafe { owner_of(method) } != owner {
                return Err("engine method declaration absent".into());
            }
            let name = unsafe { CStr::from_ptr(name) }
                .to_str()
                .map_err(|e| e.to_string())?;
            let binding = format!("{owner_name}::{name}");
            groups
                .entry(binding)
                .or_default()
                .push((method, method_flags));
        }
        for (name, candidates) in groups {
            if candidates.len() != 1 {
                deferred += candidates.len();
                continue;
            }
            let (method, method_flags) = candidates[0];
            if method_flags & 0x10 == 0 {
                deferred += 1;
                continue;
            }
            let Some(target) = registry.registered_target(&name)? else {
                deferred += 1;
                continue;
            };
            let sig = unsafe { signature(method) };
            if sig.is_null() {
                return Err("engine signature absent".into());
            }
            let arity = unsafe { count(sig) } as usize;
            if arity > ARGUMENT_CAPACITY {
                deferred += 1;
                continue;
            }
            let mut iterator = ptr::null_mut();
            let mut signature_types = Vec::new();
            let mut enum_storage = Vec::new();
            let mut arguments = Vec::new();
            let mut names = [ptr::null(); ARGUMENT_CAPACITY];
            unsafe { param_names(method, names.as_mut_ptr()) };
            let mut result = None;
            let mut engine_arguments = None;
            let mut utf16_arguments = None;
            let mut supported = true;
            for index in 0..=arity {
                let ty = unsafe {
                    if index == 0 {
                        returns(sig)
                    } else {
                        parameter(sig, &mut iterator)
                    }
                };
                if ty.is_null() {
                    return Err("engine signature type absent".into());
                }
                let type_kind = unsafe { kind(ty) };
                let class = unsafe { type_class(ty) };
                if class.is_null() || !family.contains(&unsafe { image_of(class) }) {
                    supported = false;
                    break;
                }
                let reference = unsafe { byref(ty) } != 0;
                signature_types.push((type_kind, class, reference));
                if index == 0 && type_kind == 0x18 && !reference {
                    let Some(class) =
                        api.engine_handle_result(method, owner, engine_base, &family)?
                    else {
                        supported = false;
                        break;
                    };
                    result = Some(Return::EngineHandle {
                        class: class as usize,
                    });
                } else if engine_object
                    && index == 1
                    && type_kind == 0x18
                    && !reference
                    && class == intptr
                    && !names[0].is_null()
                    && unsafe { CStr::from_ptr(names[0]) } == c"_unity_self"
                {
                    arguments.push(Argument::EngineSelf);
                } else if index != 0 && type_kind == 0x18 && class == intptr && !reference {
                    if engine_arguments.is_none() {
                        engine_arguments = Some(api.engine_pointer_arguments(
                            method,
                            owner,
                            engine_base,
                            &family,
                        )?);
                    }
                    let Some(nullable) = engine_arguments.as_ref().unwrap()[index - 1] else {
                        supported = false;
                        break;
                    };
                    arguments.push(Argument::EnginePointer { nullable });
                } else if index != 0
                    && !reference
                    && matches!(type_kind, 0x12 | 0x15)
                    && unsafe { assignable(engine_base, class) } != 0
                {
                    arguments.push(Argument::EngineObject {
                        class: class as usize,
                    });
                } else if !reference && type_kind == 0x1d && index != 0 {
                    let element = unsafe { element_class(class) };
                    if element.is_null()
                        || unsafe { rank(class) } != 1
                        || !family.contains(&unsafe { image_of(element) })
                    {
                        supported = false;
                        break;
                    }
                    if unsafe { assignable(engine_base, element) } != 0 {
                        arguments.push(Argument::EngineArray {
                            class: class as usize,
                        });
                    } else if element != intptr
                        && value_layout::Api::load(library, unsafe { image_of(element) })?
                            .layout_in(element, &family)
                            .and_then(|layout| layout.scalar_inline())
                            .is_ok()
                    {
                        arguments.push(Argument::DataArray {
                            class: class as usize,
                        });
                    } else {
                        supported = false;
                        break;
                    }
                } else if index != 0
                    && !reference
                    && type_kind == 0x12
                    && unsafe { assignable(exception, class) } != 0
                {
                    arguments.push(Argument::Exception {
                        class: class as usize,
                    });
                } else if index != 0 && !reference && type_kind == 0x12 && class == array {
                    arguments.push(Argument::DataArray {
                        class: class as usize,
                    });
                } else if index != 0 && reference && type_kind == 0x11 && class == span {
                    if utf16_arguments.is_none() {
                        utf16_arguments =
                            Some(api.engine_utf16_arguments(method, owner, span, &family)?);
                    }
                    if !utf16_arguments.as_ref().unwrap()[index - 1] {
                        supported = false;
                        break;
                    }
                    super::logging::validate_span_layout(library, span)?;
                    arguments.push(Argument::Utf16Span);
                } else if reference {
                    if index == 0 {
                        supported = false;
                        break;
                    }
                    let (width, alignment) =
                        match value_layout::Api::load(library, unsafe { image_of(class) })?
                            .layout_in(class, &family)
                            .and_then(|l| l.scalar_inline())
                        {
                            Ok(layout) => layout,
                            Err(_) => {
                                supported = false;
                                break;
                            }
                        };
                    if !(1..=256).contains(&width) || !matches!(alignment, 1 | 2 | 4 | 8) {
                        supported = false;
                        break;
                    }
                    // Read actual Param flags from this method's bounded
                    // ParamList. No precomputed token or direction table.
                    let method_token = unsafe { token(method) };
                    let method_table = unsafe { table(owner_image, 6) };
                    let param_table = unsafe { table(owner_image, 8) };
                    if method_table.is_null() || param_table.is_null() || method_token >> 24 != 6 {
                        return Err("engine parameter metadata table absent".into());
                    }
                    let method_row = (method_token & 0xffffff) as i32 - 1;
                    let method_rows = unsafe { rows(method_table) };
                    let param_rows = unsafe { rows(param_table) };
                    if method_row < 0
                        || method_row >= method_rows
                        || !(0..=1_000_000).contains(&param_rows)
                    {
                        return Err("engine parameter table range invalid".into());
                    }
                    let first = unsafe { row_col(method_table, method_row, 5) };
                    let end = if method_row + 1 < method_rows {
                        unsafe { row_col(method_table, method_row + 1, 5) }
                    } else {
                        param_rows as u32 + 1
                    };
                    if first == 0
                        || end < first
                        || end - first > (ARGUMENT_CAPACITY + 1) as u32
                        || end > param_rows as u32 + 1
                    {
                        return Err("engine ParamList range invalid".into());
                    }
                    let mut attrs = None;
                    for row in first..end {
                        if unsafe { row_col(param_table, row as i32 - 1, 1) } == index as u32 {
                            if attrs.is_some() {
                                return Err("duplicate engine parameter sequence".into());
                            }
                            attrs = Some(unsafe { row_col(param_table, row as i32 - 1, 0) });
                        }
                    }
                    let attrs = attrs.ok_or("engine byref direction absent")?;
                    arguments.push(Argument::Value(Payload {
                        width,
                        alignment,
                        output: attrs & 3 == 2,
                    }));
                } else {
                    let storage = if type_kind == 17 {
                        if unsafe { is_enum(class) } == 0 {
                            supported = false;
                            break;
                        }
                        let base = unsafe { enum_base(class) };
                        if base.is_null() {
                            return Err("engine enum storage absent".into());
                        }
                        let base_kind = unsafe { kind(base) };
                        enum_storage.push((class, base_kind));
                        base_kind
                    } else {
                        type_kind
                    };
                    let Some(scalar) = Scalar::from_kind(storage) else {
                        supported = false;
                        break;
                    };
                    if index == 0 {
                        result = Some(Return::Scalar(scalar));
                    } else if scalar.width == 0 {
                        supported = false;
                        break;
                    } else {
                        arguments.push(Argument::Scalar(scalar));
                    }
                }
            }
            if !supported {
                deferred += 1;
                continue;
            }
            api.fixed_typed_icall_enum_storage(
                method,
                owner,
                &signature_types,
                &family,
                &enum_storage,
            )?;
            // Existing semantic adapters keep their registration. This
            // lookup never executes a method or generates a wrapper.
            if !unsafe { lookup(method) }.is_null() {
                continue;
            }
            discovered.push(Binding {
                library: library as usize,
                name: CString::new(name).map_err(|e| e.to_string())?,
                target,
                arguments,
                returns: result.ok_or("engine scalar return absent")?,
            });
        }
    }
    if discovered.len() > POOL_CAPACITY {
        return Err("compiled engine entry pool exhausted before installation".into());
    }
    let entries = (0..discovered.len())
        .map(native_pool::entry)
        .collect::<Result<Vec<_>, _>>()?;
    BINDINGS
        .set(discovered)
        .map_err(|_| "metadata engine bridge already installed")?;
    let bindings = BINDINGS.get().unwrap();
    for (binding, entry) in bindings.iter().zip(entries) {
        unsafe { add(binding.name.as_ptr(), entry) };
    }
    eprintln!("NATIVE_MONO_SCALAR_ENGINE_INSTALLED: declarations={}; deferred={deferred}; metadata-discovered=true; runtime-registration-targets=true; fixed-entry-pool=true", bindings.len());
    Ok(())
}
fn execute(binding: &Binding, arguments: &[u64]) -> Result<(u64, u64), String> {
    if arguments.len() != binding.arguments.len() {
        return Err("engine argument arity changed".into());
    }
    let mut checked = arguments.to_vec();
    let mut object_pins = Vec::new();
    for (argument, value) in binding.arguments.iter().zip(&mut checked) {
        if matches!(
            argument,
            Argument::EngineSelf | Argument::EnginePointer { .. }
        ) {
            if *value != 0 || !matches!(argument, Argument::EnginePointer { nullable: true }) {
                *value = engine_objects::engine_pointer(*value as usize)?;
            }
        } else if let Argument::EngineArray { class }
        | Argument::EngineObject { class }
        | Argument::Exception { class }
        | Argument::DataArray { class } = argument
        {
            let native = managed_calls::Api::load(binding.library as Object)?.compatible(
                *value as Object,
                managed_calls::Shape::Reference {
                    class: *class as Object,
                    string: false,
                },
            )?;
            if native.is_null() {
                *value = 0;
                continue;
            }
            let (token, pin) = callbacks::with_metadata_service(|route, memory| {
                // Exceptions keep runtime stack/unmanaged state in Mono.
                // Engine formatting uses normal metadata/invoke/field hooks.
                let token = if matches!(argument, Argument::Exception { .. }) {
                    route.root_reference(native, memory)?
                } else if matches!(argument, Argument::DataArray { .. }) {
                    route.root_blittable_array(native, memory)?.0
                } else {
                    route.root_container(native, memory)?
                };
                let roots = route
                    .objects
                    .as_mut()
                    .ok_or("engine reference has no root service")?;
                if matches!(argument, Argument::EngineObject { .. }) {
                    roots
                        .roots
                        .publish_engine_object(roots.api, memory, token)?;
                }
                Ok((token, roots.api.worker_root(native)?))
            })?;
            *value = token;
            object_pins.push(pin);
        }
    }
    let arguments = checked.as_slice();
    let utf16: Vec<_> = binding
        .arguments
        .iter()
        .zip(arguments)
        .enumerate()
        .filter(|(_, (kind, _))| matches!(kind, Argument::Utf16Span))
        .map(|(index, (_, &address))| {
            unsafe { super::logging::owned_span(address as *const super::logging::Span) }
                .map(|units| (index, units))
        })
        .collect::<Result<_, _>>()?;
    let layouts: Vec<_> = binding
        .arguments
        .iter()
        .map(|argument| match argument {
            Argument::Value(layout) => Some(*layout),
            _ => None,
        })
        .collect();
    let floating: Vec<_> = binding
        .arguments
        .iter()
        .zip(arguments)
        .enumerate()
        .filter_map(|(index, (argument, &bits))| {
            (index < 4 && matches!(argument, Argument::Scalar(s) if s.floating()))
                .then_some((index, bits))
        })
        .collect();
    let (value, frame, outputs) = if layouts.iter().any(Option::is_some) {
        let frame = unsafe { ValueFrame::prepare(arguments, &layouts) }?;
        let spans: Vec<_> = frame
            .spans
            .iter()
            .map(|(position, bytes)| (*position, bytes.as_slice()))
            .collect();
        let strings: Vec<_> = utf16
            .iter()
            .map(|(position, units)| (*position, units.as_deref()))
            .collect();
        let (value, outputs) = if strings.is_empty() {
            callbacks::guest_value_arguments(
                binding.target,
                arguments,
                &floating,
                &spans,
                &frame.aliases,
            )?
        } else {
            callbacks::guest_value_span_arguments(
                binding.target,
                arguments,
                &floating,
                &spans,
                &frame.aliases,
                &strings,
            )?
        };
        (value, Some(frame), outputs)
    } else if !utf16.is_empty() {
        let strings: Vec<_> = utf16
            .iter()
            .map(|(position, units)| (*position, units.as_deref()))
            .collect();
        (
            callbacks::guest_span_arguments(binding.target, arguments, &floating, &strings, None)?
                .0,
            None,
            Vec::new(),
        )
    } else {
        (
            callbacks::guest_scalars(binding.target, arguments, &floating)?,
            None,
            Vec::new(),
        )
    };
    let word = match binding.returns {
        Return::Scalar(scalar) => {
            scalar.normalize(if scalar.floating() { value.1 } else { value.0 })?
        }
        Return::EngineHandle { class } => {
            engine_objects::engine_handle(binding.library as Object, class as Object, value.0)?
                as u64
        }
    };
    if !object_pins.is_empty() {
        // The original engine may bind m_CachedPtr or update array elements.
        // Synchronize only checked cells through the existing Mono barriers;
        // canonical identities remain rooted throughout the callback.
        callbacks::with_metadata_service(|route, memory| route.sync_guest(memory))?;
        for pin in &object_pins {
            pin.target()?;
        }
    }
    // Validate the result and every output before changing any native slot.
    if let Some(frame) = frame {
        unsafe { frame.commit(&outputs) }?;
    }
    Ok(if binding.returns.floating() {
        (0, word)
    } else {
        (word, 0)
    })
}
struct ValueFrame {
    spans: Vec<(usize, Vec<u8>)>,
    aliases: Vec<(usize, usize)>,
    destinations: Vec<(usize, Payload)>,
}
impl ValueFrame {
    /// The compiled adapter is called only by a verified native Mono ABI.
    /// Validate all ranges/aliases before reading any native value storage.
    unsafe fn prepare(arguments: &[u64], layouts: &[Option<Payload>]) -> Result<Self, String> {
        if arguments.len() != layouts.len() {
            return Err("value callback layout arity mismatch".into());
        }
        let mut spans: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut aliases = Vec::new();
        let mut destinations: Vec<(usize, Payload)> = Vec::new();
        for (index, (&address, &layout)) in arguments.iter().zip(layouts).enumerate() {
            let Some(layout) = layout else { continue };
            let address = address as usize;
            let end = address
                .checked_add(layout.width)
                .ok_or("native value span overflow")?;
            if address == 0
                || !(1..=256).contains(&layout.width)
                || !matches!(layout.alignment, 1 | 2 | 4 | 8)
                || !address.is_multiple_of(layout.alignment)
            {
                return Err("native value pointer/width/alignment mismatch".into());
            }
            let mut alias = None;
            for (group, (other, prior)) in destinations.iter_mut().enumerate() {
                if address < *other + prior.width && *other < end {
                    if address != *other
                        || layout.width != prior.width
                        || layout.alignment != prior.alignment
                    {
                        return Err("native value arguments overlap with different layouts".into());
                    }
                    // One input alias makes the shared storage an input/output.
                    prior.output &= layout.output;
                    alias = Some(group);
                    break;
                }
            }
            if let Some(group) = alias {
                aliases.push((index, spans[group].0));
            } else {
                spans.push((index, Vec::new()));
                destinations.push((address, layout));
            }
        }
        for ((_, bytes), &(address, layout)) in spans.iter_mut().zip(&destinations) {
            *bytes = if layout.output {
                vec![0; layout.width]
            } else {
                unsafe { std::slice::from_raw_parts(address as *const u8, layout.width) }.to_vec()
            };
        }
        Ok(Self {
            spans,
            aliases,
            destinations,
        })
    }
    unsafe fn commit(&self, outputs: &[Vec<u8>]) -> Result<(), String> {
        if outputs.len() != self.destinations.len()
            || outputs
                .iter()
                .zip(&self.destinations)
                .any(|(bytes, (_, layout))| bytes.len() != layout.width)
        {
            return Err("native value output length mismatch; all outputs unchanged".into());
        }
        for (bytes, &(address, layout)) in outputs.iter().zip(&self.destinations) {
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, layout.width) };
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::{
        memory::{Memory, EXECUTE, READ, WRITE},
        process::STACK,
        runtime::Runtime64,
    };
    fn scalar(kind: u32) -> Argument {
        Argument::Scalar(Scalar::from_kind(kind).unwrap())
    }
    fn value(width: usize, output: bool) -> Argument {
        Argument::Value(Payload {
            width,
            alignment: 4,
            output,
        })
    }
    fn binding(name: &str, target: u64, arguments: Vec<Argument>, result: u32) -> Binding {
        Binding {
            library: 0,
            name: CString::new(name).unwrap(),
            target,
            arguments,
            returns: Return::Scalar(Scalar::from_kind(result).unwrap()),
        }
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn same_compiled_pool_handles_different_profiles_signatures_targets_and_float_bits() {
        BINDINGS
            .set(vec![
                binding(
                    "FirstGame::Mixed",
                    0x1000,
                    vec![scalar(8), scalar(12), scalar(12), scalar(12)],
                    1,
                ),
                binding("SecondGame::Add", 0x1800, vec![scalar(12), scalar(12)], 12),
                binding("OtherVersion::Double", 0x1c00, vec![], 13),
                binding(
                    "SecondGame::Value",
                    0x1000,
                    vec![
                        value(16, false),
                        value(16, false),
                        scalar(12),
                        value(16, true),
                    ],
                    1,
                ),
                binding(
                    "OtherVersion::Seven",
                    0x1800,
                    vec![scalar(12); 6]
                        .into_iter()
                        .chain([value(64, true)])
                        .collect(),
                    1,
                ),
                binding(
                    "PackedStack",
                    0x1d00,
                    vec![scalar(8); 8]
                        .into_iter()
                        .chain([scalar(7), scalar(2), scalar(12), scalar(8)])
                        .collect(),
                    1,
                ),
            ])
            .unwrap_or_else(|_| panic!("fixture bridge already installed"));
        let mut memory = Memory::default();
        memory.map(0x1000, 8192, READ | WRITE | EXECUTE).unwrap();
        memory.map(STACK, 65536, READ | WRITE).unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            STACK,
            65536,
            "generic-engine-pool-test",
        )
        .unwrap();
        memory
            .write(
                0x1000,
                &[
                    0xb8, 0x00, 0x20, 0x00, 0x00, 0x89, 0x08, 0x66, 0x0f, 0x7e, 0x48, 0x04, 0x66,
                    0x0f, 0x7e, 0x50, 0x08, 0x66, 0x0f, 0x7e, 0x58, 0x0c, 0xc3,
                ],
            )
            .unwrap();
        let mixed = unsafe {
            std::mem::transmute::<Object, unsafe extern "C" fn(i32, f32, f32, f32)>(
                native_pool::entry(0).unwrap(),
            )
        };
        callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || unsafe {
            mixed(-7, -0.0, f32::from_bits(0x7fc01234), 3.25)
        })
        .unwrap();
        for (offset, expected) in [
            (0, -7i32 as u32),
            (4, (-0.0f32).to_bits()),
            (8, 0x7fc01234),
            (12, 3.25f32.to_bits()),
        ] {
            assert_eq!(
                memory.read_int(0x2000 + offset, 4).unwrap(),
                expected as u64
            );
        }
        // Another slot, another address and another signature, same capture
        // implementation. Exact f32/f64 bits survive both ABIs.
        memory
            .write(0x1800, &[0xf3, 0x0f, 0x58, 0xc1, 0xc3])
            .unwrap();
        let float = unsafe {
            std::mem::transmute::<Object, unsafe extern "C" fn(f32, f32) -> f32>(
                native_pool::entry(1).unwrap(),
            )
        };
        let (result, _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || unsafe {
            float(-1.5, 3.25)
        })
        .unwrap();
        assert_eq!(result, 1.75);
        let double = unsafe {
            std::mem::transmute::<Object, unsafe extern "C" fn() -> f64>(
                native_pool::entry(2).unwrap(),
            )
        };
        for bits in [0x7ff800000000beefu64, 0x8000000000000000] {
            let mut code = vec![0x48, 0xb8];
            code.extend_from_slice(&bits.to_le_bytes());
            code.extend_from_slice(&[0x66, 0x48, 0x0f, 0x6e, 0xc0, 0xc3]);
            memory.write(0x1c00, &code).unwrap();
            let (result, _) =
                callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || unsafe { double() })
                    .unwrap();
            assert_eq!(result.to_bits(), bits);
        }
        memory
            .write(
                0x1000,
                &[
                    0xf3, 0x0f, 0x10, 0x01, 0xf3, 0x0f, 0x58, 0xc2, 0xf3, 0x41, 0x0f, 0x11, 0x01,
                    0xc3,
                ],
            )
            .unwrap();
        let operation = unsafe {
            std::mem::transmute::<Object, unsafe extern "C" fn(*mut u8, *mut u8, f32, *mut u8)>(
                native_pool::entry(3).unwrap(),
            )
        };
        let mut first = [1.5f32, 0.0, 0.0, 1.0];
        let mut second = [2.0f32, 0.0, 0.0, 1.0];
        let mut output = [9.0f32; 4];
        callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || unsafe {
            operation(
                first.as_mut_ptr().cast(),
                second.as_mut_ptr().cast(),
                3.25,
                output.as_mut_ptr().cast(),
            )
        })
        .unwrap();
        assert_eq!(output, [4.75, 0.0, 0.0, 0.0]);
        memory
            .write(
                0x1800,
                &[
                    0xf3, 0x0f, 0x58, 0xc1, 0xf3, 0x0f, 0x58, 0xc2, 0xf3, 0x0f, 0x58, 0xc3, 0xf3,
                    0x0f, 0x58, 0x44, 0x24, 0x28, 0xf3, 0x0f, 0x58, 0x44, 0x24, 0x30, 0x48, 0x8b,
                    0x44, 0x24, 0x38, 0xf3, 0x0f, 0x11, 0x00, 0xc3,
                ],
            )
            .unwrap();
        let seven = unsafe {
            std::mem::transmute::<Object, unsafe extern "C" fn(f32, f32, f32, f32, f32, f32, *mut u8)>(
                native_pool::entry(4).unwrap(),
            )
        };
        let mut matrix = [9.0f32; 16];
        callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || unsafe {
            seven(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, matrix.as_mut_ptr().cast())
        })
        .unwrap();
        assert_eq!(matrix[0], 21.0);
        assert!(matrix[1..].iter().all(|&value| value == 0.0));
        let mut code = vec![0x49, 0xba]; // mov r10, output
        code.extend_from_slice(&0x2100u64.to_le_bytes());
        for index in 0..4 {
            code.extend([0x8b, 0x44, 0x24, 0x48 + index * 8]); // arg 8..11
            code.extend([0x41, 0x89, 0x42, index * 4]);
        }
        code.push(0xc3);
        memory.write(0x1d00, &code).unwrap();
        let packed = unsafe {
            std::mem::transmute::<
                Object,
                unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32, i32, u16, u8, f32, i32),
            >(native_pool::entry(5).unwrap())
        };
        callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || unsafe {
            packed(
                1,
                2,
                3,
                4,
                5,
                6,
                7,
                8,
                0x1234,
                1,
                f32::from_bits(0x7fc01234),
                -7,
            )
        })
        .unwrap();
        for (index, expected) in [0x1234, 1, 0x7fc01234, (-7i32) as u32]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                memory.read_int(0x2100 + index as u64 * 4, 4).unwrap(),
                expected as u64
            );
        }
        assert!(native_pool::entry(POOL_CAPACITY).is_err());
        assert_eq!(std::mem::size_of::<Capture>(), 136);
        assert_eq!(std::mem::size_of::<Registers>(), 16);
        assert_eq!(runtime.active_thread(), 1);
    }
    #[test]
    fn capability_storage_is_not_inferred_and_failed_results_leave_value_outputs_unchanged() {
        for kind in [4, 5, 6, 10, 11, 15, 17, 18, 24, 25, 27] {
            assert!(Scalar::from_kind(kind).is_none());
        }
        let boolean = binding("Boolean", 0x1000, vec![scalar(2)], 2);
        let mut capture = Capture::default();
        capture.integer[0] = 2;
        assert!(unsafe { decode(&boolean, &capture) }.is_err());
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.map(STACK, 65536, READ | WRITE).unwrap();
        let mut runtime =
            Runtime64::new(&mut memory, 0x140000000, STACK, 65536, "value-result-test").unwrap();
        memory
            .write(0x1000, &[0xc7, 0x01, 0, 0, 0, 0, 0xb8, 2, 0, 0, 0, 0xc3])
            .unwrap();
        let callback = binding("ValueAndBoolean", 0x1000, vec![value(12, true)], 2);
        let mut output = [9.0f32; 3];
        let (result, _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            execute(&callback, &[output.as_mut_ptr() as u64])
        })
        .unwrap();
        assert!(result.is_err());
        assert_eq!(output, [9.0; 3]);
        let receiver = binding(
            "EngineReceiver",
            0x1000,
            vec![Argument::EngineSelf, value(12, true)],
            1,
        );
        let (result, _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            execute(
                &receiver,
                &[output.as_ptr() as u64, output.as_mut_ptr() as u64],
            )
        })
        .unwrap();
        assert!(result.is_err());
        assert_eq!(output, [9.0; 3]);
        memory
            .write(0x1000, &[0xc7, 0x02, 0, 0, 0x80, 0x3f, 0xc3])
            .unwrap();
        let pointer = binding(
            "NullableEngineArgument",
            0x1000,
            vec![Argument::EnginePointer { nullable: true }, value(12, true)],
            1,
        );
        let (result, _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            execute(&pointer, &[0, output.as_mut_ptr() as u64])
        })
        .unwrap();
        result.unwrap();
        assert_eq!(output, [1.0, 0.0, 0.0]);
        output = [9.0; 3];
        let (result, _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            execute(
                &pointer,
                &[output.as_ptr() as u64, output.as_mut_ptr() as u64],
            )
        })
        .unwrap();
        assert!(result.is_err());
        assert_eq!(output, [9.0; 3]);
        let required = binding(
            "RequiredEngineArgument",
            0x1000,
            vec![Argument::EnginePointer { nullable: false }, value(12, true)],
            1,
        );
        let (result, _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            execute(&required, &[0, output.as_mut_ptr() as u64])
        })
        .unwrap();
        assert!(result.is_err());
        assert_eq!(output, [9.0; 3]);
    }
    #[test]
    fn direct_reference_lanes_preserve_float_and_integer_argument_order() {
        let descriptor = binding(
            "ReferenceProtocol",
            0x1000,
            vec![
                Argument::EngineObject { class: 1 },
                scalar(12),
                Argument::Exception { class: 2 },
                scalar(8),
                Argument::EngineArray { class: 3 },
            ],
            1,
        );
        let capture = Capture {
            integer: [0x1100, 0x2200, (-7i32) as u32 as u64, 0x3300, 0, 0, 0, 0],
            floating: [(-0.0f32).to_bits() as u64, 0, 0, 0, 0, 0, 0, 0],
            stack: 0,
        };
        assert_eq!(
            unsafe { decode(&descriptor, &capture) }.unwrap(),
            [
                0x1100,
                (-0.0f32).to_bits() as u64,
                0x2200,
                (-7i32) as u32 as u64,
                0x3300
            ]
        );
    }
    #[test]
    fn value_aliases_remain_shared_and_partial_overlap_rejects_before_dereference() {
        let mut cell = [1.0f32, 2.0, 3.0, 4.0];
        let pointer = cell.as_mut_ptr() as u64;
        let input = Payload {
            width: 12,
            alignment: 4,
            output: false,
        };
        let output = Payload {
            output: true,
            ..input
        };
        let frame =
            unsafe { ValueFrame::prepare(&[pointer, pointer], &[Some(output), Some(input)]) }
                .unwrap();
        assert_eq!(frame.spans.len(), 1);
        assert_eq!(frame.aliases, [(1, 0)]);
        assert_eq!(frame.spans[0].1, unsafe {
            std::slice::from_raw_parts(pointer as *const u8, 12)
        });
        let before = cell;
        assert!(unsafe { frame.commit(&[vec![0; 8]]) }.is_err());
        assert_eq!(cell, before);
        assert!(unsafe { ValueFrame::prepare(&[4, 8], &[Some(input), Some(input)]) }.is_err());
        assert!(unsafe { ValueFrame::prepare(&[0], &[Some(input)]) }.is_err());
        assert!(unsafe { ValueFrame::prepare(&[pointer + 1], &[Some(input)]) }.is_err());
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        memory.map(STACK, 65536, READ | WRITE).unwrap();
        let mut runtime =
            Runtime64::new(&mut memory, 0x140000000, STACK, 65536, "value-alias-test").unwrap();
        memory
            .write(
                0x1000,
                &[
                    0x48, 0x39, 0xd1, 0x75, 0x07, 0xc7, 0x01, 0x00, 0x00, 0x80, 0x40, 0xc3,
                ],
            )
            .unwrap();
        let spans: Vec<_> = frame
            .spans
            .iter()
            .map(|(position, bytes)| (*position, bytes.as_slice()))
            .collect();
        let (result, _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            callbacks::guest_value_arguments(
                0x1000,
                &[pointer, pointer],
                &[],
                &spans,
                &frame.aliases,
            )
        })
        .unwrap();
        let (_, bytes) = result.unwrap();
        unsafe { frame.commit(&bytes) }.unwrap();
        assert_eq!(cell, [4.0, 2.0, 3.0, 4.0]);
        assert!(
            callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
                callbacks::guest_value_arguments(0x1000, &[0, 0], &[], &[], &[(0, 1)])
            })
            .unwrap()
            .0
            .is_err()
        );
    }
}
