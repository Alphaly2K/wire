//! Metadata-qualified ordinary IL calls with owned scalar arguments and String
//! byref outputs. Guest pointers never enter mono_runtime_invoke's parameter ABI.
use super::{c_char, closed_types, Memory, Object};
use crate::x64::memory::{READ, WRITE};
use std::{ffi::CStr, ptr};
mod engine_headers;
mod engine_spans;
mod scalar_outputs;
pub(super) use engine_headers::HeaderRead;
pub(super) use engine_spans::NumericSpanLayout;
pub(super) use scalar_outputs::ScalarOutputDescriptor;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Scalar {
        class: Object,
        width: usize,
        alignment: usize,
        boolean: bool,
    },
    Reference {
        class: Object,
        string: bool,
    },
}
impl Shape {
    pub(super) fn width(self) -> usize {
        match self {
            Self::Scalar { width, .. } => width,
            Self::Reference { .. } => 8,
        }
    }
    fn alignment(self) -> usize {
        match self {
            Self::Scalar { alignment, .. } => alignment,
            Self::Reference { .. } => 8,
        }
    }
    pub(super) fn validate(self, word: u64) -> Result<(), String> {
        if matches!(self, Self::Scalar { boolean: true, .. }) && word & 255 > 1 {
            return Err("managed Boolean is not canonical".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub(super) struct Parameter {
    pub(super) shape: Shape,
    pub(super) byref: bool,
}
pub(super) struct Descriptor {
    pub(super) receiver: Option<Object>,
    pub(super) parameters: Vec<Parameter>,
    pub(super) returns: Option<Shape>,
    pub(super) name: String,
}
pub(super) struct CoroutineDescriptor {
    pub(super) call: Descriptor,
    pub(super) boolean: Object,
}
#[derive(Clone, Copy)]
pub(super) enum BufferOperation {
    Copy,
    Compare,
    Replicate,
    Stride,
}
enum EntryAbi {
    StaticInternal,
    InstanceEngineValue,
    StaticManaged,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ExceptionClause {
    flags: u32,
    try_offset: u32,
    try_len: u32,
    handler_offset: u32,
    handler_len: u32,
    data: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PointerSource {
    Engine { nullable: bool },
    OwnedResource { class: usize, nullable: bool },
}
impl PointerSource {
    fn nullable(self) -> Self {
        match self {
            Self::Engine { .. } => Self::Engine { nullable: true },
            Self::OwnedResource { class, .. } => Self::OwnedResource {
                class,
                nullable: true,
            },
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Api {
    library: Object,
    class: unsafe extern "C" fn(Object) -> Object,
    image: unsafe extern "C" fn(Object) -> Object,
    name: unsafe extern "C" fn(Object) -> *const c_char,
    flags: unsafe extern "C" fn(Object, *mut u32) -> u32,
    signature: unsafe extern "C" fn(Object) -> Object,
    instance: unsafe extern "C" fn(Object) -> u32,
    count: unsafe extern "C" fn(Object) -> u32,
    parameters: unsafe extern "C" fn(Object, *mut Object) -> Object,
    returns: unsafe extern "C" fn(Object) -> Object,
    kind: unsafe extern "C" fn(Object) -> u32,
    byref: unsafe extern "C" fn(Object) -> u32,
    type_class: unsafe extern "C" fn(Object) -> Object,
    pointer_type: unsafe extern "C" fn(Object) -> Object,
    value: unsafe extern "C" fn(Object) -> i32,
    delegate: unsafe extern "C" fn(Object) -> i32,
    is_enum: unsafe extern "C" fn(Object) -> i32,
    enum_base: unsafe extern "C" fn(Object) -> Object,
    size: unsafe extern "C" fn(Object, *mut u32) -> i32,
    generic: unsafe extern "C" fn(Object) -> u32,
    inflated: unsafe extern "C" fn(Object) -> u32,
    header: unsafe extern "C" fn(Object) -> Object,
    code: unsafe extern "C" fn(Object, *mut u32, *mut u32) -> *const u8,
    free: unsafe extern "C" fn(Object),
    object_class: unsafe extern "C" fn(Object) -> Object,
    assignable: unsafe extern "C" fn(Object, Object) -> i32,
    unbox: unsafe extern "C" fn(Object) -> Object,
    pub(super) invoke: unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object,
    closed: closed_types::Api,
}
impl Api {
    /// Only actual array objects need backing publication at an ordinary IL
    /// result boundary. Opaque reference helpers keep their existing protocols.
    pub(super) fn reference_array_result(self, object: Object) -> Result<bool, String> {
        if object.is_null() {
            return Ok(false);
        }
        let class = unsafe { (self.object_class)(object) };
        if class.is_null() {
            return Err("managed reference result has no actual class".into());
        }
        let p = unsafe { libc::dlsym(self.library, c"mono_class_get_rank".as_ptr()) };
        if p.is_null() {
            return Err("managed array result rank API absent".into());
        }
        let rank: unsafe extern "C" fn(Object) -> u32 = unsafe { std::mem::transmute(p) };
        Ok(unsafe { rank(class) } != 0)
    }
    /// Derive the private v2 object-handle result from the actual Unity wrapper:
    /// call <this icall>; call UnmarshalUnityObject<T>(IntPtr); ret. Method IDs,
    /// the result class and MethodSpec signatures all come from current metadata.
    pub(super) fn engine_handle_result(
        self,
        method: Object,
        owner: Object,
        engine_base: Object,
        images: &[Object],
    ) -> Result<Option<Object>, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(self.library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing engine handle metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let methods = sym!(
            "mono_class_get_methods",
            unsafe extern "C" fn(Object, *mut Object) -> Object
        );
        let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
        let resolve = sym!(
            "mono_get_method",
            unsafe extern "C" fn(Object, u32, Object) -> Object
        );
        let class_name = sym!(
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let namespace = sym!(
            "mono_class_get_namespace",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let intptr = sym!("mono_get_intptr_class", unsafe extern "C" fn() -> Object);
        let clause_count = sym!(
            "mono_method_header_get_num_clauses",
            unsafe extern "C" fn(Object) -> i32
        );
        let clause_next = sym!(
            "mono_method_header_get_clauses",
            unsafe extern "C" fn(Object, Object, *mut Object, *mut ExceptionClause) -> i32
        );
        let image = unsafe { (self.image)(owner) };
        let target = unsafe { token(method) };
        if target >> 24 != 6 || !images.contains(&image) {
            return Err("engine handle source method lacks owned MethodDef".into());
        }
        let mut result = None;
        let mut iterator = ptr::null_mut();
        loop {
            let wrapper = unsafe { methods(owner, &mut iterator) };
            if wrapper.is_null() {
                break;
            }
            let mut implementation = 0;
            let flags = unsafe { (self.flags)(wrapper, &mut implementation) };
            let signature = unsafe { (self.signature)(wrapper) };
            if implementation & (0x1000 | 3) != 0
                || flags & (0x400 | 0x2000) != 0
                || signature.is_null()
                || unsafe { (self.generic)(wrapper) } != 0
            {
                continue;
            }
            let returns = unsafe { (self.returns)(signature) };
            if returns.is_null()
                || unsafe { (self.byref)(returns) } != 0
                || unsafe { (self.kind)(returns) } != 0x12
            {
                continue;
            }
            let expected = unsafe { (self.type_class)(returns) };
            if expected.is_null()
                || !images.contains(&unsafe { (self.image)(expected) })
                || unsafe { (self.assignable)(engine_base, expected) } == 0
            {
                continue;
            }
            let header = unsafe { (self.header)(wrapper) };
            if header.is_null() {
                continue;
            }
            let (mut length, mut stack) = (0, 0);
            let code = unsafe { (self.code)(header, &mut length, &mut stack) };
            let converter = if code.is_null() || !(11..=128).contains(&length) || stack > 16 {
                None
            } else {
                let code = unsafe { std::slice::from_raw_parts(code, length as usize) };
                engine_handle_tail(code, target).or_else(|| {
                    if unsafe { clause_count(header) } != 1 {
                        return None;
                    }
                    let mut iterator = ptr::null_mut();
                    let mut clause = ExceptionClause::default();
                    if unsafe { clause_next(header, wrapper, &mut iterator, &mut clause) } == 0 {
                        return None;
                    }
                    engine_handle_finally_tail(code, target, clause)
                })
            };
            unsafe { (self.free)(header) };
            let Some(converter) = converter else {
                continue;
            };
            if converter >> 24 != 0x2b {
                continue;
            }
            let converter = unsafe { resolve(image, converter, owner) };
            if converter.is_null() {
                continue;
            }
            let converter_owner = unsafe { (self.class)(converter) };
            let text_is = |text: *const c_char, expected: &CStr| {
                !text.is_null() && unsafe { CStr::from_ptr(text) } == expected
            };
            if converter_owner.is_null()
                || unsafe { (self.image)(converter_owner) } != unsafe { (self.image)(engine_base) }
                || !text_is(
                    unsafe { namespace(converter_owner) },
                    c"UnityEngine.Bindings",
                )
                || !text_is(unsafe { class_name(converter_owner) }, c"Unmarshal")
                || !text_is(unsafe { (self.name)(converter) }, c"UnmarshalUnityObject")
            {
                continue;
            }
            let mut implementation = 0;
            let flags = unsafe { (self.flags)(converter, &mut implementation) };
            let signature = unsafe { (self.signature)(converter) };
            if signature.is_null()
                || flags & 0x10 == 0
                || flags & (0x400 | 0x2000) != 0
                || implementation & (0x1000 | 3) != 0
                || unsafe { (self.instance)(signature) } != 0
                || unsafe { (self.count)(signature) } != 1
            {
                continue;
            }
            let mut parameter = ptr::null_mut();
            let argument = unsafe { (self.parameters)(signature, &mut parameter) };
            let returns = unsafe { (self.returns)(signature) };
            if argument.is_null()
                || returns.is_null()
                || unsafe { (self.byref)(argument) } != 0
                || unsafe { (self.kind)(argument) } != 0x18
                || unsafe { (self.type_class)(argument) } != unsafe { intptr() }
                || unsafe { (self.byref)(returns) } != 0
                || unsafe { (self.kind)(returns) } != 0x12
                || unsafe { (self.type_class)(returns) } != expected
            {
                continue;
            }
            if result.is_some_and(|prior| prior != expected) {
                return Ok(None);
            }
            result = Some(expected);
        }
        Ok(result)
    }
    /// Infer engine-pointer arguments from ordinary generated wrappers. The
    /// wrapper loads its parameters in order and uses the actual closed
    /// Object.MarshalledUnityObject.Marshal<T> helper or an actual verified
    /// owned-buffer marshaller. Bounded null guards preserve the same origins.
    pub(super) fn engine_pointer_arguments(
        self,
        method: Object,
        owner: Object,
        engine_base: Object,
        images: &[Object],
    ) -> Result<Vec<Option<PointerSource>>, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(self.library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing engine pointer metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let methods = sym!(
            "mono_class_get_methods",
            unsafe extern "C" fn(Object, *mut Object) -> Object
        );
        let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
        let resolve = sym!(
            "mono_get_method",
            unsafe extern "C" fn(Object, u32, Object) -> Object
        );
        let class_name = sym!(
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let nesting = sym!(
            "mono_class_get_nesting_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let intptr = sym!("mono_get_intptr_class", unsafe extern "C" fn() -> Object);
        let object_class = sym!("mono_get_object_class", unsafe extern "C" fn() -> Object);
        let string_class = sym!("mono_get_string_class", unsafe extern "C" fn() -> Object);
        let void_class = sym!("mono_get_void_class", unsafe extern "C" fn() -> Object);
        let call_conv = sym!(
            "mono_signature_get_call_conv",
            unsafe extern "C" fn(Object) -> u32
        );
        let explicit_this = sym!(
            "mono_signature_explicit_this",
            unsafe extern "C" fn(Object) -> u32
        );
        let find = sym!(
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let get_method = sym!(
            "mono_class_get_method_from_name",
            unsafe extern "C" fn(Object, *const c_char, i32) -> Object
        );
        let throw_owner = unsafe {
            find(
                (self.image)(engine_base),
                c"UnityEngine.Bindings".as_ptr(),
                c"ThrowHelper".as_ptr(),
            )
        };
        let throw_method = if throw_owner.is_null() {
            ptr::null_mut()
        } else {
            unsafe { get_method(throw_owner, c"ThrowArgumentNullException".as_ptr(), 2) }
        };
        let checked_throw = !throw_method.is_null()
            && self
                .fixed_typed_managed(
                    throw_method,
                    throw_owner,
                    &[
                        (1, unsafe { void_class() }, false),
                        (0x1c, unsafe { object_class() }, false),
                        (0xe, unsafe { string_class() }, false),
                    ],
                    images,
                )
                .is_ok();
        let image = unsafe { (self.image)(owner) };
        let target = unsafe { token(method) };
        let signature = unsafe { (self.signature)(method) };
        if signature.is_null() || target >> 24 != 6 || !images.contains(&image) {
            return Err("engine pointer source lacks owned MethodDef/signature".into());
        }
        let arity = unsafe { (self.count)(signature) } as usize;
        if arity > 16 {
            return Err("engine pointer argument bound exceeded".into());
        }
        let mut result = None;
        let mut iterator = ptr::null_mut();
        loop {
            let wrapper = unsafe { methods(owner, &mut iterator) };
            if wrapper.is_null() {
                break;
            }
            let mut implementation = 0;
            let flags = unsafe { (self.flags)(wrapper, &mut implementation) };
            let sig = unsafe { (self.signature)(wrapper) };
            if implementation & (0x1000 | 3) != 0
                || flags & (0x400 | 0x2000) != 0
                || sig.is_null()
                || unsafe { (self.generic)(wrapper) } != 0
                || unsafe { (self.inflated)(wrapper) } != 0
            {
                continue;
            }
            let mut arguments = Vec::new();
            if unsafe { (self.instance)(sig) } != 0 {
                arguments.push((owner, false));
            }
            let mut parameter = ptr::null_mut();
            for _ in 0..unsafe { (self.count)(sig) } {
                let ty = unsafe { (self.parameters)(sig, &mut parameter) };
                if ty.is_null() {
                    return Err("engine wrapper parameter metadata absent".into());
                }
                arguments.push((
                    unsafe { (self.type_class)(ty) },
                    unsafe { (self.byref)(ty) } != 0,
                ));
            }
            if arguments.len() != arity {
                continue;
            }
            let header = unsafe { (self.header)(wrapper) };
            if header.is_null() {
                continue;
            }
            let (mut length, mut stack) = (0, 0);
            let code = unsafe { (self.code)(header, &mut length, &mut stack) };
            let inferred = if code.is_null() || !(6..=256).contains(&length) || stack > 16 {
                None
            } else {
                let mut marshal = |position: usize, converter| {
                    let (source, reference) = arguments[position];
                    if reference
                        || source.is_null()
                        || !images.contains(&unsafe { (self.image)(source) })
                    {
                        return None;
                    }
                    let converter = unsafe { resolve(image, converter, owner) };
                    if converter.is_null() {
                        return None;
                    }
                    if super::owned_resources::converter(source, converter) {
                        return Some(PointerSource::OwnedResource {
                            class: source as usize,
                            nullable: false,
                        });
                    }
                    let declaring = unsafe { (self.class)(converter) };
                    if declaring.is_null()
                        || unsafe { nesting(declaring) } != engine_base
                        || !text_matches(unsafe { class_name(declaring) }, c"MarshalledUnityObject")
                    {
                        return None;
                    }
                    let nullable = if text_matches(unsafe { (self.name)(converter) }, c"Marshal") {
                        true
                    } else if text_matches(unsafe { (self.name)(converter) }, c"MarshalNotNull") {
                        false
                    } else {
                        return None;
                    };
                    let sig = unsafe { (self.signature)(converter) };
                    let mut implementation = 0;
                    let flags = unsafe { (self.flags)(converter, &mut implementation) };
                    if sig.is_null()
                        || flags & 0x10 == 0
                        || flags & (0x400 | 0x2000) != 0
                        || implementation & (0x1000 | 3) != 0
                        || unsafe { (self.instance)(sig) } != 0
                        || unsafe { (self.count)(sig) } != 1
                    {
                        return None;
                    }
                    let mut iter = ptr::null_mut();
                    let ty = unsafe { (self.parameters)(sig, &mut iter) };
                    let returns = unsafe { (self.returns)(sig) };
                    if ty.is_null() || returns.is_null() {
                        return None;
                    }
                    let expected = unsafe { (self.type_class)(ty) };
                    if expected.is_null()
                        || unsafe { (self.byref)(ty) } != 0
                        || unsafe { (self.kind)(ty) } != 0x12
                        || !images.contains(&unsafe { (self.image)(expected) })
                        || unsafe { (self.assignable)(engine_base, expected) } == 0
                        || unsafe { (self.assignable)(expected, source) } == 0
                        || unsafe { (self.byref)(returns) } != 0
                        || unsafe { (self.kind)(returns) } != 0x18
                        || unsafe { (self.type_class)(returns) } != unsafe { intptr() }
                    {
                        return None;
                    }
                    Some(PointerSource::Engine { nullable })
                };
                let code = unsafe { std::slice::from_raw_parts(code, length as usize) };
                engine_pointer_prefix(code, target, arity, &mut marshal)
                    .or_else(|| {
                        engine_pointer_flow_with_calls(
                            code,
                            target,
                            arity,
                            &mut marshal,
                            |called| {
                                checked_throw
                                    && unsafe { resolve(image, called, owner) } == throw_method
                            },
                            |called| {
                                let called = unsafe { resolve(image, called, owner) };
                                if called.is_null() {
                                    return None;
                                }
                                let class = unsafe { (self.class)(called) };
                                if class.is_null()
                                    || !images.contains(&unsafe { (self.image)(class) })
                                {
                                    return None;
                                }
                                let signature = unsafe { (self.signature)(called) };
                                if signature.is_null()
                                    || unsafe { call_conv(signature) } != 0
                                    || unsafe { explicit_this(signature) } != 0
                                {
                                    return None;
                                }
                                let returns = unsafe { (self.returns)(signature) };
                                if returns.is_null() {
                                    return None;
                                }
                                let count = unsafe { (self.count)(signature) } as usize
                                    + usize::from(unsafe { (self.instance)(signature) } != 0);
                                (count <= 16)
                                    .then_some((count, unsafe { (self.kind)(returns) } != 1))
                            },
                        )
                    })
                    .or_else(|| {
                        engine_pointer_nullable_prefix(
                            code,
                            target,
                            arity,
                            &mut marshal,
                            PointerSource::nullable,
                        )
                    })
            };
            unsafe { (self.free)(header) };
            let Some(inferred) = inferred else { continue };
            if result.as_ref().is_some_and(|prior| prior != &inferred) {
                return Ok(vec![None; arity]);
            }
            result = Some(inferred);
        }
        Ok(result.unwrap_or_else(|| vec![None; arity]))
    }
    /// ManagedSpanWrapper also carries numeric buffers. Admit UTF16 only when
    /// the actual wrapper passes a local prepared by the actual StringMarshaller
    /// from the corresponding String argument. Ambiguous/complex flows defer.
    pub(super) fn engine_utf16_arguments(
        self,
        method: Object,
        owner: Object,
        span: Object,
        images: &[Object],
    ) -> Result<Vec<bool>, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(self.library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing string wrapper metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let find = sym!(
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let member = sym!(
            "mono_class_get_method_from_name",
            unsafe extern "C" fn(Object, *const c_char, i32) -> Object
        );
        let methods = sym!(
            "mono_class_get_methods",
            unsafe extern "C" fn(Object, *mut Object) -> Object
        );
        let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
        let resolve = sym!(
            "mono_get_method",
            unsafe extern "C" fn(Object, u32, Object) -> Object
        );
        let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
        let corlib = unsafe { corlib() };
        let string = unsafe { find(corlib, c"System".as_ptr(), c"String".as_ptr()) };
        let boolean = unsafe { find(corlib, c"System".as_ptr(), c"Boolean".as_ptr()) };
        let helper_class = unsafe {
            find(
                (self.image)(span),
                c"UnityEngine.Bindings".as_ptr(),
                c"StringMarshaller".as_ptr(),
            )
        };
        if string.is_null() || boolean.is_null() || helper_class.is_null() {
            return Err("string wrapper actual helper classes absent".into());
        }
        let helper = unsafe { member(helper_class, c"TryMarshalEmptyOrNullString".as_ptr(), 2) };
        self.fixed_typed_managed(
            helper,
            helper_class,
            &[
                (2, boolean, false),
                (0xe, string, false),
                (0x11, span, true),
            ],
            images,
        )?;
        let image = unsafe { (self.image)(owner) };
        let target = unsafe { token(method) };
        let signature = unsafe { (self.signature)(method) };
        if target >> 24 != 6 || signature.is_null() || !images.contains(&image) {
            return Err("string wrapper source lacks owned MethodDef/signature".into());
        }
        let arity = unsafe { (self.count)(signature) } as usize;
        if arity > 16 {
            return Err("string wrapper arity exceeds bound".into());
        }
        let mut span_positions = Vec::new();
        let mut trailing_value = None;
        let mut parameter = ptr::null_mut();
        for position in 0..arity {
            let ty = unsafe { (self.parameters)(signature, &mut parameter) };
            if ty.is_null() {
                return Err("string wrapper target parameter absent".into());
            }
            if unsafe { (self.type_class)(ty) } == span && unsafe { (self.byref)(ty) } != 0 {
                span_positions.push(position);
            }
            if position + 1 == arity
                && unsafe { (self.kind)(ty) } == 0x11
                && unsafe { (self.byref)(ty) } != 0
                && unsafe { (self.type_class)(ty) } != span
            {
                trailing_value = Some(unsafe { (self.type_class)(ty) });
            }
        }
        let target_return = unsafe { (self.returns)(signature) };
        if target_return.is_null() || unsafe { (self.kind)(target_return) } != 1 {
            trailing_value = None;
        }
        let mut result = None;
        let mut iterator = ptr::null_mut();
        loop {
            let wrapper = unsafe { methods(owner, &mut iterator) };
            if wrapper.is_null() {
                break;
            }
            let mut implementation = 0;
            let flags = unsafe { (self.flags)(wrapper, &mut implementation) };
            let signature = unsafe { (self.signature)(wrapper) };
            if signature.is_null()
                || !managed_il_flags(
                    flags,
                    implementation,
                    unsafe { (self.instance)(signature) } != 0,
                )
                || unsafe { (self.generic)(wrapper) } != 0
                || unsafe { (self.inflated)(wrapper) } != 0
            {
                continue;
            }
            let mut string_arguments = Vec::new();
            if unsafe { (self.instance)(signature) } != 0 {
                string_arguments.push(false);
            }
            let mut parameter = ptr::null_mut();
            for _ in 0..unsafe { (self.count)(signature) } {
                let ty = unsafe { (self.parameters)(signature, &mut parameter) };
                if ty.is_null() {
                    return Err("string wrapper source parameter absent".into());
                }
                string_arguments.push(
                    unsafe { (self.kind)(ty) } == 0xe
                        && unsafe { (self.byref)(ty) } == 0
                        && unsafe { (self.type_class)(ty) } == string,
                );
            }
            let output = string_arguments.len() + 1 == arity
                && trailing_value.is_some_and(|class| {
                    let ret = unsafe { (self.returns)(signature) };
                    !ret.is_null()
                        && unsafe { (self.kind)(ret) } == 0x11
                        && unsafe { (self.type_class)(ret) } == class
                        && unsafe { (self.byref)(ret) } == 0
                });
            if string_arguments.len() != arity && !output {
                continue;
            }
            let header = unsafe { (self.header)(wrapper) };
            if header.is_null() {
                continue;
            }
            let (mut length, mut stack) = (0, 0);
            let code = unsafe { (self.code)(header, &mut length, &mut stack) };
            let inferred = if code.is_null() || !(6..=512).contains(&length) || stack > 32 {
                None
            } else {
                engine_utf16_locals_with_output(
                    unsafe { std::slice::from_raw_parts(code, length as usize) },
                    target,
                    arity,
                    &span_positions,
                    output,
                    |position, called| {
                        string_arguments.get(position) == Some(&true)
                            && unsafe { resolve(image, called, owner) } == helper
                    },
                )
            };
            unsafe { (self.free)(header) };
            let Some(inferred) = inferred else {
                continue;
            };
            if result.as_ref().is_some_and(|prior| prior != &inferred) {
                return Ok(vec![false; arity]);
            }
            result = Some(inferred);
        }
        Ok(result.unwrap_or_else(|| vec![false; arity]))
    }
    /// Prove the output allocator protocol from the complete generated wrapper
    /// and its real finally clause. Span names and widths alone cannot establish
    /// whether an IntPtr contains UTF16, numeric data or borrowed memory.
    pub(super) fn engine_utf16_output(
        self,
        method: Object,
        owner: Object,
        span: Object,
        images: &[Object],
    ) -> Result<bool, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(self.library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing output wrapper metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let find = sym!(
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let member = sym!(
            "mono_class_get_method_from_name",
            unsafe extern "C" fn(Object, *const c_char, i32) -> Object
        );
        let methods = sym!(
            "mono_class_get_methods",
            unsafe extern "C" fn(Object, *mut Object) -> Object
        );
        let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
        let resolve = sym!(
            "mono_get_method",
            unsafe extern "C" fn(Object, u32, Object) -> Object
        );
        let string = sym!("mono_get_string_class", unsafe extern "C" fn() -> Object);
        let locals = sym!(
            "mono_method_header_get_locals",
            unsafe extern "C" fn(Object, *mut u32, *mut i32) -> *const Object
        );
        let clause_count = sym!(
            "mono_method_header_get_num_clauses",
            unsafe extern "C" fn(Object) -> i32
        );
        let clause_next = sym!(
            "mono_method_header_get_clauses",
            unsafe extern "C" fn(Object, Object, *mut Object, *mut ExceptionClause) -> i32
        );
        let helper_class = unsafe {
            find(
                (self.image)(span),
                c"UnityEngine.Bindings".as_ptr(),
                c"OutStringMarshaller".as_ptr(),
            )
        };
        let helper = unsafe { member(helper_class, c"GetStringAndDispose".as_ptr(), 1) };
        let string = unsafe { string() };
        if helper_class.is_null() || helper.is_null() || string.is_null() {
            return Ok(false);
        }
        let signature = unsafe { (self.signature)(helper) };
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(helper, &mut implementation) };
        if signature.is_null()
            || !managed_il_flags(flags, implementation, false)
            || unsafe { (self.class)(helper) } != helper_class
            || unsafe { (self.generic)(helper) } != 0
            || unsafe { (self.inflated)(helper) } != 0
            || unsafe { (self.instance)(signature) } != 0
            || unsafe { (self.count)(signature) } != 1
        {
            return Ok(false);
        }
        let ret = unsafe { (self.returns)(signature) };
        let mut iterator = ptr::null_mut();
        let input = unsafe { (self.parameters)(signature, &mut iterator) };
        let exact = |ty: Object, kind, class| {
            !ty.is_null()
                && unsafe { (self.kind)(ty) } == kind
                && unsafe { (self.type_class)(ty) } == class
                && unsafe { (self.byref)(ty) } == 0
        };
        if !exact(ret, 0xe, string) || !exact(input, 0x11, span) {
            return Ok(false);
        }
        let image = unsafe { (self.image)(owner) };
        let target = unsafe { token(method) };
        if !images.contains(&image) || target >> 24 != 6 {
            return Ok(false);
        }
        let mut iterator = ptr::null_mut();
        loop {
            let wrapper = unsafe { methods(owner, &mut iterator) };
            if wrapper.is_null() {
                break;
            }
            let signature = unsafe { (self.signature)(wrapper) };
            let mut implementation = 0;
            let flags = unsafe { (self.flags)(wrapper, &mut implementation) };
            if signature.is_null()
                || !managed_il_flags(
                    flags,
                    implementation,
                    unsafe { (self.instance)(signature) } != 0,
                )
                || unsafe { (self.generic)(wrapper) } != 0
                || unsafe { (self.inflated)(wrapper) } != 0
                || !exact(unsafe { (self.returns)(signature) }, 0xe, string)
            {
                continue;
            }
            let header = unsafe { (self.header)(wrapper) };
            if header.is_null() {
                continue;
            }
            let (mut length, mut stack, mut count, mut init) = (0, 0, 0, 0);
            let code = unsafe { (self.code)(header, &mut length, &mut stack) };
            let types = unsafe { locals(header, &mut count, &mut init) };
            let mut iterator = ptr::null_mut();
            let mut clause = ExceptionClause::default();
            let valid = if code.is_null()
                || !(10..=512).contains(&length)
                || stack > 32
                || count > 32
                || types.is_null()
                || init == 0
                || unsafe { clause_count(header) } != 1
                || unsafe { clause_next(header, wrapper, &mut iterator, &mut clause) } == 0
            {
                false
            } else if let Some((native, result, converter)) = engine_owned_output_tail(
                unsafe { std::slice::from_raw_parts(code, length as usize) },
                target,
                clause,
            ) {
                native < count
                    && result < count
                    && exact(unsafe { *types.add(native as usize) }, 0x11, span)
                    && exact(unsafe { *types.add(result as usize) }, 0xe, string)
                    && unsafe { resolve(image, converter, owner) } == helper
            } else {
                false
            };
            unsafe { (self.free)(header) };
            if valid {
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// Read a bounded, pinned actual Exception chain without executing ToString IL.
    pub(super) fn exception_diagnostic(
        self,
        roots: super::instances::Api,
        exception: Object,
    ) -> Result<String, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(self.library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing exception diagnostic API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let base = sym!("mono_get_exception_class", unsafe extern "C" fn() -> Object);
        let class_name = sym!(
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let namespace = sym!(
            "mono_class_get_namespace",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let field = sym!(
            "mono_class_get_field_from_name",
            unsafe extern "C" fn(Object, *const c_char) -> Object
        );
        let parent = sym!(
            "mono_field_get_parent",
            unsafe extern "C" fn(Object) -> Object
        );
        let field_type = sym!(
            "mono_field_get_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let field_flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
        let field_get = sym!(
            "mono_field_get_value",
            unsafe extern "C" fn(Object, Object, Object)
        );
        let string_class = sym!("mono_get_string_class", unsafe extern "C" fn() -> Object);
        let length = sym!("mono_string_length", unsafe extern "C" fn(Object) -> i32);
        let chars = sym!(
            "mono_string_chars",
            unsafe extern "C" fn(Object) -> *const u16
        );
        let base = unsafe { base() };
        if base.is_null() {
            return Err("actual System.Exception metadata absent".into());
        }
        let text = |p: *const c_char| -> Result<String, String> {
            if p.is_null() {
                return Err("exception class identity absent".into());
            }
            Ok(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        };
        let message_field = unsafe { field(base, c"_message".as_ptr()) };
        let inner_field = unsafe { field(base, c"_innerException".as_ptr()) };
        for (candidate, expected_kind, expected_class) in [
            (message_field, 14, unsafe { string_class() }),
            (inner_field, 0x12, base),
        ] {
            if candidate.is_null() {
                continue;
            }
            let ty = unsafe { field_type(candidate) };
            if unsafe { parent(candidate) } != base
                || unsafe { field_flags(candidate) } & 0x10 != 0
                || ty.is_null()
                || unsafe { (self.kind)(ty) } != expected_kind
                || unsafe { (self.byref)(ty) } != 0
                || unsafe { (self.type_class)(ty) } != expected_class
            {
                return Err("Exception diagnostic field metadata changed".into());
            }
        }
        let mut pinned = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut parts = Vec::new();
        let mut next = exception;
        while !next.is_null() && parts.len() < 8 {
            if !seen.insert(next as usize) {
                parts.push("[inner exception cycle]".into());
                next = ptr::null_mut();
                break;
            }
            pinned.push(roots.worker_root(next)?);
            let exception = pinned.last().unwrap().target()?;
            let class = unsafe { (self.object_class)(exception) };
            if class.is_null() || unsafe { (self.assignable)(base, class) } == 0 {
                return Err("invoke exception is not an actual System.Exception".into());
            }
            let mut identity = format!(
                "{}.{}",
                text(unsafe { namespace(class) })?,
                text(unsafe { class_name(class) })?
            );
            if !message_field.is_null() {
                let mut message: Object = ptr::null_mut();
                unsafe {
                    field_get(
                        exception,
                        message_field,
                        (&mut message as *mut Object).cast(),
                    )
                };
                if !message.is_null() {
                    let message_root = roots.worker_root(message)?;
                    let message = message_root.target()?;
                    if unsafe { (self.object_class)(message) } != unsafe { string_class() } {
                        return Err("Exception message has incompatible native class".into());
                    }
                    let length = unsafe { length(message) };
                    let data = unsafe { chars(message) };
                    if length < 0 || data.is_null() {
                        return Err("Exception message storage absent".into());
                    }
                    let message = String::from_utf16_lossy(unsafe {
                        std::slice::from_raw_parts(data, (length as usize).min(2048))
                    });
                    identity.push_str(&format!(": {message:?}"));
                }
            }
            parts.push(identity);
            next = ptr::null_mut();
            if !inner_field.is_null() {
                unsafe { field_get(exception, inner_field, (&mut next as *mut Object).cast()) };
            }
        }
        if !next.is_null() {
            parts.push("[inner exception depth limit]".into());
        }
        Ok(parts.join(" -> "))
    }
    /// Unity's SetupCoroutine IL entry treats IntPtr as a Boolean output address.
    /// Its native address is substituted with private storage by Frame, never
    /// admitted to the general scalar/reference ABI.
    pub(super) fn describe_coroutine(
        self,
        method: Object,
        core: Object,
        images: &[Object],
    ) -> Result<Option<CoroutineDescriptor>, String> {
        if method.is_null() {
            return Err("NULL coroutine method".into());
        }
        let text = unsafe { (self.name)(method) };
        if text.is_null() || unsafe { CStr::from_ptr(text) } != c"InvokeMoveNext" {
            return Ok(None);
        }
        let find = unsafe { libc::dlsym(self.library, c"mono_class_from_name".as_ptr()) };
        let corlib = unsafe { libc::dlsym(self.library, c"mono_get_corlib".as_ptr()) };
        if find.is_null() || corlib.is_null() {
            return Err("coroutine metadata API absent".into());
        }
        let find: unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object =
            unsafe { std::mem::transmute(find) };
        let corlib: unsafe extern "C" fn() -> Object = unsafe { std::mem::transmute(corlib) };
        let owner = unsafe { find(core, c"UnityEngine".as_ptr(), c"SetupCoroutine".as_ptr()) };
        if owner.is_null() || unsafe { (self.class)(method) } != owner {
            return Ok(None);
        }
        let corlib = unsafe { corlib() };
        let enumerator = unsafe {
            find(
                corlib,
                c"System.Collections".as_ptr(),
                c"IEnumerator".as_ptr(),
            )
        };
        let intptr = unsafe { find(corlib, c"System".as_ptr(), c"IntPtr".as_ptr()) };
        let boolean = unsafe { find(corlib, c"System".as_ptr(), c"Boolean".as_ptr()) };
        let signature = unsafe { (self.signature)(method) };
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        if [core, corlib, enumerator, intptr, boolean, signature]
            .iter()
            .any(|p| p.is_null())
            || !images.contains(&core)
            || !images.contains(&corlib)
            || unsafe { (self.image)(owner) } != core
            || !managed_il_flags(flags, implementation, false)
            || !concrete_method(
                unsafe { (self.generic)(method) } != 0,
                unsafe { (self.inflated)(method) } != 0,
                false,
                false,
            )
            || unsafe { (self.instance)(signature) } != 0
            || unsafe { (self.count)(signature) } != 2
        {
            return Err("coroutine entry differs from static IL contract".into());
        }
        let mut iter = ptr::null_mut();
        for (ty, expected_kind, expected_class) in [
            (unsafe { (self.returns)(signature) }, 1, ptr::null_mut()),
            (
                unsafe { (self.parameters)(signature, &mut iter) },
                0x12,
                enumerator,
            ),
            (
                unsafe { (self.parameters)(signature, &mut iter) },
                0x18,
                intptr,
            ),
        ] {
            if ty.is_null()
                || unsafe { (self.kind)(ty) } != expected_kind
                || unsafe { (self.byref)(ty) } != 0
                || (!expected_class.is_null() && unsafe { (self.type_class)(ty) } != expected_class)
            {
                return Err("coroutine exact IEnumerator/IntPtr/Void signature changed".into());
            }
        }
        self.closed.concrete(owner)?;
        let mut alignment = 0;
        if unsafe { (self.size)(intptr, &mut alignment) } != 8 || alignment != 8 {
            return Err("coroutine IntPtr is not 64-bit".into());
        }
        let header = unsafe { (self.header)(method) };
        if header.is_null() {
            return Err("coroutine IL body absent".into());
        }
        let (mut length, mut stack) = (0, 0);
        let code = unsafe { (self.code)(header, &mut length, &mut stack) };
        let valid = !code.is_null() && (1..=4096).contains(&length) && stack <= 16;
        unsafe { (self.free)(header) };
        if !valid {
            return Err("coroutine IL body exceeds fixed bound".into());
        }
        Ok(Some(CoroutineDescriptor {
            call: Descriptor {
                receiver: None,
                parameters: vec![
                    Parameter {
                        shape: Shape::Reference {
                            class: enumerator,
                            string: false,
                        },
                        byref: false,
                    },
                    Parameter {
                        shape: Shape::Scalar {
                            class: intptr,
                            width: 8,
                            alignment: 8,
                            boolean: false,
                        },
                        byref: false,
                    },
                ],
                returns: None,
                name: "UnityEngine.SetupCoroutine::InvokeMoveNext".into(),
            },
            boolean,
        }))
    }
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing typed managed call API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            library,
            class: sym!(
                "mono_method_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            image: sym!(
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            ),
            name: sym!(
                "mono_method_get_name",
                unsafe extern "C" fn(Object) -> *const c_char
            ),
            flags: sym!(
                "mono_method_get_flags",
                unsafe extern "C" fn(Object, *mut u32) -> u32
            ),
            signature: sym!(
                "mono_method_signature",
                unsafe extern "C" fn(Object) -> Object
            ),
            instance: sym!(
                "mono_signature_is_instance",
                unsafe extern "C" fn(Object) -> u32
            ),
            count: sym!(
                "mono_signature_get_param_count",
                unsafe extern "C" fn(Object) -> u32
            ),
            parameters: sym!(
                "mono_signature_get_params",
                unsafe extern "C" fn(Object, *mut Object) -> Object
            ),
            returns: sym!(
                "mono_signature_get_return_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            kind: sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32),
            byref: sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32),
            type_class: sym!(
                "mono_class_from_mono_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            pointer_type: sym!(
                "mono_type_get_ptr_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            value: sym!(
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            ),
            delegate: sym!(
                "mono_class_is_delegate",
                unsafe extern "C" fn(Object) -> i32
            ),
            is_enum: sym!("mono_class_is_enum", unsafe extern "C" fn(Object) -> i32),
            enum_base: sym!(
                "mono_class_enum_basetype",
                unsafe extern "C" fn(Object) -> Object
            ),
            size: sym!(
                "mono_class_value_size",
                unsafe extern "C" fn(Object, *mut u32) -> i32
            ),
            generic: sym!(
                "unity_mono_method_is_generic",
                unsafe extern "C" fn(Object) -> u32
            ),
            inflated: sym!(
                "unity_mono_method_is_inflated",
                unsafe extern "C" fn(Object) -> u32
            ),
            header: sym!(
                "mono_method_get_header",
                unsafe extern "C" fn(Object) -> Object
            ),
            code: sym!(
                "mono_method_header_get_code",
                unsafe extern "C" fn(Object, *mut u32, *mut u32) -> *const u8
            ),
            free: sym!("mono_metadata_free_mh", unsafe extern "C" fn(Object)),
            object_class: sym!(
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            assignable: sym!(
                "mono_class_is_assignable_from",
                unsafe extern "C" fn(Object, Object) -> i32
            ),
            unbox: sym!("mono_object_unbox", unsafe extern "C" fn(Object) -> Object),
            invoke: sym!(
                "mono_runtime_invoke",
                unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object
            ),
            closed: closed_types::Api::load(library)?,
        })
    }
    pub(super) fn shape(self, ty: Object, images: &[Object]) -> Result<Shape, String> {
        if ty.is_null() {
            return Err("managed parameter/return type missing".into());
        }
        let class = unsafe { (self.type_class)(ty) };
        if class.is_null() || !images.contains(&unsafe { (self.image)(class) }) {
            return Err("managed argument type outside fingerprinted image family".into());
        }
        self.closed.concrete(class)?;
        let kind = unsafe { (self.kind)(ty) };
        let scalar = match kind {
            2 | 4 | 5 => Some((1, 1, kind == 2)),
            3 | 6 | 7 => Some((2, 2, false)),
            8 | 9 | 0xc => Some((4, 4, false)),
            0xa | 0xb | 0xd => Some((8, 8, false)),
            0x11 if unsafe { (self.is_enum)(class) } != 0 => {
                let base = unsafe { (self.enum_base)(class) };
                if base.is_null() {
                    return Err("managed enum lacks base type".into());
                }
                let width = super::value_layout::enum_width(unsafe { (self.kind)(base) })
                    .ok_or("managed enum base is not integer")?;
                Some((width, width, false))
            }
            0x11 => {
                // Runtime invoke takes a pointer to value bytes. A small
                // aggregate with complete scalar coverage fits the existing
                // native-stack word, with no pointer/padding/reference copying.
                let layout =
                    super::value_layout::Api::load(self.library, unsafe { (self.image)(class) })?
                        .layout_in(class, images)?;
                let (width, alignment) = layout.scalar_inline()?;
                if !matches!(width, 1 | 2 | 4 | 8) {
                    return Err("managed scalar aggregate exceeds native-stack word".into());
                }
                Some((width, alignment, false))
            }
            _ => None,
        };
        if let Some((width, expected_alignment, boolean)) = scalar {
            let mut alignment = 0;
            if unsafe { (self.value)(class) } == 0
                || unsafe { (self.size)(class, &mut alignment) } != width as i32
                || alignment as usize != expected_alignment
            {
                return Err("managed scalar size/alignment mismatch".into());
            }
            Ok(Shape::Scalar {
                class,
                width,
                alignment: expected_alignment,
                boolean,
            })
        } else if matches!(kind, 0xe | 0x12 | 0x15 | 0x1c | 0x1d)
            && unsafe { (self.value)(class) } == 0
        {
            Ok(Shape::Reference {
                class,
                string: kind == 0xe,
            })
        } else {
            Err("managed argument requires unsupported typed ABI".into())
        }
    }
    pub(super) fn describe(
        self,
        method: Object,
        images: &[Object],
        reference_images: &[Object],
    ) -> Result<Option<Descriptor>, String> {
        self.describe_inner(method, images, reference_images, false)
    }
    /// Only non-generic instance methods of a closed constructed declaring
    /// class. The complete declaring/argument shapes and IL are still checked.
    pub(super) fn describe_constructed_instance(
        self,
        method: Object,
        images: &[Object],
        reference_images: &[Object],
    ) -> Result<Option<Descriptor>, String> {
        self.describe_inner(method, images, reference_images, true)
    }
    fn describe_inner(
        self,
        method: Object,
        images: &[Object],
        reference_images: &[Object],
        constructed: bool,
    ) -> Result<Option<Descriptor>, String> {
        if method.is_null() {
            return Err("NULL managed method".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let name = unsafe { (self.name)(method) };
        if name.is_null() {
            return Err("managed method name missing".into());
        }
        let name = unsafe { CStr::from_ptr(name) };
        // Constructors have their existing allocation/interface-specific
        // contracts. Ordinary calls never bypass those through this route.
        if matches!(name.to_bytes(), b".ctor" | b".cctor") {
            return Ok(None);
        }
        let class = unsafe { (self.class)(method) };
        let sig = unsafe { (self.signature)(method) };
        // Delegate Invoke is a runtime method rather than a CIL body. Mono's
        // existing runtime_invoke routes it through its interpreter wrapper;
        // we retain the same rooted receiver, argument and exception protocol.
        let delegate = !class.is_null()
            && !sig.is_null()
            && name == c"Invoke"
            && unsafe { (self.delegate)(class) } != 0
            && delegate_invoke_flags(flags, implementation, unsafe { (self.instance)(sig) } != 0);
        if class.is_null()
            || !images.contains(&unsafe { (self.image)(class) })
            || sig.is_null()
            || (!delegate
                && !managed_il_flags(flags, implementation, unsafe { (self.instance)(sig) } != 0))
            || !concrete_method(
                unsafe { (self.generic)(method) } != 0,
                unsafe { (self.inflated)(method) } != 0,
                unsafe { (self.instance)(sig) } != 0,
                constructed || delegate && unsafe { (self.inflated)(method) } != 0,
            )
        {
            return Err("managed method outside owned concrete IL contract".into());
        }
        if constructed {
            self.closed.constructed(class)?;
        } else {
            self.closed.concrete(class)?;
        }
        let receiver = if flags & 0x10 == 0 {
            if unsafe { (self.value)(class) } != 0 {
                return Err("managed value receiver needs a separate unboxed ABI".into());
            }
            Some(class)
        } else {
            None
        };
        let count = unsafe { (self.count)(sig) };
        if count > 16 {
            return Err("managed parameter count exceeds bound".into());
        }
        let ret = unsafe { (self.returns)(sig) };
        if ret.is_null() || unsafe { (self.byref)(ret) } != 0 {
            return Err("managed return type absent/byref".into());
        }
        let returns = if unsafe { (self.kind)(ret) } == 1 {
            None
        } else {
            Some(self.shape(ret, reference_images)?)
        };
        let mut parameters = Vec::new();
        let mut iterator = ptr::null_mut();
        for _ in 0..count {
            let ty = unsafe { (self.parameters)(sig, &mut iterator) };
            let shape = self.shape(ty, reference_images)?;
            let byref = unsafe { (self.byref)(ty) } != 0;
            if byref && matches!(shape, Shape::Reference { string: false, .. }) {
                return Err("managed byref reference requires String output adapter".into());
            }
            parameters.push(Parameter { shape, byref });
        }
        if !delegate {
            let header = unsafe { (self.header)(method) };
            if header.is_null() {
                return Err("managed method has no IL body".into());
            }
            let mut length = 0;
            let mut stack = 0;
            let code = unsafe { (self.code)(header, &mut length, &mut stack) };
            unsafe { (self.free)(header) };
            if code.is_null() || !(1..=65536).contains(&length) || stack > 256 {
                return Err("managed IL body exceeds bounds".into());
            }
        }
        Ok(Some(Descriptor {
            receiver,
            parameters,
            returns,
            name: name.to_string_lossy().into_owned(),
        }))
    }
    /// Fixed compiled reference -> reference internal-call adapters check the
    /// actual static metadata signature before registering any native callback.
    pub(super) fn reference_icall(
        self,
        method: Object,
        owner: Object,
        argument: Object,
        result: Object,
    ) -> Result<(), String> {
        if method.is_null() || unsafe { (self.class)(method) } != owner {
            return Err("reference icall owner mismatch".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.signature)(method) };
        if sig.is_null()
            || flags & 0x10 == 0
            || flags & (0x400 | 0x2000) != 0
            || implementation & 0x1000 == 0
            || unsafe { (self.instance)(sig) } != 0
            || unsafe { (self.count)(sig) } != 1
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
        {
            return Err("reference icall is not a concrete static InternalCall".into());
        }
        let mut iterator = ptr::null_mut();
        for (ty, class) in [
            (unsafe { (self.parameters)(sig, &mut iterator) }, argument),
            (unsafe { (self.returns)(sig) }, result),
        ] {
            if ty.is_null()
                || class.is_null()
                || unsafe { (self.byref)(ty) } != 0
                || !matches!(unsafe { (self.kind)(ty) }, 0x12 | 0x1d)
                || unsafe { (self.type_class)(ty) } != class
                || unsafe { (self.value)(class) } != 0
            {
                return Err("reference icall argument/return ABI mismatch".into());
            }
            self.closed.concrete(class)?;
        }
        Ok(())
    }
    pub(super) fn find_objects_icall(
        self,
        method: Object,
        owner: Object,
        arguments: [Object; 3],
        result: Object,
    ) -> Result<(), String> {
        if method.is_null() || owner.is_null() || unsafe { (self.class)(method) } != owner {
            return Err("object query icall owner mismatch".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.signature)(method) };
        if sig.is_null()
            || flags & 0x10 == 0
            || flags & (0x400 | 0x2000) != 0
            || implementation & 0x1000 == 0
            || unsafe { (self.instance)(sig) } != 0
            || unsafe { (self.count)(sig) } != 3
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
        {
            return Err("object query is not the exact static InternalCall".into());
        }
        let mut iterator = ptr::null_mut();
        for (index, expected) in std::iter::once(result).chain(arguments).enumerate() {
            let ty = unsafe {
                if index == 0 {
                    (self.returns)(sig)
                } else {
                    (self.parameters)(sig, &mut iterator)
                }
            };
            let kind = match index {
                0 => 0x1d,
                1 => 0x12,
                _ => 0x11,
            };
            if ty.is_null()
                || expected.is_null()
                || unsafe { (self.byref)(ty) } != 0
                || unsafe { (self.kind)(ty) } != kind
                || unsafe { (self.type_class)(ty) } != expected
            {
                return Err("object query exact reference/enum class mismatch".into());
            }
            self.closed.concrete(expected)?;
            if index < 2 {
                if unsafe { (self.value)(expected) } != 0 {
                    return Err("object query reference is a value class".into());
                }
            } else {
                let mut alignment = 0;
                if unsafe { (self.is_enum)(expected) } == 0
                    || unsafe { (self.kind)((self.enum_base)(expected)) } != 8
                    || unsafe { (self.size)(expected, &mut alignment) } != 4
                    || alignment != 4
                {
                    return Err("object query enum is not actual Int32 storage".into());
                }
            }
        }
        Ok(())
    }
    pub(super) fn type_bool_intptr_icall(
        self,
        method: Object,
        owner: Object,
        argument: Object,
    ) -> Result<(), String> {
        if method.is_null() || unsafe { (self.class)(method) } != owner {
            return Err("factory icall owner mismatch".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.signature)(method) };
        if sig.is_null()
            || flags & 0x10 == 0
            || flags & (0x400 | 0x2000) != 0
            || implementation & 0x1000 == 0
            || unsafe { (self.instance)(sig) } != 0
            || unsafe { (self.count)(sig) } != 2
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
        {
            return Err("factory icall is not a concrete static InternalCall".into());
        }
        let ret = unsafe { (self.returns)(sig) };
        let mut iterator = ptr::null_mut();
        let ty = unsafe { (self.parameters)(sig, &mut iterator) };
        let boolean = unsafe { (self.parameters)(sig, &mut iterator) };
        if ret.is_null()
            || ty.is_null()
            || boolean.is_null()
            || unsafe { (self.kind)(ret) } != 0x18
            || unsafe { (self.byref)(ret) } != 0
            || unsafe { (self.kind)(ty) } != 0x12
            || unsafe { (self.byref)(ty) } != 0
            || unsafe { (self.type_class)(ty) } != argument
            || unsafe { (self.kind)(boolean) } != 2
            || unsafe { (self.byref)(boolean) } != 0
        {
            return Err("factory icall Type/Boolean/IntPtr ABI mismatch".into());
        }
        let class = unsafe { (self.type_class)(ret) };
        let mut alignment = 0;
        if class.is_null()
            || unsafe { (self.value)(class) } == 0
            || unsafe { (self.size)(class, &mut alignment) } != 8
            || alignment != 8
        {
            return Err("factory IntPtr is not native 64-bit scalar storage".into());
        }
        self.closed.concrete(argument)
    }
    /// Compiled scalar adapters verify the exact primitive/enum classes and
    /// InternalCall signature. No IntPtr, reference or byref is admitted here.
    pub(super) fn scalar_icall(
        self,
        method: Object,
        owner: Object,
        returns: Object,
        arguments: &[Object],
        images: &[Object],
    ) -> Result<(), String> {
        self.fixed_scalar_icall(method, owner, returns, arguments, images, false)
    }
    /// Marker IntPtr values are opaque owned guest identities, never native
    /// dereferenceable pointers. This does not admit IntPtr to ordinary IL ABI.
    pub(super) fn marker_icall(
        self,
        method: Object,
        owner: Object,
        returns: Object,
        intptr: Object,
        images: &[Object],
    ) -> Result<(), String> {
        self.fixed_scalar_icall(method, owner, returns, &[intptr], images, true)
    }
    fn fixed_scalar_icall(
        self,
        method: Object,
        owner: Object,
        returns: Object,
        arguments: &[Object],
        images: &[Object],
        marker: bool,
    ) -> Result<(), String> {
        if method.is_null()
            || owner.is_null()
            || !images.contains(&unsafe { (self.image)(owner) })
            || unsafe { (self.class)(method) } != owner
            || arguments.len() > 4
        {
            return Err("scalar icall owner/arity mismatch".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.signature)(method) };
        if sig.is_null()
            || flags & 0x10 == 0
            || flags & (0x400 | 0x2000) != 0
            || implementation & 0x1000 == 0
            || unsafe { (self.instance)(sig) } != 0
            || unsafe { (self.count)(sig) } as usize != arguments.len()
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
        {
            return Err("scalar icall is not a concrete static InternalCall".into());
        }
        let mut iterator = ptr::null_mut();
        for (index, expected) in std::iter::once(returns)
            .chain(arguments.iter().copied())
            .enumerate()
        {
            let ty = unsafe {
                if index == 0 {
                    (self.returns)(sig)
                } else {
                    (self.parameters)(sig, &mut iterator)
                }
            };
            if ty.is_null()
                || expected.is_null()
                || unsafe { (self.byref)(ty) } != 0
                || unsafe { (self.type_class)(ty) } != expected
            {
                return Err("scalar icall exact type mismatch".into());
            }
            if index == 0 && unsafe { (self.kind)(ty) } == 1 {
                continue;
            }
            if marker && index == 1 && unsafe { (self.kind)(ty) } == 0x18 {
                let mut alignment = 0;
                if unsafe { (self.size)(expected, &mut alignment) } != 8 || alignment != 8 {
                    return Err("marker IntPtr storage is not 64-bit".into());
                }
                continue;
            }
            if !matches!(self.shape(ty, images)?, Shape::Scalar { .. }) {
                return Err("scalar icall requires primitive/enum values".into());
            }
        }
        Ok(())
    }
    /// Fixed UnsafeUtility allocator ABI only. Raw pointers are not admitted
    /// to ordinary managed calls or generic field/array payload publication.
    /// Classes are Void, Int64, Int32, Allocator and IntPtr respectively.
    pub(super) fn allocator_icall(
        self,
        method: Object,
        owner: Object,
        free: bool,
        classes: &[Object; 5],
        images: &[Object],
    ) -> Result<(), String> {
        let specs: &[(u32, Object)] = if free {
            &[(1, classes[0]), (0xf, classes[0]), (0x11, classes[3])]
        } else {
            &[
                (0xf, classes[0]),
                (0xa, classes[1]),
                (8, classes[2]),
                (0x11, classes[3]),
                (0x18, classes[4]),
            ]
        };
        self.fixed_buffer_icall(method, owner, specs, images)
    }
    pub(super) fn buffer_memset_icall(
        self,
        method: Object,
        owner: Object,
        classes: &[Object; 3],
        images: &[Object],
    ) -> Result<(), String> {
        self.fixed_buffer_icall(
            method,
            owner,
            &[
                (1, classes[0]),
                (0xf, classes[0]),
                (5, classes[1]),
                (0xa, classes[2]),
            ],
            images,
        )
    }
    pub(super) fn tracked_allocator_icall(
        self,
        method: Object,
        owner: Object,
        classes: &[Object; 5],
        images: &[Object],
    ) -> Result<(), String> {
        self.fixed_buffer_icall(
            method,
            owner,
            &[
                (0xf, classes[0]),
                (0xa, classes[1]),
                (8, classes[2]),
                (0x11, classes[3]),
                (8, classes[2]),
                (0x18, classes[4]),
            ],
            images,
        )
    }
    /// Typed memory operations share one owned-span protocol. These are public
    /// API signatures, verified against runtime metadata, not game method IDs.
    /// Classes are Void, Int64 and Int32 respectively.
    pub(super) fn buffer_memory_icall(
        self,
        method: Object,
        owner: Object,
        operation: BufferOperation,
        classes: &[Object; 3],
        images: &[Object],
    ) -> Result<(), String> {
        let specs: &[(u32, Object)] = match operation {
            BufferOperation::Copy => &[
                (1, classes[0]),
                (0xf, classes[0]),
                (0xf, classes[0]),
                (0xa, classes[1]),
            ],
            BufferOperation::Compare => &[
                (8, classes[2]),
                (0xf, classes[0]),
                (0xf, classes[0]),
                (0xa, classes[1]),
            ],
            BufferOperation::Replicate => &[
                (1, classes[0]),
                (0xf, classes[0]),
                (0xf, classes[0]),
                (8, classes[2]),
                (8, classes[2]),
            ],
            BufferOperation::Stride => &[
                (1, classes[0]),
                (0xf, classes[0]),
                (8, classes[2]),
                (0xf, classes[0]),
                (8, classes[2]),
                (8, classes[2]),
                (8, classes[2]),
            ],
        };
        self.fixed_buffer_icall(method, owner, specs, images)
    }
    fn fixed_buffer_icall(
        self,
        method: Object,
        owner: Object,
        specs: &[(u32, Object)],
        images: &[Object],
    ) -> Result<(), String> {
        if method.is_null()
            || owner.is_null()
            || !images.contains(&unsafe { (self.image)(owner) })
            || unsafe { (self.class)(method) } != owner
            || specs.iter().any(|(_, p)| p.is_null())
        {
            return Err("allocator icall owner/type identity mismatch".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.signature)(method) };
        if sig.is_null()
            || flags & 0x10 == 0
            || flags & (0x400 | 0x2000) != 0
            || implementation & 0x1000 == 0
            || unsafe { (self.instance)(sig) } != 0
            || unsafe { (self.count)(sig) } as usize != specs.len() - 1
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
        {
            return Err("allocator icall is not an exact static InternalCall".into());
        }
        let mut iterator = ptr::null_mut();
        for (index, &(kind, expected)) in specs.iter().enumerate() {
            let ty = unsafe {
                if index == 0 {
                    (self.returns)(sig)
                } else {
                    (self.parameters)(sig, &mut iterator)
                }
            };
            if ty.is_null()
                || unsafe { (self.byref)(ty) } != 0
                || unsafe { (self.kind)(ty) } != kind
            {
                return Err("allocator icall type kind/byref mismatch".into());
            }
            let element = if kind == 0xf {
                unsafe { (self.pointer_type)(ty) }
            } else {
                ty
            };
            if element.is_null()
                || unsafe { (self.type_class)(element) } != expected
                || (kind == 0xf
                    && (unsafe { (self.kind)(element) } != 1
                        || unsafe { (self.byref)(element) } != 0))
            {
                return Err("allocator icall exact class/pointer element mismatch".into());
            }
            if kind == 1 || kind == 0xf {
                continue;
            }
            let mut alignment = 0;
            let width = match kind {
                5 => 1,
                8 | 0x11 => 4,
                _ => 8,
            };
            if unsafe { (self.size)(expected, &mut alignment) } != width
                || alignment != width as u32
                || (kind == 0x11
                    && (unsafe { (self.is_enum)(expected) } == 0
                        || unsafe { (self.kind)((self.enum_base)(expected)) } != 8))
            {
                return Err("allocator scalar/enum storage mismatch".into());
            }
        }
        Ok(())
    }
    /// Exact metadata for fixed, precompiled injected/pointer adapters only.
    /// This does not admit these shapes to ordinary managed-call payloads.
    pub(super) fn fixed_typed_icall(
        self,
        method: Object,
        owner: Object,
        specs: &[(u32, Object, bool)],
        images: &[Object],
    ) -> Result<(), String> {
        self.fixed_typed_icall_enum_storage(method, owner, specs, images, &[])
    }
    /// A compiled adapter may explicitly declare a different integer enum ABI.
    /// Existing adapters keep their Int32 contract; overrides name exact classes.
    pub(super) fn fixed_typed_icall_enum_storage(
        self,
        method: Object,
        owner: Object,
        specs: &[(u32, Object, bool)],
        images: &[Object],
        enum_storage: &[(Object, u32)],
    ) -> Result<(), String> {
        self.fixed_typed_entry(
            method,
            owner,
            specs,
            images,
            enum_storage,
            EntryAbi::StaticInternal,
        )
    }
    pub(super) fn fixed_typed_instance_icall_enum_storage(
        self,
        method: Object,
        owner: Object,
        specs: &[(u32, Object, bool)],
        images: &[Object],
        enum_storage: &[(Object, u32)],
    ) -> Result<(), String> {
        if !super::render_context::context_class(owner) && !super::keyword_views::space(owner) {
            return Err("instance engine value ABI has no typed capability protocol".into());
        }
        self.fixed_typed_entry(
            method,
            owner,
            specs,
            images,
            enum_storage,
            EntryAbi::InstanceEngineValue,
        )
    }
    pub(super) fn fixed_typed_managed(
        self,
        method: Object,
        owner: Object,
        specs: &[(u32, Object, bool)],
        images: &[Object],
    ) -> Result<(), String> {
        self.fixed_typed_entry(method, owner, specs, images, &[], EntryAbi::StaticManaged)?;
        let header = unsafe { (self.header)(method) };
        if header.is_null() {
            return Err("fixed managed entry has no IL header".into());
        }
        let mut size = 0;
        let mut stack = 0;
        let code = unsafe { (self.code)(header, &mut size, &mut stack) };
        unsafe { (self.free)(header) };
        if code.is_null() || size == 0 {
            return Err("fixed managed entry has no IL".into());
        }
        Ok(())
    }
    /// An owned-pointer marshaller may expose only the actual verified m_Ptr
    /// field. Complete IL boundaries prevent an operand from mimicking LDFLD.
    pub(super) fn pointer_field_converter(
        self,
        method: Object,
        field: Object,
    ) -> Result<(), String> {
        let token = unsafe { libc::dlsym(self.library, c"mono_class_get_field_token".as_ptr()) };
        if token.is_null() || method.is_null() || field.is_null() {
            return Err("owned-pointer converter lacks actual field metadata".into());
        }
        let token: unsafe extern "C" fn(Object) -> u32 = unsafe { std::mem::transmute(token) };
        let token = unsafe { token(field) };
        let header = unsafe { (self.header)(method) };
        if header.is_null() {
            return Err("owned-pointer converter has no IL".into());
        }
        let (mut length, mut stack) = (0, 0);
        let code = unsafe { (self.code)(header, &mut length, &mut stack) };
        let valid = !code.is_null()
            && (7..=32).contains(&length)
            && stack <= 8
            && pointer_field_body(
                unsafe { std::slice::from_raw_parts(code, length as usize) },
                token,
            );
        unsafe { (self.free)(header) };
        if !valid {
            return Err(
                "owned-pointer converter does not read only the actual pointer field".into(),
            );
        }
        Ok(())
    }
    fn fixed_typed_entry(
        self,
        method: Object,
        owner: Object,
        specs: &[(u32, Object, bool)],
        images: &[Object],
        enum_storage: &[(Object, u32)],
        abi: EntryAbi,
    ) -> Result<(), String> {
        let internal = !matches!(abi, EntryAbi::StaticManaged);
        let instance = matches!(abi, EntryAbi::InstanceEngineValue);
        if method.is_null()
            || owner.is_null()
            || specs.is_empty()
            || specs.len() > 17
            || !images.contains(&unsafe { (self.image)(owner) })
            || unsafe { (self.class)(method) } != owner
        {
            return Err("fixed typed icall owner/arity mismatch".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.signature)(method) };
        if sig.is_null()
            || (flags & 0x10 == 0) != instance
            || flags & (0x400 | 0x2000) != 0
            || if internal {
                implementation & 0x1000 == 0
            } else {
                !managed_il_flags(flags, implementation, false)
            }
            || (unsafe { (self.instance)(sig) } != 0) != instance
            || unsafe { (self.count)(sig) } as usize != specs.len() - 1
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
        {
            return Err("fixed typed entry is not a concrete static InternalCall".into());
        }
        let mut iterator = ptr::null_mut();
        for (index, &(kind, expected, byref)) in specs.iter().enumerate() {
            let ty = unsafe {
                if index == 0 {
                    (self.returns)(sig)
                } else {
                    (self.parameters)(sig, &mut iterator)
                }
            };
            if ty.is_null()
                || expected.is_null()
                || unsafe { (self.kind)(ty) } != kind
                || (unsafe { (self.byref)(ty) } != 0) != byref
            {
                return Err("fixed typed icall kind/byref mismatch".into());
            }
            let element = if kind == 0xf {
                unsafe { (self.pointer_type)(ty) }
            } else {
                ty
            };
            if element.is_null()
                || unsafe { (self.type_class)(element) } != expected
                || !images.contains(&unsafe { (self.image)(expected) })
            {
                return Err("fixed typed icall exact owned type mismatch".into());
            }
            if kind == 0x11 && !byref {
                let base_kind = enum_storage
                    .iter()
                    .find(|(class, _)| *class == expected)
                    .map_or(8, |(_, kind)| *kind);
                let width = super::value_layout::enum_width(base_kind)
                    .ok_or("fixed typed enum storage must be integer")?;
                if unsafe { (self.is_enum)(expected) } == 0 {
                    return Err("fixed typed value requires an enum".into());
                }
                let mut alignment = 0;
                let base = unsafe { (self.enum_base)(expected) };
                if base.is_null()
                    || unsafe { (self.kind)(base) } != base_kind
                    || unsafe { (self.size)(expected, &mut alignment) } != width as i32
                    || alignment != width as u32
                {
                    return Err("fixed typed enum does not match compiled integer storage".into());
                }
            }
        }
        Ok(())
    }
    pub(super) fn json_from_icall(
        self,
        method: Object,
        owner: Object,
        span: Object,
        object: Object,
        ty: Object,
    ) -> Result<(), String> {
        if method.is_null() || owner.is_null() || unsafe { (self.class)(method) } != owner {
            return Err("JSON icall owner mismatch".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.signature)(method) };
        if sig.is_null()
            || flags & 0x10 == 0
            || flags & (0x400 | 0x2000) != 0
            || implementation & 0x1000 == 0
            || unsafe { (self.instance)(sig) } != 0
            || unsafe { (self.count)(sig) } != 3
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
        {
            return Err("JSON entry is not a concrete static InternalCall".into());
        }
        let mut iterator = ptr::null_mut();
        for (actual, expected, kind, byref) in [
            (unsafe { (self.returns)(sig) }, object, 0x1c, false),
            (
                unsafe { (self.parameters)(sig, &mut iterator) },
                span,
                0x11,
                true,
            ),
            (
                unsafe { (self.parameters)(sig, &mut iterator) },
                object,
                0x1c,
                false,
            ),
            (
                unsafe { (self.parameters)(sig, &mut iterator) },
                ty,
                0x12,
                false,
            ),
        ] {
            if actual.is_null()
                || expected.is_null()
                || unsafe { (self.kind)(actual) } != kind
                || (unsafe { (self.byref)(actual) } != 0) != byref
                || unsafe { (self.type_class)(actual) } != expected
            {
                return Err("JSON span/Object/Type ABI mismatch".into());
            }
        }
        Ok(())
    }
    pub(super) fn receiver(
        self,
        native: Object,
        declared: Object,
        images: &[Object],
    ) -> Result<Object, String> {
        if native.is_null() || declared.is_null() {
            return Err("managed instance receiver is NULL".into());
        }
        let actual = unsafe { (self.object_class)(native) };
        if actual.is_null()
            || !images.contains(&unsafe { (self.image)(actual) })
            || unsafe { (self.value)(actual) } != 0
            || unsafe { (self.assignable)(declared, actual) } == 0
        {
            return Err("managed receiver is not an owned assignable reference class".into());
        }
        self.closed.concrete(actual)?;
        Ok(actual)
    }
    pub(super) fn compatible(self, native: Object, shape: Shape) -> Result<Object, String> {
        let Shape::Reference { class, .. } = shape else {
            return Err("managed reference supplied to scalar".into());
        };
        if !native.is_null()
            && unsafe { (self.assignable)(class, (self.object_class)(native)) } == 0
        {
            return Err("managed argument reference is not assignable".into());
        }
        Ok(native)
    }
    pub(super) fn scalar_box(self, native: Object, shape: Shape) -> Result<Object, String> {
        let Shape::Scalar { class, width, .. } = shape else {
            return Err("managed return is not scalar box".into());
        };
        if native.is_null() || unsafe { (self.object_class)(native) } != class {
            return Err("managed scalar result lacks exact boxed class".into());
        }
        let payload = unsafe { (self.unbox)(native) };
        if payload != unsafe { native.cast::<u8>().add(16).cast() } {
            return Err("managed scalar box payload ABI mismatch".into());
        }
        let mut bytes = [0u8; 8];
        unsafe { ptr::copy_nonoverlapping(payload.cast(), bytes.as_mut_ptr(), width) };
        shape.validate(u64::from_le_bytes(bytes))?;
        Ok(class)
    }
}
fn concrete_method(generic: bool, inflated: bool, instance: bool, constructed: bool) -> bool {
    !generic
        && if constructed {
            inflated && instance
        } else {
            !inflated
        }
}
fn engine_handle_tail(code: &[u8], target: u32) -> Option<u32> {
    let instructions = engine_wrapper_instructions(code)?;
    match instructions.as_slice() {
        [.., (0x28, called), (0x28, converter), (0x2a, _)] if *called == target => Some(*converter),
        _ => None,
    }
}
/// A generated string wrapper unmarshals its saved native result in a finally
/// handler before releasing the pin. Check actual EH ranges and local origins;
/// merely finding a converter call elsewhere in the method is insufficient.
fn engine_handle_finally_tail(code: &[u8], target: u32, clause: ExceptionClause) -> Option<u32> {
    let instructions = engine_wrapper_instructions(code)?;
    if clause.flags != 2
        || instructions
            .iter()
            .filter(|&&(op, t)| op == 0x28 && t == target)
            .count()
            != 1
    {
        return None;
    }
    let local = |instruction: (u8, u32), store| match instruction {
        (op @ 0x0a..=0x0d, _) if store => Some((op - 0x0a) as u32),
        (0x13, n) if store => Some(n),
        (op @ 0x06..=0x09, _) if !store => Some((op - 0x06) as u32),
        (0x11, n) if !store => Some(n),
        _ => None,
    };
    let start = instructions.iter().position(|&i| i == (0x28, target))?;
    let tail = instructions.get(start..)?;
    if tail.len() != 12 {
        return None;
    }
    let mut offsets = Vec::new();
    let mut at = 0;
    for &(op, _) in &instructions {
        offsets.push(at);
        at += 1 + engine_wrapper_operand_width(op)?;
    }
    let native = local(tail[1], true)?;
    let managed = local(tail[5], true)?;
    let pin = local(tail[8], true)?;
    let (leave, delta) = tail[2];
    let delta = match leave {
        0xde => delta as u8 as i8 as i64,
        0xdd => delta as i32 as i64,
        _ => return None,
    };
    let converter = match tail[4] {
        (0x28, token) if token >> 24 == 0x2b => token,
        _ => return None,
    };
    if native == managed
        || native == pin
        || managed == pin
        || local(tail[3], false)? != native
        || local(tail[10], false)? != managed
        || tail[6] != (0x16, 0)
        || tail[7] != (0xe0, 0)
        || tail[9] != (0xdc, 0)
        || tail[11] != (0x2a, 0)
        || clause.handler_offset as usize != offsets[start + 3]
        || clause.handler_offset.checked_add(clause.handler_len)? as usize != offsets[start + 10]
        || clause.try_offset as usize > offsets[start]
        || clause.try_offset.checked_add(clause.try_len)? as usize != offsets[start + 3]
        || offsets[start + 3] as i64 + delta != offsets[start + 10] as i64
    {
        return None;
    }
    Some(converter)
}
fn text_matches(text: *const c_char, expected: &CStr) -> bool {
    !text.is_null() && unsafe { CStr::from_ptr(text) } == expected
}
fn engine_owned_output_tail(
    code: &[u8],
    target: u32,
    clause: ExceptionClause,
) -> Option<(u32, u32, u32)> {
    let instructions = engine_wrapper_instructions(code)?;
    if clause.flags != 2
        || instructions
            .iter()
            .filter(|&&(op, t)| op == 0x28 && t == target)
            .count()
            != 1
    {
        return None;
    }
    let at = instructions.iter().position(|&i| i == (0x28, target))?;
    let &(0x12, native) = instructions.get(at.checked_sub(1)?)? else {
        return None;
    };
    let tail = instructions.get(at..)?;
    if tail.len() != 8 {
        return None;
    }
    let load = |instruction: (u8, u32)| match instruction {
        (op @ 0x06..=0x09, _) => Some((op - 6) as u32),
        (0x11, n) => Some(n),
        _ => None,
    };
    let result = match tail[4] {
        (op @ 0x0a..=0x0d, _) => (op - 0x0a) as u32,
        (0x13, n) => n,
        _ => return None,
    };
    let converter = match tail[3] {
        (0x28, t) if matches!(t >> 24, 6 | 0xa) => t,
        _ => return None,
    };
    let delta = match tail[1] {
        (0xde, n) => n as u8 as i8 as i64,
        (0xdd, n) => n as i32 as i64,
        _ => return None,
    };
    let mut offsets = Vec::new();
    let mut offset = 0;
    for &(op, _) in &instructions {
        offsets.push(offset);
        offset += 1 + engine_wrapper_operand_width(op)?;
    }
    if native == result
        || load(tail[2]) != Some(native)
        || load(tail[6]) != Some(result)
        || tail[5] != (0xdc, 0)
        || tail[7] != (0x2a, 0)
        || clause.handler_offset as usize != offsets[at + 2]
        || clause.handler_offset.checked_add(clause.handler_len)? as usize != offsets[at + 6]
        || clause.try_offset as usize > offsets[at - 1]
        || clause.try_offset.checked_add(clause.try_len)? as usize != offsets[at + 2]
        || offsets[at + 2] as i64 + delta != offsets[at + 6] as i64
    {
        return None;
    }
    Some((native, result, converter))
}
fn engine_wrapper_operand_width(opcode: u8) -> Option<usize> {
    Some(match opcode {
        0x00 | 0x02..=0x0d | 0x14..=0x1e | 0x25 | 0x26 | 0x2a | 0xd3 | 0xdc | 0xe0 => 0,
        0x0e..=0x13 | 0x1f | 0x2b..=0x37 | 0xde => 1,
        0x20 | 0x22 | 0x28 | 0x38..=0x44 | 0x6f | 0x72 | 0x7b..=0x80 | 0xdd => 4,
        0x21 | 0x23 => 8,
        _ => return None,
    })
}
fn engine_wrapper_instructions(code: &[u8]) -> Option<Vec<(u8, u32)>> {
    // Parse complete instruction boundaries in the small generated wrapper
    // vocabulary. Unknown opcodes defer; operands cannot impersonate CALLs.
    let mut at = 0;
    let mut instructions = Vec::new();
    while at < code.len() {
        let opcode = code[at];
        at += 1;
        let width = engine_wrapper_operand_width(opcode)?;
        let operand = code.get(at..at.checked_add(width)?)?;
        let mut word = [0; 4];
        let copied = operand.len().min(4);
        word[..copied].copy_from_slice(&operand[..copied]);
        instructions.push((opcode, u32::from_le_bytes(word)));
        at += width;
    }
    Some(instructions)
}
fn pointer_field_body(code: &[u8], field: u32) -> bool {
    if field >> 24 != 4 {
        return false;
    }
    let Some(instructions) = engine_wrapper_instructions(code) else {
        return false;
    };
    let instructions: Vec<_> = instructions
        .into_iter()
        .filter(|&(op, _)| op != 0)
        .collect();
    instructions == [(0x02, 0), (0x7b, field), (0x2a, 0)]
}
fn engine_pointer_prefix<T: Copy + Eq>(
    code: &[u8],
    target: u32,
    arity: usize,
    mut marshal: impl FnMut(usize, u32) -> Option<T>,
) -> Option<Vec<Option<T>>> {
    let instructions = engine_wrapper_instructions(code)?;
    if arity > 16
        || instructions
            .iter()
            .filter(|&&(op, token)| op == 0x28 && token == target)
            .count()
            != 1
    {
        return None;
    }
    let mut at = 0;
    let mut arguments = Vec::new();
    for position in 0..arity {
        while instructions.get(at).is_some_and(|&(op, _)| op == 0) {
            at += 1;
        }
        let &(op, operand) = instructions.get(at)?;
        let (index, address) = match op {
            0x02..=0x05 => ((op - 2) as usize, false),
            0x0e => (operand as usize, false),
            0x0f => (operand as usize, true),
            _ => return None,
        };
        if index != position {
            return None;
        }
        at += 1;
        let pointer = if let Some(&(0x28, converter)) = instructions.get(at) {
            if converter != target {
                if address || !matches!(converter >> 24, 6 | 0xa | 0x2b) {
                    return None;
                }
                let nullable = marshal(position, converter)?;
                at += 1;
                Some(nullable)
            } else {
                None
            }
        } else {
            None
        };
        arguments.push(pointer);
    }
    while instructions.get(at).is_some_and(|&(op, _)| op == 0) {
        at += 1;
    }
    (instructions.get(at) == Some(&(0x28, target))).then_some(arguments)
}
/// Generated nullable marshalling keeps argument order and has exactly two
/// paths: the verified converter or literal native NULL. Validate both branch
/// targets and their complete bodies; unrelated joins retain a missing entry.
fn engine_pointer_nullable_prefix<T: Copy + Eq>(
    code: &[u8],
    target: u32,
    arity: usize,
    mut marshal: impl FnMut(usize, u32) -> Option<T>,
    mut nullable: impl FnMut(T) -> T,
) -> Option<Vec<Option<T>>> {
    let instructions = engine_wrapper_instructions(code)?;
    if arity > 16
        || instructions
            .iter()
            .filter(|&&(op, token)| op == 0x28 && token == target)
            .count()
            != 1
    {
        return None;
    }
    let mut offsets = Vec::new();
    let mut offset = 0;
    for &(op, _) in &instructions {
        offsets.push(offset);
        offset += 1 + engine_wrapper_operand_width(op)?;
    }
    let branch = |at: usize| -> Option<usize> {
        let &(op, word) = instructions.get(at)?;
        let delta = match op {
            0x2b | 0x2c => word as u8 as i8 as i64,
            0x38 | 0x39 => word as i32 as i64,
            _ => return None,
        };
        let end = offsets[at] + 1 + engine_wrapper_operand_width(op)?;
        offsets
            .binary_search(&usize::try_from(end as i64 + delta).ok()?)
            .ok()
    };
    let skip_nops = |at: &mut usize| {
        while instructions.get(*at).is_some_and(|&(op, _)| op == 0) {
            *at += 1;
        }
    };
    let argument = |at: usize, position: usize| -> Option<bool> {
        match *instructions.get(at)? {
            (op @ 0x02..=0x05, _) if (op - 2) as usize == position => Some(false),
            (0x0e, index) if index as usize == position => Some(false),
            (0x0f, index) if index as usize == position => Some(true),
            _ => None,
        }
    };
    let mut at = 0;
    let mut result = Vec::new();
    for position in 0..arity {
        skip_nops(&mut at);
        let address = argument(at, position)?;
        at += 1;
        let source = if instructions
            .get(at)
            .is_some_and(|&(op, _)| matches!(op, 0x2c | 0x39))
        {
            if address {
                return None;
            }
            let null_start = branch(at)?;
            at += 1;
            skip_nops(&mut at);
            if argument(at, position)? {
                return None;
            }
            at += 1;
            let &(0x28, converter) = instructions.get(at)? else {
                return None;
            };
            if converter == target {
                return None;
            }
            let source = marshal(position, converter)?;
            at += 1;
            if !instructions
                .get(at)
                .is_some_and(|&(op, _)| matches!(op, 0x2b | 0x38))
            {
                return None;
            }
            let join = branch(at)?;
            at += 1;
            if at != null_start {
                return None;
            }
            skip_nops(&mut at);
            if instructions.get(at..at + 2)? != [(0x16, 0), (0xd3, 0)] {
                return None;
            }
            at += 2;
            if at != join {
                return None;
            }
            Some(nullable(source))
        } else if let Some(&(0x28, converter)) = instructions.get(at) {
            if converter != target {
                if address {
                    return None;
                }
                at += 1;
                Some(marshal(position, converter)?)
            } else {
                None
            }
        } else {
            None
        };
        result.push(source);
    }
    skip_nops(&mut at);
    (instructions.get(at) == Some(&(0x28, target))).then_some(result)
}
/// Track argument origins across acyclic generated null guards. Every reachable
/// incoming stack at a join must agree. Only verified Marshal conversions and
/// the actual throwing helper are calls in this vocabulary; no arbitrary calls,
/// locals, field writes or branch operands can manufacture pointer provenance.
#[cfg(test)]
fn engine_pointer_flow<T: Copy + Eq>(
    code: &[u8],
    target: u32,
    arity: usize,
    marshal: impl FnMut(usize, u32) -> Option<T>,
    throwing: impl FnMut(u32) -> bool,
) -> Option<Vec<Option<T>>> {
    engine_pointer_flow_with_calls(code, target, arity, marshal, throwing, |_| None)
}
fn engine_pointer_flow_with_calls<T: Copy + Eq>(
    code: &[u8],
    target: u32,
    arity: usize,
    marshal: impl FnMut(usize, u32) -> Option<T>,
    throwing: impl FnMut(u32) -> bool,
    call_effect: impl FnMut(u32) -> Option<(usize, bool)>,
) -> Option<Vec<Option<T>>> {
    engine_argument_flow(
        code,
        target,
        arity,
        marshal,
        throwing,
        call_effect,
        |_, _| None,
    )
}
fn engine_argument_flow<T: Copy + Eq>(
    code: &[u8],
    target: u32,
    arity: usize,
    mut marshal: impl FnMut(usize, u32) -> Option<T>,
    mut throwing: impl FnMut(u32) -> bool,
    mut call_effect: impl FnMut(u32) -> Option<(usize, bool)>,
    mut local_address: impl FnMut(usize, u32) -> Option<(usize, T)>,
) -> Option<Vec<Option<T>>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Origin<T> {
        Argument(usize, bool),
        Pointer(usize, T),
        Local(usize, T),
        Literal,
    }
    let instructions = engine_wrapper_instructions(code)?;
    if arity > 16
        || instructions.len() > 256
        || instructions
            .iter()
            .filter(|&&(op, token)| op == 0x28 && token == target)
            .count()
            != 1
    {
        return None;
    }
    let mut offsets = Vec::new();
    let mut ends = Vec::new();
    let mut offset = 0;
    for &(op, _) in &instructions {
        offsets.push(offset);
        offset += 1 + engine_wrapper_operand_width(op)?;
        ends.push(offset);
    }
    let mut states: Vec<Option<Vec<Origin<T>>>> = vec![None; instructions.len()];
    let mut pending = vec![(0, Vec::new())];
    let mut result = None;
    while let Some((at, mut stack)) = pending.pop() {
        let old = states.get_mut(at)?;
        if let Some(prior) = old {
            if *prior != stack {
                return None;
            }
            continue;
        }
        *old = Some(stack.clone());
        let (op, operand) = instructions[at];
        let mut branch = None;
        let mut fallthrough = true;
        match op {
            0 => {}
            0x02..=0x05 => {
                let index = (op - 2) as usize;
                if index >= arity {
                    return None;
                }
                stack.push(Origin::Argument(index, false));
            }
            0x0e | 0x0f => {
                if operand as usize >= arity {
                    return None;
                }
                stack.push(Origin::Argument(operand as usize, op == 0x0f));
            }
            0x12 => stack.push(match local_address(at, operand) {
                Some((position, value)) => Origin::Local(position, value),
                None => Origin::Literal,
            }),
            0x06..=0x09 | 0x11 => stack.push(Origin::Literal),
            0x0a..=0x0d | 0x13 => {
                stack.pop()?;
            }
            0xd3 | 0xe0 => {
                if matches!(stack.pop()?, Origin::Pointer(..) | Origin::Local(..)) {
                    return None;
                }
                stack.push(Origin::Literal);
            }
            0x14..=0x23 | 0x72 => stack.push(Origin::Literal),
            0x25 => stack.push(*stack.last()?),
            0x26 => {
                stack.pop()?;
            }
            0x28 if operand == target => {
                if stack.len() != arity {
                    return None;
                }
                let origins: Vec<_> = stack
                    .into_iter()
                    .enumerate()
                    .map(|(position, origin)| match origin {
                        Origin::Argument(index, _) if index == position => Some(None),
                        Origin::Pointer(index, nullable) | Origin::Local(index, nullable)
                            if index == position =>
                        {
                            Some(Some(nullable))
                        }
                        Origin::Literal => Some(None),
                        _ => None,
                    })
                    .collect::<Option<_>>()?;
                if result.as_ref().is_some_and(|prior| prior != &origins) {
                    return None;
                }
                result = Some(origins);
                continue;
            }
            0x28 if throwing(operand) => {
                if stack.pop()? != Origin::Literal {
                    return None;
                }
                match stack.pop()? {
                    Origin::Argument(_, false) | Origin::Pointer(_, _) => {}
                    _ => return None,
                }
                continue;
            }
            0x28 | 0x6f => {
                if !matches!(operand >> 24, 6 | 0xa | 0x2b) {
                    return None;
                }
                let converted = match stack.last().copied() {
                    Some(Origin::Argument(position, false)) if position < arity && op == 0x28 => {
                        marshal(position, operand).map(|source| (position, source))
                    }
                    _ => None,
                };
                if let Some((position, source)) = converted {
                    stack.pop()?;
                    stack.push(Origin::Pointer(position, source));
                } else {
                    let (count, returns) = call_effect(operand)?;
                    let remaining = stack.len().checked_sub(count)?;
                    if stack[remaining..]
                        .iter()
                        .any(|value| matches!(value, Origin::Local(..)))
                    {
                        return None;
                    }
                    stack.truncate(remaining);
                    if returns {
                        stack.push(Origin::Literal);
                    }
                }
            }
            0x2b..=0x2d | 0x38..=0x3a => {
                let delta = if op < 0x38 {
                    operand as u8 as i8 as i64
                } else {
                    operand as i32 as i64
                };
                let address = ends[at] as i64 + delta;
                let next = offsets
                    .binary_search(&usize::try_from(address).ok()?)
                    .ok()?;
                if next <= at {
                    return None;
                }
                branch = Some(next);
                if matches!(op, 0x2b | 0x38) {
                    fallthrough = false;
                } else {
                    stack.pop()?;
                }
            }
            0x2a => continue,
            _ => return None,
        }
        if stack.len() > 16 {
            return None;
        }
        if let Some(next) = branch {
            pending.push((next, stack.clone()));
        }
        if fallthrough {
            pending.push((at + 1, stack));
        }
    }
    result
}
#[cfg(test)]
fn engine_utf16_locals(
    code: &[u8],
    target: u32,
    arity: usize,
    span_positions: &[usize],
    is_string_helper: impl FnMut(usize, u32) -> bool,
) -> Option<Vec<bool>> {
    engine_utf16_locals_with_output(code, target, arity, span_positions, false, is_string_helper)
}
fn engine_utf16_locals_with_output(
    code: &[u8],
    target: u32,
    arity: usize,
    span_positions: &[usize],
    trailing_output: bool,
    mut is_string_helper: impl FnMut(usize, u32) -> bool,
) -> Option<Vec<bool>> {
    let instructions = engine_wrapper_instructions(code)?;
    if arity > 16 || span_positions.is_empty() || span_positions.iter().any(|&p| p >= arity) {
        return None;
    }
    let calls: Vec<_> = instructions
        .iter()
        .enumerate()
        .filter_map(|(i, &(op, called))| (op == 0x28 && called == target).then_some(i))
        .collect();
    if calls.len() != 1 {
        return None;
    }
    let call = calls[0];
    let argument_index = |op: u8, operand: u32| match op {
        0x02..=0x05 => Some((op - 2) as usize),
        0x0e => Some(operand as usize),
        _ => None,
    };
    let mut locals = std::collections::BTreeMap::new();
    for i in 2..call {
        let (op, called) = instructions[i];
        if op != 0x28 {
            continue;
        }
        let (op, argument) = instructions[i - 2];
        let (address, local) = instructions[i - 1];
        if address != 0x12 {
            continue;
        }
        let Some(argument) = argument_index(op, argument) else {
            continue;
        };
        if !is_string_helper(argument, called) {
            continue;
        }
        if locals.insert(local, argument).is_some() {
            return None;
        }
    }
    // A by-value reassignment of a header breaks the established origin.
    if instructions[..call].iter().any(|&(op, operand)| {
        let local = match op {
            0x0a..=0x0d => Some((op - 0x0a) as u32),
            0x13 => Some(operand),
            _ => None,
        };
        local.is_some_and(|local| locals.contains_key(&local))
    }) {
        return None;
    }
    let first = *span_positions.iter().min()?;
    let mut at = call;
    let mut result = vec![false; arity];
    for position in (first..arity).rev() {
        while at > 0 && instructions[at - 1].0 == 0 {
            at -= 1;
        }
        at = at.checked_sub(1)?;
        let (op, operand) = instructions[at];
        if span_positions.contains(&position) {
            if op != 0x12 || locals.get(&operand) != Some(&position) {
                return None;
            }
            result[position] = true;
        } else if argument_index(op, operand) == Some(position)
            || (op == 0x0f && operand as usize == position)
            || (trailing_output && position + 1 == arity && op == 0x12)
        {
            // Plain positional inputs after a header do not obscure its origin.
        } else {
            return None;
        }
    }
    Some(result)
}
fn managed_il_flags(flags: u32, implementation: u32, instance: bool) -> bool {
    (flags & 0x10 == 0) == instance
        && flags & (0x400 | 0x2000) == 0
        && implementation & (0x3 | 0x4 | 0x20 | 0x1000) == 0
}
fn delegate_invoke_flags(flags: u32, implementation: u32, instance: bool) -> bool {
    instance
        && flags & (0x10 | 0x400 | 0x2000) == 0
        && implementation & (0x3 | 0x4 | 0x20 | 0x1000) == 3
}
struct Slot {
    shape: Shape,
    guest: Option<u64>,
}
pub(super) struct Frame {
    // Inline native-stack storage: BDWGC must see managed references written
    // through byref arguments during IL execution and before canonical rooting.
    // Rust allocator memory is not a scanned Mono GC allocation.
    words: [u64; 16],
    slots: Vec<Slot>,
    arguments: Vec<(usize, bool)>,
    byte_output: Option<ByteOutput>,
    scalar_outputs: Vec<ScalarOutput>,
}
struct ScalarOutput {
    // Eight-byte aligned private cell with canaries on both sides. Only the
    // proven primitive width can be copied back after the complete invocation.
    buffer: Box<[u64; 3]>,
    address: u64,
    shape: Shape,
}
struct ByteOutput {
    buffer: Box<[u8; 16]>,
    address: u64,
    boolean: Object,
}
pub(super) struct Output {
    pub(super) address: u64,
    pub(super) shape: Shape,
    pub(super) word: u64,
}
impl Frame {
    pub(super) fn scalar_argument(&self, index: usize, shape: Shape) -> Result<u64, String> {
        let &(slot, indirect) = self
            .arguments
            .get(index)
            .ok_or("scoped capability argument absent")?;
        let storage = self
            .slots
            .get(slot)
            .ok_or("scoped capability storage absent")?;
        if !indirect
            || storage.guest.is_some()
            || storage.shape != shape
            || !matches!(shape, Shape::Scalar { .. })
        {
            return Err("scoped capability requires an exact scalar input frame".into());
        }
        Ok(self.words[slot])
    }
    pub(super) fn replace_scalar_argument(
        &mut self,
        index: usize,
        shape: Shape,
        word: u64,
    ) -> Result<(), String> {
        self.scalar_argument(index, shape)?;
        shape.validate(word)?;
        self.words[self.arguments[index].0] = word;
        Ok(())
    }
    pub(super) fn prepare(
        memory: &Memory,
        descriptor: &Descriptor,
        arguments: u64,
        mut resolve: impl FnMut(u64, Shape) -> Result<Object, String>,
        mut writable: impl FnMut(u64, usize) -> bool,
    ) -> Result<Self, String> {
        let count = descriptor.parameters.len();
        if count > 16 || (count != 0 && (arguments == 0 || !arguments.is_multiple_of(8))) {
            return Err("managed parameter vector outside bounded aligned storage".into());
        }
        if count != 0 {
            memory.check(arguments, count * 8, READ)?;
        }
        let mut frame = Self {
            words: [0; 16],
            slots: Vec::with_capacity(count),
            arguments: Vec::with_capacity(count),
            byte_output: None,
            scalar_outputs: Vec::new(),
        };
        for (index, parameter) in descriptor.parameters.iter().enumerate() {
            let argument = memory.read_int(arguments + index as u64 * 8, 8)?;
            let width = parameter.shape.width();
            let alignment = parameter.shape.alignment();
            if !matches!(width, 1 | 2 | 4 | 8)
                || !matches!(alignment, 1 | 2 | 4 | 8)
                || alignment > width
                || !width.is_multiple_of(alignment)
            {
                return Err("managed value storage size/alignment outside word ABI".into());
            }
            let reference = matches!(parameter.shape, Shape::Reference { .. });
            let word = if parameter.byref || !reference {
                if argument == 0 || !argument.is_multiple_of(alignment as u64) {
                    return Err("managed value argument address/alignment mismatch".into());
                }
                memory.read_int(argument, width)?
            } else {
                argument
            };
            parameter.shape.validate(word)?;
            let native = if reference {
                resolve(word, parameter.shape)? as u64
            } else {
                word
            };
            let guest = if parameter.byref {
                memory.check(argument, width, WRITE)?;
                if !writable(argument, width) {
                    return Err("managed byref output is not an owned C++ temporary slot".into());
                }
                let end = argument
                    .checked_add(width as u64)
                    .ok_or("managed output span overflow")?;
                let vector_end = arguments
                    .checked_add(count as u64 * 8)
                    .ok_or("managed argument vector overflow")?;
                if argument < vector_end && arguments < end {
                    return Err("managed output overlaps parameter vector".into());
                }
                Some(argument)
            } else {
                None
            };
            let mut existing = None;
            if let Some(address) = guest {
                for (index, slot) in frame.slots.iter().enumerate() {
                    if let Some(other) = slot.guest {
                        if address < other + slot.shape.width() as u64
                            && other < address + width as u64
                        {
                            if address != other || slot.shape != parameter.shape {
                                return Err(
                                    "managed byref arguments overlap with different shapes".into(),
                                );
                            }
                            existing = Some(index);
                        }
                    }
                }
            }
            let slot = existing.unwrap_or_else(|| {
                let index = frame.slots.len();
                frame.words[index] = native;
                frame.slots.push(Slot {
                    shape: parameter.shape,
                    guest,
                });
                index
            });
            frame.arguments.push((slot, parameter.byref || !reference));
        }
        Ok(frame)
    }
    pub(super) fn prepare_coroutine_output(
        &mut self,
        memory: &Memory,
        arguments: u64,
        boolean: Object,
        writable: impl FnOnce(u64, usize) -> bool,
    ) -> Result<(), String> {
        if self.byte_output.is_some()
            || boolean.is_null()
            || self.slots.len() != 2
            || self.arguments != [(0, false), (1, true)]
            || !matches!(
                self.slots[1].shape,
                Shape::Scalar {
                    width: 8,
                    boolean: false,
                    ..
                }
            )
        {
            return Err("coroutine output requires exact private IEnumerator/IntPtr frame".into());
        }
        let address = self.words[1];
        memory.check(address, 1, WRITE)?;
        let pointer_argument = memory.read_int(arguments + 8, 8)?;
        if address == 0
            || !writable(address, 1)
            || (address >= arguments && address - arguments < 16)
            || (address >= pointer_argument && address - pointer_argument < 8)
        {
            return Err("coroutine Boolean output is not a disjoint live temporary byte".into());
        }
        let mut buffer = Box::new([0xa5; 16]);
        buffer[0] = 0;
        self.words[1] = buffer.as_mut_ptr() as u64;
        self.byte_output = Some(ByteOutput {
            buffer,
            address,
            boolean,
        });
        Ok(())
    }
    pub(super) fn prepare_scalar_outputs(
        &mut self,
        memory: &Memory,
        arguments: u64,
        outputs: &[(usize, Shape)],
        mut writable: impl FnMut(u64, usize) -> bool,
    ) -> Result<(), String> {
        if self.byte_output.is_some() || !self.scalar_outputs.is_empty() || outputs.is_empty() {
            return Err("scalar output frame already prepared or empty".into());
        }
        let overlaps = |a: u64, n: usize, b: u64, m: usize| {
            a < b.saturating_add(m as u64) && b < a.saturating_add(n as u64)
        };
        let mut prepared: Vec<ScalarOutput> = Vec::new();
        let mut replacements = Vec::new();
        for &(parameter, shape) in outputs {
            let width = shape.width();
            if !matches!(shape, Shape::Scalar { .. }) || !matches!(width, 1 | 2 | 4 | 8) {
                return Err("scalar output shape exceeds primitive cell".into());
            }
            let &(slot, indirect) = self
                .arguments
                .get(parameter)
                .ok_or("scalar output parameter absent")?;
            if !indirect
                || self.slots[slot].guest.is_some()
                || !matches!(
                    self.slots[slot].shape,
                    Shape::Scalar {
                        width: 8,
                        boolean: false,
                        ..
                    }
                )
                || replacements.iter().any(|&(prior, _)| prior == slot)
            {
                return Err("scalar output input lacks a unique private IntPtr word".into());
            }
            let address = self.words[slot];
            memory.check(address, width, WRITE)?;
            if address == 0
                || !address.is_multiple_of(shape.alignment() as u64)
                || !writable(address, width)
                || overlaps(address, width, arguments, self.arguments.len() * 8)
            {
                return Err("scalar output is not disjoint live temporary storage".into());
            }
            // An output cannot overwrite its own or another argument's pointer
            // cell. Validate every scalar input, independent of argument order.
            for (index, &(input_slot, indirect)) in self.arguments.iter().enumerate() {
                if indirect {
                    let input = memory.read_int(arguments + index as u64 * 8, 8)?;
                    if overlaps(address, width, input, self.slots[input_slot].shape.width()) {
                        return Err("scalar output overlaps an argument input cell".into());
                    }
                }
            }
            if self.slots.iter().any(|other| {
                other
                    .guest
                    .is_some_and(|a| overlaps(address, width, a, other.shape.width()))
            }) {
                return Err("scalar output overlaps an ordinary byref cell".into());
            }
            let mut alias = None;
            for (index, prior) in prepared.iter().enumerate() {
                if overlaps(address, width, prior.address, prior.shape.width()) {
                    if address != prior.address || shape != prior.shape {
                        return Err("scalar output alias has different range/type".into());
                    }
                    alias = Some(index);
                }
            }
            let index = alias.unwrap_or_else(|| {
                let mut buffer = Box::new([0xa5a5_a5a5_a5a5_a5a5u64; 3]);
                let bytes = buffer[1].to_le_bytes();
                let mut initial = bytes;
                initial[..width].fill(0);
                buffer[1] = u64::from_le_bytes(initial);
                prepared.push(ScalarOutput {
                    buffer,
                    address,
                    shape,
                });
                prepared.len() - 1
            });
            replacements.push((slot, index));
        }
        for (slot, output) in replacements {
            self.words[slot] = (&mut prepared[output].buffer[1] as *mut u64) as u64;
        }
        self.scalar_outputs = prepared;
        Ok(())
    }
    pub(super) fn pointers(&mut self) -> Vec<Object> {
        self.arguments
            .iter()
            .map(|&(slot, address)| {
                if address {
                    (&mut self.words[slot] as *mut u64).cast()
                } else {
                    self.words[slot] as Object
                }
            })
            .collect()
    }
    pub(super) fn outputs(&self) -> Result<Vec<Output>, String> {
        let mut outputs = Vec::new();
        for output in &self.scalar_outputs {
            let width = output.shape.width();
            let bytes = output.buffer[1].to_le_bytes();
            if output.buffer[0] != 0xa5a5_a5a5_a5a5_a5a5
                || output.buffer[2] != 0xa5a5_a5a5_a5a5_a5a5
                || bytes[width..].iter().any(|&b| b != 0xa5)
            {
                return Err("scalar output exceeds private primitive cell/canary".into());
            }
            let mut word = [0; 8];
            word[..width].copy_from_slice(&bytes[..width]);
            let word = u64::from_le_bytes(word);
            output.shape.validate(word)?;
            outputs.push(Output {
                address: output.address,
                shape: output.shape,
                word,
            });
        }
        if let Some(output) = &self.byte_output {
            if output.buffer[1..].iter().any(|&value| value != 0xa5) || output.buffer[0] > 1 {
                return Err("coroutine output exceeds canonical Boolean byte/canary".into());
            }
            outputs.push(Output {
                address: output.address,
                shape: Shape::Scalar {
                    class: output.boolean,
                    width: 1,
                    alignment: 1,
                    boolean: true,
                },
                word: output.buffer[0] as u64,
            });
        }
        for (index, slot) in self.slots.iter().enumerate() {
            if let Some(address) = slot.guest {
                slot.shape.validate(self.words[index])?;
                outputs.push(Output {
                    address,
                    shape: slot.shape,
                    word: self.words[index],
                });
            }
        }
        Ok(outputs)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_wrappers_preserve_provenance_across_metadata_typed_calls_and_branch_joins() {
        let target: u32 = 0x06000010;
        let marshal: u32 = 0x2b000020;
        let helper: u32 = 0x06000030;
        let as_span: u32 = 0x0a000040;
        let pin: u32 = 0x0a000050;
        let length: u32 = 0x0a000060;
        let constructor: u32 = 0x06000070;
        let mut code = vec![0x02, 0x28];
        code.extend(marshal.to_le_bytes());
        code.extend([0x03, 0x12, 2, 0x28]);
        code.extend(helper.to_le_bytes());
        let branch = code.len();
        code.extend([0x2d, 0]);
        code.extend([0x03, 0x28]);
        code.extend(as_span.to_le_bytes());
        code.push(0x0a);
        code.extend([0x12, 0, 0x28]);
        code.extend(pin.to_le_bytes());
        code.push(0x0b);
        code.extend([0x12, 2, 0x07, 0xd3, 0x12, 0, 0x28]);
        code.extend(length.to_le_bytes());
        code.push(0x28);
        code.extend(constructor.to_le_bytes());
        code[branch + 1] = (code.len() - branch - 2) as u8;
        code.extend([0x12, 2, 0x28]);
        code.extend(target.to_le_bytes());
        code.push(0x2a);
        let effects = |t| {
            if t == helper {
                Some((2, true))
            } else if [as_span, pin, length, marshal].contains(&t) {
                Some((1, true))
            } else if t == constructor {
                Some((3, false))
            } else {
                None
            }
        };
        let source =
            |p, t| (p == 0 && t == marshal).then_some(PointerSource::Engine { nullable: true });
        assert_eq!(
            engine_pointer_flow_with_calls(&code, target, 2, source, |_| false, effects),
            Some(vec![Some(PointerSource::Engine { nullable: true }), None])
        );
        // An arbitrary IntPtr-returning method has stack effects but cannot
        // manufacture the provenance of an actual Marshal<T> conversion.
        assert_eq!(
            engine_pointer_flow_with_calls::<PointerSource>(
                &code,
                target,
                2,
                |_, _| None,
                |_| false,
                effects
            ),
            Some(vec![None, None])
        );
        assert_eq!(
            engine_pointer_flow(&code, target, 2, source, |_| false),
            None
        );
        assert_eq!(
            engine_pointer_flow_with_calls(
                &code,
                target,
                2,
                source,
                |_| false,
                |t| if t == helper {
                    Some((3, true))
                } else {
                    effects(t)
                }
            ),
            None
        );
        let mut wrong = code.clone();
        wrong[branch + 1] = (-2i8) as u8;
        assert_eq!(
            engine_pointer_flow_with_calls(&wrong, target, 2, source, |_| false, effects),
            None
        );
        assert_eq!(
            engine_utf16_locals(&code, target, 2, &[1], |p, t| p == 1 && t == helper),
            Some(vec![false, true])
        );
    }
    #[test]
    fn output_strings_require_the_same_span_local_and_actual_disposal_finally() {
        let target: u32 = 0x06000123;
        let converter: u32 = 0x06000456;
        let mut code = vec![0x02, 0x12, 0, 0x28];
        code.extend(target.to_le_bytes());
        code.extend([0xde, 8, 0x06, 0x28]);
        code.extend(converter.to_le_bytes());
        code.extend([0x0b, 0xdc, 0x07, 0x2a]);
        let clause = ExceptionClause {
            flags: 2,
            try_offset: 0,
            try_len: 10,
            handler_offset: 10,
            handler_len: 8,
            data: 0,
        };
        assert_eq!(
            engine_owned_output_tail(&code, target, clause),
            Some((0, 1, converter))
        );
        for changed in [
            ExceptionClause { flags: 0, ..clause },
            ExceptionClause {
                handler_len: 7,
                ..clause
            },
            ExceptionClause {
                try_len: 9,
                ..clause
            },
        ] {
            assert_eq!(engine_owned_output_tail(&code, target, changed), None);
        }
        let mut changed = code.clone();
        changed[10] = 0x08;
        assert_eq!(engine_owned_output_tail(&changed, target, clause), None);
        changed = code.clone();
        changed[8] = 0x2b;
        assert_eq!(engine_owned_output_tail(&changed, target, clause), None);
        changed = code.clone();
        changed[16] = 0x0a;
        assert_eq!(engine_owned_output_tail(&changed, target, clause), None);
        assert_eq!(
            engine_owned_output_tail(&code[..code.len() - 1], target, clause),
            None
        );
    }
    #[test]
    fn engine_handle_finally_requires_actual_eh_and_separate_result_and_pin_locals() {
        let target: u32 = 0x06005678;
        let converter: u32 = 0x2b009abc;
        let mut code = vec![0x12, 2, 0x28];
        code.extend(target.to_le_bytes());
        code.extend([0x13, 4, 0xde, 12, 0x11, 4, 0x28]);
        code.extend(converter.to_le_bytes());
        code.extend([0x0d, 0x16, 0xe0, 0x0b, 0xdc, 0x09, 0x2a]);
        let clause = ExceptionClause {
            flags: 2,
            try_offset: 0,
            try_len: 11,
            handler_offset: 11,
            handler_len: 12,
            data: 0,
        };
        assert_eq!(
            engine_handle_finally_tail(&code, target, clause),
            Some(converter)
        );
        assert_eq!(engine_handle_finally_tail(&code, target + 1, clause), None);
        for invalid in [
            ExceptionClause { flags: 0, ..clause },
            ExceptionClause {
                handler_offset: 12,
                ..clause
            },
            ExceptionClause {
                handler_len: 11,
                ..clause
            },
            ExceptionClause {
                try_len: 12,
                ..clause
            },
        ] {
            assert_eq!(engine_handle_finally_tail(&code, target, invalid), None);
        }
        let mut changed = code.clone();
        changed[12] = 3;
        assert_eq!(engine_handle_finally_tail(&changed, target, clause), None);
        changed = code.clone();
        changed[21] = 0x0d;
        assert_eq!(engine_handle_finally_tail(&changed, target, clause), None);
        changed = code.clone();
        changed[10] = 11;
        assert_eq!(engine_handle_finally_tail(&changed, target, clause), None);
        assert_eq!(std::mem::size_of::<ExceptionClause>(), 32);
        assert_eq!(std::mem::align_of::<ExceptionClause>(), 8);
    }
    #[test]
    fn owned_pointer_converter_requires_exact_field_read_and_complete_instruction_boundaries() {
        let field: u32 = 0x04007654;
        let mut code = vec![0x02, 0x7b];
        code.extend(field.to_le_bytes());
        code.push(0x2a);
        assert!(pointer_field_body(&code, field));
        assert!(!pointer_field_body(&code, field + 1));
        assert!(!pointer_field_body(&code, 0x06007654));
        let mut changed = code.clone();
        changed[1] = 0x80;
        assert!(!pointer_field_body(&changed, field));
        changed = code.clone();
        changed[0] = 0x03;
        assert!(!pointer_field_body(&changed, field));
        assert!(!pointer_field_body(&code[..code.len() - 1], field));
        code.extend([0x03, 0x26]);
        assert!(!pointer_field_body(&code, field));
    }
    #[test]
    fn nullable_owned_pointer_preserves_source_argument_and_rejects_other_branch_bodies() {
        let target: u32 = 0x06001234;
        let converter: u32 = 0x06006789;
        let mut code = vec![0x02, 0x03, 0x2c, 8, 0x03, 0x28];
        code.extend(converter.to_le_bytes());
        code.extend([0x2b, 3, 0x00, 0x16, 0xd3, 0x00, 0x28]);
        code.extend(target.to_le_bytes());
        code.push(0x2a);
        let source = |p, t| {
            (p == 1 && t == converter).then_some(PointerSource::OwnedResource {
                class: 0x1234,
                nullable: false,
            })
        };
        assert_eq!(
            engine_pointer_nullable_prefix(&code, target, 2, source, PointerSource::nullable),
            Some(vec![
                None,
                Some(PointerSource::OwnedResource {
                    class: 0x1234,
                    nullable: true
                })
            ])
        );
        let mut changed = code.clone();
        changed[4] = 0x02;
        assert!(engine_pointer_nullable_prefix(
            &changed,
            target,
            2,
            source,
            PointerSource::nullable
        )
        .is_none());
        changed = code.clone();
        changed[3] = 9;
        assert!(engine_pointer_nullable_prefix(
            &changed,
            target,
            2,
            source,
            PointerSource::nullable
        )
        .is_none());
        changed = code.clone();
        changed[13] = 0x17;
        assert!(engine_pointer_nullable_prefix(
            &changed,
            target,
            2,
            source,
            PointerSource::nullable
        )
        .is_none());
        assert!(engine_pointer_nullable_prefix::<PointerSource>(
            &code,
            target,
            2,
            |_, _| None,
            PointerSource::nullable
        )
        .is_none());
    }
    #[test]
    fn utf16_span_origin_requires_verified_string_helper_and_matching_local_argument() {
        let target: u32 = 0x06009876;
        let helper: u32 = 0x0a001234;
        let mut code = vec![0x03, 0x12, 2, 0x28];
        code.extend(helper.to_le_bytes());
        code.extend([0x2d, 0, 0x12, 2, 0x04, 0x28]);
        code.extend(target.to_le_bytes());
        code.push(0x2a);
        let valid = |p, t| p == 1 && t == helper;
        assert_eq!(
            engine_utf16_locals(&code, target, 3, &[1], valid),
            Some(vec![false, true, false])
        );
        assert_eq!(
            engine_utf16_locals(&code, target, 3, &[1], |_, _| false),
            None
        );
        assert_eq!(engine_utf16_locals(&code, target, 3, &[2], valid), None);
        let mut changed = code.clone();
        changed[1] = 0x11;
        assert_eq!(engine_utf16_locals(&changed, target, 3, &[1], valid), None);
        let mut overwritten = code.clone();
        overwritten.splice(8..8, [0x13, 2]);
        assert_eq!(
            engine_utf16_locals(&overwritten, target, 3, &[1], valid),
            None
        );
        assert_eq!(
            engine_utf16_locals(&code[..code.len() - 3], target, 3, &[1], valid),
            None
        );
        let mut duplicated = code.clone();
        duplicated.push(0x28);
        duplicated.extend(target.to_le_bytes());
        assert_eq!(
            engine_utf16_locals(&duplicated, target, 3, &[1], valid),
            None
        );
    }
    #[test]
    fn engine_pointer_wrappers_require_ordered_loads_and_verified_marshal_calls() {
        let target: u32 = 0x06001234;
        let marshal: u32 = 0x2b007654;
        let mut code = vec![0x02, 0x28];
        code.extend(marshal.to_le_bytes());
        code.extend([0x0f, 1, 0x04, 0x05, 0x28]);
        code.extend(target.to_le_bytes());
        code.push(0x2a);
        assert_eq!(
            engine_pointer_prefix(&code, target, 4, |index, token| {
                (index == 0 && token == marshal).then_some(true)
            }),
            Some(vec![Some(true), None, None, None])
        );
        assert_eq!(
            engine_pointer_prefix::<bool>(&code, target, 4, |_, _| None),
            None
        );
        let mut reordered = code.clone();
        reordered[0] = 0x03;
        assert_eq!(
            engine_pointer_prefix(&reordered, target, 4, |_, _| Some(true)),
            None
        );
        let mut branched = code.clone();
        branched.splice(0..0, [0x2b, 0]);
        assert_eq!(
            engine_pointer_prefix(&branched, target, 4, |_, _| Some(true)),
            None
        );
        let mut duplicate = code.clone();
        duplicate.push(0x28);
        duplicate.extend(target.to_le_bytes());
        assert_eq!(
            engine_pointer_prefix(&duplicate, target, 4, |_, _| Some(true)),
            None
        );
        assert_eq!(
            engine_pointer_prefix(&code[..code.len() - 2], target, 4, |_, _| Some(true)),
            None
        );
    }
    #[test]
    fn guarded_pointer_flow_tracks_actual_converters_and_rejects_ambiguous_paths() {
        let target: u32 = 0x06001357;
        let marshal: u32 = 0x2b004321;
        let thrower: u32 = 0x06008765;
        let mut code = vec![0x03, 0x2d, 11, 0x03, 0x72];
        code.extend(0x70001234u32.to_le_bytes());
        code.push(0x28);
        code.extend(thrower.to_le_bytes());
        code.extend([0, 0x02, 0x03, 0x28]);
        code.extend(marshal.to_le_bytes());
        code.extend([0x25, 0x2d, 11, 0x03, 0x72]);
        code.extend(0x70001234u32.to_le_bytes());
        code.push(0x28);
        code.extend(thrower.to_le_bytes());
        code.extend([0, 0x28]);
        code.extend(target.to_le_bytes());
        code.push(0x2a);
        let infer = |bytes: &[u8], allow_throw: bool| {
            engine_pointer_flow(
                bytes,
                target,
                2,
                |position, token| (position == 1 && token == marshal).then_some(false),
                |token| allow_throw && token == thrower,
            )
        };
        assert_eq!(infer(&code, true), Some(vec![None, Some(false)]));
        assert_eq!(infer(&code, false), None);
        assert_eq!(infer(&code[..code.len() - 2], true), None);
        let mut invalid = code.clone();
        invalid[2] = 9; // A branch into the ThrowHelper token operand.
        assert_eq!(infer(&invalid, true), None);
        invalid[2] = (-3i8) as u8; // Back edge to the first guard.
        assert_eq!(infer(&invalid, true), None);
        invalid = code.clone();
        invalid[16] = 0x02; // Source changed to the first argument.
        assert_eq!(infer(&invalid, true), None);
        let mut conflict = vec![0x02, 0x2d, 3, 0x02, 0x2b, 6, 0x02, 0x28];
        conflict.extend(marshal.to_le_bytes());
        conflict.push(0x28);
        conflict.extend(target.to_le_bytes());
        conflict.push(0x2a);
        assert_eq!(
            engine_pointer_flow(&conflict, target, 1, |_, _| Some(true), |_| false),
            None
        );
    }
    #[test]
    fn engine_handle_wrapper_uses_current_tokens_and_rejects_calls_inside_operands() {
        let mut code = vec![
            0x02, 0x28, 0x52, 0, 0, 0x2b, 0x25, 0x2d, 6, 0x02, 0x28, 0x98, 0x2c, 0, 6, 0,
        ];
        let tail = |target: u32, converter: u32| {
            let mut bytes = vec![0x28];
            bytes.extend_from_slice(&target.to_le_bytes());
            bytes.push(0x28);
            bytes.extend_from_slice(&converter.to_le_bytes());
            bytes.push(0x2a);
            bytes
        };
        code.extend(tail(0x06000826, 0x2b000056));
        assert_eq!(engine_handle_tail(&code, 0x06000826), Some(0x2b000056));
        assert_eq!(engine_handle_tail(&code, 0x06000827), None);
        assert_eq!(
            engine_handle_tail(&tail(0x06001234, 0x2b009876), 0x06001234),
            Some(0x2b009876)
        );
        assert_eq!(
            engine_handle_tail(&code[..code.len() - 1], 0x06000826),
            None
        );
        let mut disguised = vec![0x21]; // ldc.i8 operand spans the apparent CALL.
        disguised.extend(tail(0x06000826, 0x2b000056));
        assert_eq!(engine_handle_tail(&disguised, 0x06000826), None);
    }
    use crate::x64::memory::{READ, WRITE};
    #[test]
    fn constructed_instance_admission_keeps_generic_methods_and_static_entries_out() {
        assert!(concrete_method(false, false, false, false));
        assert!(concrete_method(false, false, true, false));
        assert!(!concrete_method(false, true, true, false));
        assert!(concrete_method(false, true, true, true));
        assert!(!concrete_method(false, true, false, true));
        assert!(!concrete_method(false, false, true, true));
        for inflated in [false, true] {
            for instance in [false, true] {
                for constructed in [false, true] {
                    assert!(!concrete_method(true, inflated, instance, constructed));
                }
            }
        }
    }
    fn boolean() -> Shape {
        Shape::Scalar {
            class: 1usize as Object,
            width: 1,
            alignment: 1,
            boolean: true,
        }
    }
    #[test]
    fn word_sized_aggregate_uses_metadata_alignment_instead_of_total_width() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        memory.write_int(0x1000, 8, 0x1084).unwrap();
        memory.write_int(0x1084, 8, 0x1234567887654321).unwrap();
        let mut descriptor = Descriptor {
            receiver: None,
            parameters: vec![Parameter {
                shape: Shape::Scalar {
                    class: 3usize as Object,
                    width: 8,
                    alignment: 4,
                    boolean: false,
                },
                byref: true,
            }],
            returns: None,
            name: "Aggregate".into(),
        };
        let mut frame = Frame::prepare(
            &memory,
            &descriptor,
            0x1000,
            |_, _| panic!("aggregate resolved as reference"),
            |_, _| true,
        )
        .unwrap();
        let pointers = frame.pointers();
        assert_eq!(unsafe { *pointers[0].cast::<u64>() }, 0x1234567887654321);
        unsafe { *pointers[0].cast::<u64>() = 0xabcdef0123456789 };
        let output = frame.outputs().unwrap();
        assert_eq!(output[0].address, 0x1084);
        assert_eq!(output[0].word, 0xabcdef0123456789);
        memory.write_int(0x1000, 8, 0x1082).unwrap();
        assert!(Frame::prepare(
            &memory,
            &descriptor,
            0x1000,
            |_, _| unreachable!(),
            |_, _| true
        )
        .is_err());
        descriptor.parameters[0].shape = Shape::Scalar {
            class: 3usize as Object,
            width: 8,
            alignment: 8,
            boolean: false,
        };
        memory.write_int(0x1000, 8, 0x1084).unwrap();
        assert!(Frame::prepare(
            &memory,
            &descriptor,
            0x1000,
            |_, _| unreachable!(),
            |_, _| true
        )
        .is_err());
    }
    fn coroutine() -> Descriptor {
        Descriptor {
            receiver: None,
            parameters: vec![
                Parameter {
                    shape: Shape::Reference {
                        class: 2usize as Object,
                        string: false,
                    },
                    byref: false,
                },
                Parameter {
                    shape: Shape::Scalar {
                        class: 3usize as Object,
                        width: 8,
                        alignment: 8,
                        boolean: false,
                    },
                    byref: false,
                },
            ],
            returns: None,
            name: "UnityEngine.SetupCoroutine::InvokeMoveNext".into(),
        }
    }
    #[test]
    fn coroutine_output_uses_private_byte_and_rejects_overlap_invalid_bool_and_canary_damage() {
        let mut memory = Memory::default();
        memory.map(0x2000, 4096, READ | WRITE).unwrap();
        memory.write_int(0x2000, 8, 17).unwrap();
        memory.write_int(0x2008, 8, 0x2100).unwrap();
        memory.write_int(0x2100, 8, 0x2201).unwrap();
        memory.write_int(0x2201, 1, 9).unwrap();
        let mut frame = Frame::prepare(
            &memory,
            &coroutine(),
            0x2000,
            |token, _| {
                assert_eq!(token, 17);
                Ok(0x999usize as Object)
            },
            |_, _| false,
        )
        .unwrap();
        frame
            .prepare_coroutine_output(&memory, 0x2000, 1usize as Object, |address, width| {
                address == 0x2201 && width == 1
            })
            .unwrap();
        let params = frame.pointers();
        assert_eq!(params[0], 0x999usize as Object);
        let native = unsafe { params[1].cast::<*mut u8>().read() };
        assert_ne!(native as u64, 0x2201);
        unsafe { native.write(1) };
        let output = frame.outputs().unwrap();
        assert_eq!(
            (output[0].address, output[0].shape.width(), output[0].word),
            (0x2201, 1, 1)
        );
        assert_eq!(memory.read_int(0x2201, 1).unwrap(), 9);
        unsafe { native.add(1).write(0) };
        assert!(frame.outputs().is_err());
        unsafe {
            native.add(1).write(0xa5);
            native.write(2)
        };
        assert!(frame.outputs().is_err());
        assert_eq!(memory.read_int(0x2201, 1).unwrap(), 9);
        for address in [0, 0x2000, 0x200f, 0x2100, 0x2107, 0x3000] {
            memory.write_int(0x2100, 8, address).unwrap();
            let mut rejected = Frame::prepare(
                &memory,
                &coroutine(),
                0x2000,
                |_, _| Ok(0x999usize as Object),
                |_, _| false,
            )
            .unwrap();
            assert!(rejected
                .prepare_coroutine_output(&memory, 0x2000, 1usize as Object, |_, _| true)
                .is_err());
            assert_eq!(rejected.words[1], address);
        }
        memory.write_int(0x2100, 8, 0x2201).unwrap();
        let mut retired = Frame::prepare(
            &memory,
            &coroutine(),
            0x2000,
            |_, _| Ok(0x999usize as Object),
            |_, _| false,
        )
        .unwrap();
        assert!(retired
            .prepare_coroutine_output(&memory, 0x2000, 1usize as Object, |_, _| false)
            .is_err());
        assert_eq!(retired.words[1], 0x2201);
    }
    fn string() -> Shape {
        Shape::Reference {
            class: 2usize as Object,
            string: true,
        }
    }
    fn scalar_output_inputs(count: usize) -> Descriptor {
        Descriptor {
            receiver: None,
            parameters: (0..count)
                .map(|_| Parameter {
                    shape: Shape::Scalar {
                        class: 3usize as Object,
                        width: 8,
                        alignment: 8,
                        boolean: false,
                    },
                    byref: false,
                })
                .collect(),
            returns: None,
            name: "unknown scalar outputs".into(),
        }
    }
    #[test]
    fn scalar_output_aliases_share_private_storage_and_fail_transactionally() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        memory.write_int(0x1000, 8, 0x1100).unwrap();
        memory.write_int(0x1008, 8, 0x1108).unwrap();
        let shape = Shape::Scalar {
            class: 4usize as Object,
            width: 4,
            alignment: 4,
            boolean: false,
        };
        for (first, second, second_shape, valid) in [
            (0x1200, 0x1200, shape, true),
            (0x1200, 0x1202, shape, false),
            (0x1200, 0x1200, boolean(), false),
            (0x1200, 0x1004, shape, false),
            (0x1200, 0x1104, shape, false),
            (0x1200, 0x1108, shape, false),
            (0x1200, 0x2000, shape, false),
        ] {
            memory.write_int(0x1100, 8, first).unwrap();
            memory.write_int(0x1108, 8, second).unwrap();
            let mut frame = Frame::prepare(
                &memory,
                &scalar_output_inputs(2),
                0x1000,
                |_, _| unreachable!(),
                |_, _| false,
            )
            .unwrap();
            let prepared = frame.prepare_scalar_outputs(
                &memory,
                0x1000,
                &[(0, shape), (1, second_shape)],
                |_, _| true,
            );
            assert_eq!(prepared.is_ok(), valid);
            if valid {
                let parameters = frame.pointers();
                let first = unsafe { parameters[0].cast::<*mut u32>().read() };
                let second = unsafe { parameters[1].cast::<*mut u32>().read() };
                assert_eq!(first, second);
                unsafe { first.write(77) };
                assert_eq!(frame.outputs().unwrap()[0].word, 77);
                unsafe { first.cast::<u8>().sub(1).write(0) };
                assert!(frame.outputs().is_err());
            } else {
                assert_eq!((frame.words[0], frame.words[1]), (first, second));
                assert!(frame.scalar_outputs.is_empty());
            }
        }
    }
    #[test]
    fn scalar_output_boolean_and_width_canaries_are_validated_before_copyback() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        memory.write_int(0x1000, 8, 0x1100).unwrap();
        memory.write_int(0x1100, 8, 0x1200).unwrap();
        memory.write_int(0x1200, 1, 9).unwrap();
        let mut frame = Frame::prepare(
            &memory,
            &scalar_output_inputs(1),
            0x1000,
            |_, _| unreachable!(),
            |_, _| false,
        )
        .unwrap();
        frame
            .prepare_scalar_outputs(&memory, 0x1000, &[(0, boolean())], |_, _| true)
            .unwrap();
        let parameters = frame.pointers();
        let output = unsafe { parameters[0].cast::<*mut u8>().read() };
        unsafe { output.write(2) };
        assert!(frame.outputs().is_err());
        unsafe { output.write(1) };
        assert_eq!(frame.outputs().unwrap()[0].word, 1);
        unsafe { output.add(1).write(0) };
        assert!(frame.outputs().is_err());
        assert_eq!(memory.read_int(0x1200, 1).unwrap(), 9);
    }
    #[test]
    fn owned_frame_uses_native_refs_and_coalesces_same_byref_alias_without_guest_pointers() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        let descriptor = Descriptor {
            receiver: None,
            name: "UnknownStaticMethod".into(),
            returns: Some(boolean()),
            parameters: vec![
                Parameter {
                    shape: string(),
                    byref: false,
                },
                Parameter {
                    shape: string(),
                    byref: true,
                },
                Parameter {
                    shape: string(),
                    byref: true,
                },
            ],
        };
        memory.write_int(0x1000, 8, 77).unwrap();
        memory.write_int(0x1008, 8, 0x1100).unwrap();
        memory.write_int(0x1010, 8, 0x1100).unwrap();
        memory.write_int(0x1100, 8, 77).unwrap();
        let mut frame = Frame::prepare(
            &memory,
            &descriptor,
            0x1000,
            |token, _| {
                assert_eq!(token, 77);
                Ok(99usize as Object)
            },
            |_, _| true,
        )
        .unwrap();
        let pointers = frame.pointers();
        assert_eq!(pointers[0] as usize, 99);
        assert_eq!(pointers[1], pointers[2]);
        assert_ne!(pointers[1] as usize, 0x1100);
        unsafe {
            pointers[1].cast::<Object>().write(ptr::null_mut());
        }
        let outputs = frame.outputs().unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].word, 0);
        assert_eq!(memory.read_int(0x1100, 8).unwrap(), 77);
    }
    #[test]
    fn late_noncanonical_boolean_and_bad_output_layout_leave_guest_outputs_untouched() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        let descriptor = Descriptor {
            receiver: None,
            name: "BooleanOutputs".into(),
            returns: None,
            parameters: vec![
                Parameter {
                    shape: boolean(),
                    byref: true,
                },
                Parameter {
                    shape: boolean(),
                    byref: true,
                },
            ],
        };
        memory.write_int(0x1000, 8, 0x1100).unwrap();
        memory.write_int(0x1008, 8, 0x1101).unwrap();
        memory.write_int(0x1100, 2, 0x0101).unwrap();
        let mut frame = Frame::prepare(
            &memory,
            &descriptor,
            0x1000,
            |_, _| unreachable!(),
            |_, _| true,
        )
        .unwrap();
        let pointers = frame.pointers();
        unsafe {
            pointers[0].cast::<u8>().write(0);
            pointers[1].cast::<u8>().write(2);
        }
        assert!(frame.outputs().is_err());
        assert_eq!(memory.read_int(0x1100, 2).unwrap(), 0x0101);
        assert!(Frame::prepare(
            &memory,
            &descriptor,
            0x1000,
            |_, _| unreachable!(),
            |_, _| false
        )
        .is_err());
        memory.write_int(0x1008, 8, 0x1000).unwrap();
        assert!(Frame::prepare(
            &memory,
            &descriptor,
            0x1000,
            |_, _| unreachable!(),
            |_, _| true
        )
        .is_err());
        assert!(Frame::prepare(
            &memory,
            &descriptor,
            u64::MAX - 7,
            |_, _| unreachable!(),
            |_, _| true
        )
        .is_err());
    }
    #[test]
    fn native_internalcall_pinvoke_abstract_and_synchronized_static_entries_are_rejected() {
        assert!(managed_il_flags(0x96, 0, false));
        assert!(managed_il_flags(0x86, 0, true));
        assert!(!managed_il_flags(0x96, 0, true));
        assert!(!managed_il_flags(0x86, 0, false));
        for flags in [0x6, 0x496, 0x2096] {
            assert!(!managed_il_flags(flags, 0, false));
        }
        for implementation in [1, 3, 4, 0x20, 0x1000] {
            assert!(!managed_il_flags(0x96, implementation, false));
            assert!(!managed_il_flags(0x86, implementation, true));
        }
    }
    #[test]
    fn delegate_invoke_admits_only_managed_instance_runtime_implementation() {
        assert!(delegate_invoke_flags(0x1c6, 3, true));
        assert!(!delegate_invoke_flags(0x1c6, 3, false));
        for flags in [0x1d6, 0x5c6, 0x21c6] {
            assert!(!delegate_invoke_flags(flags, 3, true));
        }
        for implementation in [0, 1, 2, 7, 0x23, 0x1003] {
            assert!(!delegate_invoke_flags(0x1c6, implementation, true));
        }
    }
}
