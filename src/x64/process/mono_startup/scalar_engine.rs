//! Metadata-discovered scalar/value engine bridge. The signed binary contains
//! one fixed ARM64 register-capture entry pool, independent of method names,
//! MethodDef tokens, assembly hashes, signature combinations and guest RVAs.
//! Pointer/callback/lifetime protocols remain outside this ordinary value ABI.
use super::{bindings, callbacks, engine_objects, images, managed_calls, value_layout, Object};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString},
    ptr,
    sync::OnceLock,
};
const POOL_CAPACITY: usize = 4096;
// A semantic policy, not per-method ABI specifications. These families operate
// on ordinary numeric state; capability-like Int64/IntPtr APIs are not inferred.
const FAMILIES: &[(&CStr, &CStr, bool)] = &[
    (c"UnityEngine", c"Application", false),
    (c"UnityEngine", c"SystemInfo", false),
    (c"UnityEngine", c"QualitySettings", false),
    (c"UnityEngine", c"Time", false),
    (c"UnityEngine", c"Screen", false),
    (c"UnityEngine", c"Cursor", false),
    (c"UnityEngine", c"AudioSettings", false),
    (c"UnityEngine", c"AudioListener", false),
    (c"UnityEngine", c"Input", false),
    (c"UnityEngine", c"Mathf", true),
    (c"UnityEngine", c"GL", false),
    (c"UnityEngine", c"Graphics", false),
    (c"UnityEngine", c"Canvas", false),
    (c"UnityEngine", c"UISystemProfilerApi", false),
    (c"UnityEngine.Rendering", c"GraphicsSettings", false),
    (c"UnityEngine.Rendering", c"SplashScreen", false),
    (
        c"UnityEngine.Experimental.Rendering",
        c"GraphicsFormatUtility",
        false,
    ),
    (c"UnityEngine", c"Bounds", true),
    (c"UnityEngine", c"Matrix4x4", true),
    (c"UnityEngine", c"Vector3", true),
    (c"UnityEngine", c"Quaternion", true),
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
}
struct Binding {
    name: CString,
    target: u64,
    arguments: Vec<Argument>,
    returns: Scalar,
}
static BINDINGS: OnceLock<Vec<Binding>> = OnceLock::new();
thread_local! { static SEEN: RefCell<BTreeSet<usize>> = const { RefCell::new(BTreeSet::new()) }; }
#[repr(C)]
#[derive(Default)]
struct Capture {
    integer: [u64; 8],
    floating: [u64; 8],
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
        sub sp, sp, #160
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
        mov x0, x16
        add x1, sp, #16
        bl _wire_mono_engine_pool_dispatch
        fmov d0, x1
        ldp x29, x30, [sp]
        add sp, sp, #160
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
        let arguments = decode(binding, unsafe { &*capture })?;
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
            eprintln!("NATIVE_MONO_SCALAR_ENGINE_REJECT: slot={index}; {other:?}");
            std::process::abort();
        }
    }
}
fn decode(binding: &Binding, capture: &Capture) -> Result<Vec<u64>, String> {
    if binding.arguments.len() > 8 {
        return Err("engine signature exceeds compiled register capture".into());
    }
    let (mut integer, mut floating) = (0, 0);
    binding
        .arguments
        .iter()
        .map(|argument| match argument {
            Argument::Scalar(scalar) if scalar.floating() => {
                let value = scalar.normalize(capture.floating[floating]);
                floating += 1;
                value
            }
            Argument::Scalar(scalar) => {
                let value = scalar.normalize(capture.integer[integer]);
                integer += 1;
                value
            }
            Argument::Value(_) | Argument::EngineSelf => {
                let value = capture.integer[integer];
                integer += 1;
                Ok(value)
            }
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
    let mut engine_bases: Vec<_> = family
        .iter()
        .map(|&image| unsafe { find(image, c"UnityEngine".as_ptr(), c"Object".as_ptr()) })
        .filter(|class| !class.is_null())
        .collect();
    engine_bases.sort_unstable();
    engine_bases.dedup();
    if engine_bases.len() != 1 || intptr.is_null() {
        return Err("engine Object/IntPtr metadata identity is not unique".into());
    }
    let engine_base = engine_bases[0];
    let mut visited = BTreeSet::new();
    let mut owners = Vec::new();
    for &(namespace, owner_name, values) in FAMILIES {
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
                values,
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
            owners.push((owner, name?, true, true));
        }
    }
    let mut discovered = Vec::new();
    let mut deferred = 0;
    for (owner, owner_name, values, engine_object) in owners {
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
            if arity > 8 {
                deferred += 1;
                continue;
            }
            let mut iterator = ptr::null_mut();
            let mut signature_types = Vec::new();
            let mut enum_storage = Vec::new();
            let mut arguments = Vec::new();
            let mut names = [ptr::null(); 8];
            unsafe { param_names(method, names.as_mut_ptr()) };
            let mut result = None;
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
                if engine_object
                    && index == 1
                    && type_kind == 0x18
                    && !reference
                    && class == intptr
                    && !names[0].is_null()
                    && unsafe { CStr::from_ptr(names[0]) } == c"_unity_self"
                {
                    arguments.push(Argument::EngineSelf);
                } else if reference {
                    if !values || index == 0 {
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
                    if first == 0 || end < first || end - first > 9 || end > param_rows as u32 + 1 {
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
                        result = Some(scalar);
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
    for (argument, value) in binding.arguments.iter().zip(&mut checked) {
        if matches!(argument, Argument::EngineSelf) {
            *value = engine_objects::engine_pointer(*value as usize)?;
        }
    }
    let arguments = checked.as_slice();
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
        let (value, outputs) = callbacks::guest_value_arguments(
            binding.target,
            arguments,
            &floating,
            &spans,
            &frame.aliases,
        )?;
        (value, Some(frame), outputs)
    } else {
        (
            callbacks::guest_scalars(binding.target, arguments, &floating)?,
            None,
            Vec::new(),
        )
    };
    let word = binding.returns.normalize(if binding.returns.floating() {
        value.1
    } else {
        value.0
    })?;
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
            name: CString::new(name).unwrap(),
            target,
            arguments,
            returns: Scalar::from_kind(result).unwrap(),
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
        assert!(native_pool::entry(POOL_CAPACITY).is_err());
        assert_eq!(std::mem::size_of::<Capture>(), 128);
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
        assert!(decode(&boolean, &capture).is_err());
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
