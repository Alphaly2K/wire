//! Pure scalar format/capability calls retain the original engine's answers.
//! Every primitive/enum signature and registration target is checked first.
use super::{bindings, callbacks, managed_calls, Object};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    ffi::{CStr, CString},
    sync::OnceLock,
};
#[derive(Clone, Copy, PartialEq)]
enum Type {
    Boolean,
    Integer,
    Unsigned,
    Enum(&'static CStr, &'static CStr),
}
const FORMAT: Type = Type::Enum(c"UnityEngine.Experimental.Rendering", c"GraphicsFormat");
const TEXTURE: Type = Type::Enum(c"UnityEngine", c"TextureFormat");
const RENDER: Type = Type::Enum(c"UnityEngine", c"RenderTextureFormat");
const USAGE: Type = Type::Enum(
    c"UnityEngine.Experimental.Rendering",
    c"GraphicsFormatUsage",
);
struct Spec {
    namespace: &'static CStr,
    owner: &'static CStr,
    member: &'static CStr,
    target: u64,
    result: Type,
    arguments: &'static [Type],
}
static TARGETS: OnceLock<Vec<u64>> = OnceLock::new();
thread_local! { static CALLS: Cell<usize> = const { Cell::new(0) }; static SEEN: RefCell<BTreeSet<usize>> = const { RefCell::new(BTreeSet::new()) }; }
macro_rules! adapter {
    ($name:ident, one, $result:ty) => {
        unsafe extern "C" fn $name(value: i32) -> $result {
            call(Slot::$name as usize, &[value as u32 as u64]) as $result
        }
    };
    ($name:ident, flag, $result:ty) => {
        unsafe extern "C" fn $name(value: i32, flag: u8) -> $result {
            call(Slot::$name as usize, &[value as u32 as u64, flag as u64]) as $result
        }
    };
    ($name:ident, two, $result:ty) => {
        unsafe extern "C" fn $name(first: i32, second: i32) -> $result {
            call(
                Slot::$name as usize,
                &[first as u32 as u64, second as u32 as u64],
            ) as $result
        }
    };
}
macro_rules! formats {
    ($( $name:ident, $ns:expr, $owner:expr, $member:expr, $target:expr, $result:expr, [$($arg:expr),*], $shape:ident, $rust:ty; )*) => {
        #[allow(non_camel_case_types)] enum Slot { $( $name, )* }
        const SPECS: &[Spec] = &[$(Spec { namespace: $ns, owner: $owner, member: $member, target: $target, result: $result, arguments: &[$($arg),*] },)*];
        $(adapter!($name, $shape, $rust);)*
        fn adapters() -> Vec<Object> { vec![$($name as *const () as Object,)*] }
    };
}
formats! {
    texture_format, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetGraphicsFormat_Native_TextureFormat", 0x1800f8950, FORMAT, [TEXTURE, Type::Boolean], flag, i32;
    from_graphics, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetTextureFormat_Native_GraphicsFormat", 0x1800f89a0, TEXTURE, [FORMAT], one, i32;
    render_format, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetGraphicsFormat_Native_RenderTextureFormat", 0x1800f89d0, FORMAT, [RENDER, Type::Boolean], flag, i32;
    depth_format, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetDepthStencilFormatFromBitsLegacy_Native", 0x1800f8a00, FORMAT, [Type::Integer], one, i32;
    depth_bits, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetDepthBits", 0x1800f8a10, Type::Integer, [FORMAT], one, i32;
    srgb, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsSRGBFormat", 0x1800f8a20, Type::Boolean, [FORMAT], one, u8;
    swizzle, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsSwizzleFormat", 0x1800f8a50, Type::Boolean, [FORMAT], one, u8;
    to_srgb, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetSRGBFormat", 0x1800f8ab0, FORMAT, [FORMAT], one, i32;
    to_linear, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetLinearFormat", 0x1800f8ad0, FORMAT, [FORMAT], one, i32;
    to_render, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetRenderTextureFormat", 0x1800f8af0, RENDER, [FORMAT], one, i32;
    color_count, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetColorComponentCount", 0x1800f8b10, Type::Unsigned, [FORMAT], one, u32;
    alpha_count, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetAlphaComponentCount", 0x1800f8b30, Type::Unsigned, [FORMAT], one, u32;
    component_count, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetComponentCount", 0x1800f8b50, Type::Unsigned, [FORMAT], one, u32;
    compressed, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsCompressedFormat", 0x1800f9070, Type::Boolean, [FORMAT], one, u8;
    compressed_texture, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsCompressedFormat_Native_TextureFormat", 0x1800f90a0, Type::Boolean, [TEXTURE], one, u8;
    decompress, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"CanDecompressFormat", 0x1800f90f0, Type::Boolean, [FORMAT, Type::Boolean], flag, u8;
    packed, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsPackedFormat", 0x1800f9110, Type::Boolean, [FORMAT], one, u8;
    packed16, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"Is16BitPackedFormat", 0x1800f9140, Type::Boolean, [FORMAT], one, u8;
    alpha_format, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"ConvertToAlphaFormat", 0x1800f91a0, FORMAT, [FORMAT], one, i32;
    alpha_texture, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"ConvertToAlphaFormat_Native_TextureFormat", 0x1800f91c0, TEXTURE, [TEXTURE], one, i32;
    alpha_only, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsAlphaOnlyFormat", 0x1800f92d0, Type::Boolean, [FORMAT], one, u8;
    alpha_only_texture, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsAlphaOnlyFormat_Native_TextureFormat", 0x1800f9320, Type::Boolean, [TEXTURE], one, u8;
    alpha_test, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsAlphaTestFormat", 0x1800f9330, Type::Boolean, [FORMAT], one, u8;
    alpha_channel, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"HasAlphaChannel", 0x1800f9360, Type::Boolean, [FORMAT], one, u8;
    alpha_channel_texture, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"HasAlphaChannel_Native_TextureFormat", 0x1800f9390, Type::Boolean, [TEXTURE], one, u8;
    depth, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsDepthFormat", 0x1800f93c0, Type::Boolean, [FORMAT], one, u8;
    stencil, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsStencilFormat", 0x1800f93f0, Type::Boolean, [FORMAT], one, u8;
    depth_stencil, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsDepthStencilFormat", 0x1800f9420, Type::Boolean, [FORMAT], one, u8;
    ieee, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsIEEE754Format", 0x1800f9470, Type::Boolean, [FORMAT], one, u8;
    float, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsFloatFormat", 0x1800f94a0, Type::Boolean, [FORMAT], one, u8;
    half, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsHalfFormat", 0x1800f94b0, Type::Boolean, [FORMAT], one, u8;
    unsigned, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsUnsignedFormat", 0x1800f94c0, Type::Boolean, [FORMAT], one, u8;
    signed, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsSignedFormat", 0x1800f94f0, Type::Boolean, [FORMAT], one, u8;
    norm, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsNormFormat", 0x1800f9520, Type::Boolean, [FORMAT], one, u8;
    unorm, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsUNormFormat", 0x1800f9550, Type::Boolean, [FORMAT], one, u8;
    snorm, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsSNormFormat", 0x1800f95a0, Type::Boolean, [FORMAT], one, u8;
    integer, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsIntegerFormat", 0x1800f95f0, Type::Boolean, [FORMAT], one, u8;
    uint, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsUIntFormat", 0x1800f9620, Type::Boolean, [FORMAT], one, u8;
    sint, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsSIntFormat", 0x1800f9670, Type::Boolean, [FORMAT], one, u8;
    xr, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsXRFormat", 0x1800f96c0, Type::Boolean, [FORMAT], one, u8;
    dxtc, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsDXTCFormat", 0x1800f96f0, Type::Boolean, [FORMAT], one, u8;
    rgtc, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsRGTCFormat", 0x1800f9700, Type::Boolean, [FORMAT], one, u8;
    bptc, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsBPTCFormat", 0x1800f9710, Type::Boolean, [FORMAT], one, u8;
    bc, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsBCFormat", 0x1800f90f0, Type::Boolean, [FORMAT], one, u8;
    pvrtc, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsPVRTCFormat", 0x1800f9720, Type::Boolean, [FORMAT], one, u8;
    etc, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsETCFormat", 0x1800f9730, Type::Boolean, [FORMAT], one, u8;
    eac, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsEACFormat", 0x1800f9740, Type::Boolean, [FORMAT], one, u8;
    astc, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsASTCFormat", 0x1800f9750, Type::Boolean, [FORMAT], one, u8;
    hdr, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsHDRFormat", 0x1800f9770, Type::Boolean, [FORMAT], one, u8;
    hdr_texture, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsHDRFormat_Native_TextureFormat", 0x1800f97c0, Type::Boolean, [TEXTURE], one, u8;
    crunch, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"IsCrunchFormat", 0x1800f97f0, Type::Boolean, [TEXTURE], one, u8;
    swizzle_r, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetSwizzleR", 0x1800f9810, Type::Enum(c"UnityEngine.Rendering", c"FormatSwizzle"), [FORMAT], one, i32;
    swizzle_g, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetSwizzleG", 0x1800f9830, Type::Enum(c"UnityEngine.Rendering", c"FormatSwizzle"), [FORMAT], one, i32;
    swizzle_b, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetSwizzleB", 0x1800f9850, Type::Enum(c"UnityEngine.Rendering", c"FormatSwizzle"), [FORMAT], one, i32;
    swizzle_a, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetSwizzleA", 0x1800f9870, Type::Enum(c"UnityEngine.Rendering", c"FormatSwizzle"), [FORMAT], one, i32;
    block_size, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetBlockSize", 0x1800f9890, Type::Unsigned, [FORMAT], one, u32;
    block_width, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetBlockWidth", 0x1800f98b0, Type::Unsigned, [FORMAT], one, u32;
    block_height, c"UnityEngine.Experimental.Rendering", c"GraphicsFormatUtility", c"GetBlockHeight", 0x1800f98d0, Type::Unsigned, [FORMAT], one, u32;
    supported, c"UnityEngine", c"SystemInfo", c"IsFormatSupported", 0x1800cac60, Type::Boolean, [FORMAT, USAGE], two, u8;
    compatible, c"UnityEngine", c"SystemInfo", c"GetCompatibleFormat", 0x1800cac70, FORMAT, [FORMAT, USAGE], two, i32;
}
pub(super) fn installed() -> bool {
    TARGETS.get().is_some()
}
pub(super) fn report() -> String {
    format!("native-graphics-format-calls={}\n", CALLS.with(Cell::get))
}
pub(super) fn install(
    library: Object,
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing graphics format API: ", $name).into());
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
    let method_name = sym!(
        "mono_method_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let class = |namespace: &CStr, name: &CStr| -> Result<Object, String> {
        let mut classes: Vec<_> = images
            .iter()
            .map(|&image| unsafe { find(image, namespace.as_ptr(), name.as_ptr()) })
            .filter(|p| !p.is_null())
            .collect();
        classes.sort_unstable_by_key(|p| *p as usize);
        classes.dedup();
        if classes.len() != 1 {
            return Err(format!(
                "graphics format class not unique: {namespace:?}.{name:?}"
            ));
        }
        Ok(classes[0])
    };
    let scalar = |ty| match ty {
        Type::Boolean => class(c"System", c"Boolean"),
        Type::Integer => class(c"System", c"Int32"),
        Type::Unsigned => class(c"System", c"UInt32"),
        Type::Enum(ns, name) => class(ns, name),
    };
    let api = managed_calls::Api::load(library)?;
    let mut targets = Vec::new();
    let mut names = Vec::new();
    for spec in SPECS {
        let owner = class(spec.namespace, spec.owner)?;
        let arguments = spec
            .arguments
            .iter()
            .map(|&ty| scalar(ty))
            .collect::<Result<Vec<_>, _>>()?;
        let name = CString::new(format!(
            "{}.{}::{}",
            spec.namespace.to_str().unwrap(),
            spec.owner.to_str().unwrap(),
            spec.member.to_str().unwrap()
        ))
        .unwrap();
        // SystemInfo retains public legacy overloads with the same name and
        // arity. Select the unique declared InternalCall by its entire ABI.
        let result = scalar(spec.result)?;
        let mut iterator = std::ptr::null_mut();
        let mut selected = None;
        let mut finished = false;
        for _ in 0..4096 {
            let candidate = unsafe { methods(owner, &mut iterator) };
            if candidate.is_null() {
                finished = true;
                break;
            }
            let member = unsafe { method_name(candidate) };
            if member.is_null() || unsafe { CStr::from_ptr(member) } != spec.member {
                continue;
            }
            if api
                .scalar_icall(candidate, owner, result, &arguments, images)
                .is_ok()
                && selected.replace(candidate).is_some()
            {
                return Err(format!("{name:?}: ambiguous matching scalar InternalCalls"));
            }
        }
        if !finished || selected.is_none() {
            return Err(format!(
                "{name:?}: no unique metadata-qualified scalar InternalCall"
            ));
        }
        targets.push(registry.exact_target(name.to_str().unwrap(), spec.target)?);
        names.push(name);
    }
    TARGETS
        .set(targets)
        .map_err(|_| "graphics format adapters already installed")?;
    for (name, adapter) in names.iter().zip(adapters()) {
        unsafe { add(name.as_ptr(), adapter) };
    }
    Ok(())
}
fn call(index: usize, arguments: &[u64]) -> u64 {
    let result = std::panic::catch_unwind(|| -> Result<u64, String> {
        let spec = &SPECS[index];
        if arguments.len() != spec.arguments.len()
            || spec
                .arguments
                .iter()
                .zip(arguments)
                .any(|(&ty, &value)| ty == Type::Boolean && value > 1)
        {
            return Err("graphics format Boolean/arity mismatch".into());
        }
        let target = TARGETS.get().ok_or("graphics format adapters absent")?[index];
        let value = callbacks::guest_arguments(target, arguments)?;
        if spec.result == Type::Boolean && value & 255 > 1 {
            return Err("graphics format result is not canonical Boolean".into());
        }
        CALLS.with(|c| c.set(c.get() + 1));
        if SEEN.with(|seen| seen.borrow_mut().insert(index)) {
            eprintln!(
                "NATIVE_MONO_GRAPHICS_FORMAT_VERIFIED: member={:?}; original-engine=true",
                spec.member
            );
        }
        Ok(value)
    });
    match result {
        Ok(Ok(value)) => value,
        other => {
            eprintln!("NATIVE_MONO_GRAPHICS_FORMAT_REJECT: {other:?}");
            std::process::abort()
        }
    }
}
