//! Metadata-qualified ordinary IL calls with owned scalar arguments and String
//! byref outputs. Guest pointers never enter mono_runtime_invoke's parameter ABI.
use super::{c_char, closed_types, Memory, Object};
use crate::x64::memory::{READ, WRITE};
use std::{ffi::CStr, ptr};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Scalar {
        class: Object,
        width: usize,
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
#[derive(Clone, Copy)]
pub(super) struct Api {
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
            2 | 4 | 5 => Some((1, kind == 2)),
            3 | 6 | 7 => Some((2, false)),
            8 | 9 | 0xc => Some((4, false)),
            0xa | 0xb | 0xd => Some((8, false)),
            0x11 if unsafe { (self.is_enum)(class) } != 0 => {
                let base = unsafe { (self.enum_base)(class) };
                if base.is_null() {
                    return Err("managed enum lacks base type".into());
                }
                Some((
                    super::value_layout::enum_width(unsafe { (self.kind)(base) })
                        .ok_or("managed enum base is not integer")?,
                    false,
                ))
            }
            _ => None,
        };
        if let Some((width, boolean)) = scalar {
            let mut alignment = 0;
            if unsafe { (self.value)(class) } == 0
                || unsafe { (self.size)(class, &mut alignment) } != width as i32
                || alignment as usize != width
            {
                return Err("managed scalar size/alignment mismatch".into());
            }
            Ok(Shape::Scalar {
                class,
                width,
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
        if class.is_null()
            || !images.contains(&unsafe { (self.image)(class) })
            || sig.is_null()
            || !managed_il_flags(flags, implementation, unsafe { (self.instance)(sig) } != 0)
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
        {
            return Err("managed method outside owned concrete IL contract".into());
        }
        self.closed.concrete(class)?;
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
    pub(super) fn buffer_copy_icall(
        self,
        method: Object,
        owner: Object,
        classes: &[Object; 2],
        images: &[Object],
    ) -> Result<(), String> {
        self.fixed_buffer_icall(
            method,
            owner,
            &[
                (1, classes[0]),
                (0xf, classes[0]),
                (0xf, classes[0]),
                (0xa, classes[1]),
            ],
            images,
        )
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
fn managed_il_flags(flags: u32, implementation: u32, instance: bool) -> bool {
    (flags & 0x10 == 0) == instance
        && flags & (0x400 | 0x2000) == 0
        && implementation & (0x3 | 0x4 | 0x20 | 0x1000) == 0
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
}
pub(super) struct Output {
    pub(super) address: u64,
    pub(super) shape: Shape,
    pub(super) word: u64,
}
impl Frame {
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
        };
        for (index, parameter) in descriptor.parameters.iter().enumerate() {
            let argument = memory.read_int(arguments + index as u64 * 8, 8)?;
            let width = parameter.shape.width();
            let reference = matches!(parameter.shape, Shape::Reference { .. });
            let word = if parameter.byref || !reference {
                if argument == 0 || !argument.is_multiple_of(width as u64) {
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
    use crate::x64::memory::{READ, WRITE};
    fn boolean() -> Shape {
        Shape::Scalar {
            class: 1usize as Object,
            width: 1,
            boolean: true,
        }
    }
    fn string() -> Shape {
        Shape::Reference {
            class: 2usize as Object,
            string: true,
        }
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
}
