//! Generate serialization cells from actual declarations and attributes.
//! Neither class names nor serialized field names appear in the rules. This
//! first shape set rejects serialized structs with references and raw pointers.
use super::{c_char, fields::Spec, value_layout, Object};
use std::{ffi::CStr, ptr};
#[derive(Clone, Copy)]
struct Api {
    library: Object,
    fields: unsafe extern "C" fn(Object, *mut Object) -> Object,
    parent: unsafe extern "C" fn(Object) -> Object,
    image: unsafe extern "C" fn(Object) -> Object,
    field_parent: unsafe extern "C" fn(Object) -> Object,
    name: unsafe extern "C" fn(Object) -> *const c_char,
    flags: unsafe extern "C" fn(Object) -> u32,
    ty: unsafe extern "C" fn(Object) -> Object,
    kind: unsafe extern "C" fn(Object) -> u32,
    byref: unsafe extern "C" fn(Object) -> u32,
    type_class: unsafe extern "C" fn(Object) -> Object,
    is_enum: unsafe extern "C" fn(Object) -> i32,
    is_value: unsafe extern "C" fn(Object) -> i32,
    enum_base: unsafe extern "C" fn(Object) -> Object,
    size: unsafe extern "C" fn(Object, *mut u32) -> i32,
    attrs: unsafe extern "C" fn(Object, Object, *mut u16) -> Object,
    has: unsafe extern "C" fn(Object, Object) -> i32,
    free: unsafe extern "C" fn(Object),
    serialize: Object,
    serialize_reference: Object,
}
// FIELD_ACCESS_MASK=7, PUBLIC=6. NonSerialized always wins, including over
// SerializeField. Static, literal and readonly fields are not Unity cells.
fn serialized(flags: u32, attribute: bool) -> bool {
    flags & (0x10 | 0x20 | 0x40 | 0x80) == 0 && (flags & 7 == 6 || attribute)
}
impl Api {
    fn load(library: Object, core: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing field layout API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let find = sym!(
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let serialize = unsafe { find(core, c"UnityEngine".as_ptr(), c"SerializeField".as_ptr()) };
        let serialize_reference = unsafe {
            find(
                core,
                c"UnityEngine".as_ptr(),
                c"SerializeReference".as_ptr(),
            )
        };
        if serialize.is_null() || serialize_reference.is_null() {
            return Err("owned serialization attribute declarations absent".into());
        }
        Ok(Self {
            library,
            fields: sym!(
                "mono_class_get_fields",
                unsafe extern "C" fn(Object, *mut Object) -> Object
            ),
            parent: sym!(
                "mono_class_get_parent",
                unsafe extern "C" fn(Object) -> Object
            ),
            image: sym!(
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            ),
            field_parent: sym!(
                "mono_field_get_parent",
                unsafe extern "C" fn(Object) -> Object
            ),
            name: sym!(
                "mono_field_get_name",
                unsafe extern "C" fn(Object) -> *const c_char
            ),
            flags: sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32),
            ty: sym!(
                "mono_field_get_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            kind: sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32),
            byref: sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32),
            type_class: sym!(
                "mono_class_from_mono_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            is_enum: sym!("mono_class_is_enum", unsafe extern "C" fn(Object) -> i32),
            is_value: sym!(
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            ),
            enum_base: sym!(
                "mono_class_enum_basetype",
                unsafe extern "C" fn(Object) -> Object
            ),
            size: sym!(
                "mono_class_value_size",
                unsafe extern "C" fn(Object, *mut u32) -> i32
            ),
            attrs: sym!(
                "wire_mono_custom_attrs_from_field_checked",
                unsafe extern "C" fn(Object, Object, *mut u16) -> Object
            ),
            has: sym!(
                "mono_custom_attrs_has_attr",
                unsafe extern "C" fn(Object, Object) -> i32
            ),
            free: sym!("mono_custom_attrs_free", unsafe extern "C" fn(Object)),
            serialize,
            serialize_reference,
        })
    }
    fn specs(self, images: &[Object], object: Object, class: Object) -> Result<Vec<Spec>, String> {
        self.describe(images, object, class, false)
    }
    fn describe(
        self,
        images: &[Object],
        object: Object,
        class: Object,
        storage: bool,
    ) -> Result<Vec<Spec>, String> {
        let mut result = Vec::new();
        let mut declaring = class;
        let mut seen = Vec::new();
        while !declaring.is_null() && images.contains(&unsafe { (self.image)(declaring) }) {
            if seen.contains(&declaring) || seen.len() >= 16 {
                return Err("serialization class chain exceeds bounded acyclic contract".into());
            }
            seen.push(declaring);
            let mut iterator = ptr::null_mut();
            let mut count = 0;
            loop {
                let field = unsafe { (self.fields)(declaring, &mut iterator) };
                if field.is_null() {
                    break;
                }
                count += 1;
                if count > 128 || result.len() >= 256 {
                    return Err("serialization metadata field capacity exceeded".into());
                }
                if unsafe { (self.field_parent)(field) } != declaring {
                    return Err("enumerated field belongs to another class".into());
                }
                let flags = unsafe { (self.flags)(field) };
                if flags
                    & if storage {
                        0x10 | 0x40
                    } else {
                        0x10 | 0x20 | 0x40 | 0x80
                    }
                    != 0
                {
                    continue;
                }
                let mut status = 0;
                let info = if storage {
                    ptr::null_mut()
                } else {
                    unsafe { (self.attrs)(declaring, field, &mut status) }
                };
                let attribute = !info.is_null()
                    && status == 0
                    && (unsafe { (self.has)(info, self.serialize) } != 0
                        || unsafe { (self.has)(info, self.serialize_reference) } != 0);
                if !info.is_null() {
                    unsafe {
                        (self.free)(info);
                    }
                }
                if status != 0 {
                    return Err(format!("checked serialization attributes failed: {status}"));
                }
                if !storage && !serialized(flags, attribute) {
                    continue;
                }
                let ty = unsafe { (self.ty)(field) };
                if ty.is_null() || unsafe { (self.byref)(ty) } != 0 {
                    return Err("serialized field is unknown/byref".into());
                }
                let kind = unsafe { (self.kind)(ty) };
                let target = matches!(kind, 0xe | 0x11 | 0x12 | 0x15 | 0x1c | 0x1d)
                    .then(|| unsafe { (self.type_class)(ty) });
                if target.is_some_and(|c| c.is_null()) {
                    return Err("serialized field type class absent".into());
                }
                if kind == 0x15 && unsafe { (self.is_value)(target.unwrap()) } != 0 {
                    return Err("serialized generic value requires a reference-aware layout".into());
                }
                let inline = if kind == 0x11 {
                    let target = target.unwrap();
                    if unsafe { (self.is_enum)(target) } == 0 {
                        let image = unsafe { (self.image)(target) };
                        if !images.contains(&image) {
                            return Err("inline scalar value outside owned images".into());
                        }
                        let name = unsafe { (self.name)(field) };
                        let label = if name.is_null() {
                            "<absent>".into()
                        } else {
                            unsafe { CStr::from_ptr(name) }.to_string_lossy()
                        };
                        Some(
                            value_layout::Api::load(self.library, image)?
                                .layout(target)
                                .and_then(|layout| layout.scalar_inline())
                                .map_err(|e| format!("inline field {label}: {e}"))?,
                        )
                    } else {
                        let base = unsafe { (self.enum_base)(target) };
                        let mut alignment = 0;
                        if base.is_null() {
                            return Err("serialized enum base absent".into());
                        }
                        let width = value_layout::enum_width(unsafe { (self.kind)(base) })
                            .ok_or("serialized enum base is not an integer")?;
                        if unsafe { (self.size)(target, &mut alignment) } != width as i32
                            || alignment as usize != width
                        {
                            return Err(
                                "serialized enum size/alignment differs from integer base".into()
                            );
                        }
                        Some((width, width))
                    }
                } else if matches!(kind, 2..=0xe | 0x12 | 0x15 | 0x1c | 0x1d) {
                    None
                } else {
                    let name = unsafe { (self.name)(field) };
                    let name = if name.is_null() {
                        "<absent>".into()
                    } else {
                        unsafe { CStr::from_ptr(name) }.to_string_lossy()
                    };
                    return Err(format!(
                        "serialized field {name} kind={kind:#x} requires a typed adapter"
                    ));
                };
                let name = unsafe { (self.name)(field) };
                if name.is_null() {
                    return Err("serialized field name absent".into());
                }
                result.push(Spec {
                    owner: object,
                    name: unsafe { CStr::from_ptr(name) }.to_owned(),
                    kind,
                    type_class: target,
                    guest_pointer: None,
                    inline,
                });
            }
            declaring = unsafe { (self.parent)(declaring) };
        }
        Ok(result)
    }
}
pub(super) fn specs(
    library: Object,
    core: Object,
    images: &[Object],
    object: Object,
    class: Object,
) -> Result<Vec<Spec>, String> {
    Api::load(library, core)?.specs(images, object, class)
}
/// Internal backing fields only for resolved reference generic instances in
/// the fingerprinted corlib. Private/NonSerialized/readonly storage matters
/// here; this does not change Unity serialization field selection.
pub(super) fn storage_specs(
    library: Object,
    core: Object,
    corlib: Object,
    object: Object,
    class: Object,
) -> Result<Option<Vec<Spec>>, String> {
    let api = Api::load(library, core)?;
    if class.is_null()
        || unsafe { (api.image)(class) } != corlib
        || unsafe { (api.is_value)(class) } != 0
    {
        return Ok(None);
    }
    let get_type: unsafe extern "C" fn(Object) -> Object = unsafe {
        let p = libc::dlsym(library, c"mono_class_get_type".as_ptr());
        if p.is_null() {
            return Err("container class type API absent".into());
        }
        std::mem::transmute(p)
    };
    let ty = unsafe { get_type(class) };
    if ty.is_null() || unsafe { (api.kind)(ty) } != 0x15 {
        return Ok(None);
    }
    Ok(Some(api.describe(&[corlib], object, class, true)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! {
        static UNSAFE_SHAPE: Cell<bool> = const { Cell::new(false) };
        static ATTRIBUTE_FAILURE: Cell<bool> = const { Cell::new(false) };
        static ENUM_KIND: Cell<u32> = const { Cell::new(0) };
        static WRONG_ENUM_SIZE: Cell<bool> = const { Cell::new(false) };
        static GENERIC_REFERENCE: Cell<bool> = const { Cell::new(false) };
        static GENERIC_VALUE: Cell<bool> = const { Cell::new(false) };
        static REFERENCE_ATTRIBUTE: Cell<bool> = const { Cell::new(false) };
        static INFOS: Cell<usize> = const { Cell::new(0) };
    }
    unsafe extern "C" fn enumerate(_: Object, iterator: *mut Object) -> Object {
        let index = unsafe { iterator.read() } as usize + 1;
        unsafe {
            iterator.write(index as Object);
        }
        if index <= 3 {
            index as Object
        } else {
            ptr::null_mut()
        }
    }
    unsafe extern "C" fn parent(_: Object) -> Object {
        ptr::null_mut()
    }
    unsafe extern "C" fn image(_: Object) -> Object {
        200usize as Object
    }
    unsafe extern "C" fn owner(_: Object) -> Object {
        100usize as Object
    }
    unsafe extern "C" fn name(field: Object) -> *const c_char {
        if field as usize == 1 {
            c"PreviouslyUnknownScalar".as_ptr()
        } else {
            c"PrivateSerializedReference".as_ptr()
        }
    }
    unsafe extern "C" fn flags(field: Object) -> u32 {
        match field as usize {
            1 => 6,
            2 => 1,
            _ => 6 | 0x80,
        }
    }
    unsafe extern "C" fn ty(field: Object) -> Object {
        field
    }
    unsafe extern "C" fn kind(ty: Object) -> u32 {
        if UNSAFE_SHAPE.with(Cell::get) && ty as usize == 2 {
            0xf
        } else if ENUM_KIND.with(Cell::get) != 0 && ty as usize == 1 {
            0x11
        } else if ENUM_KIND.with(Cell::get) != 0 && ty as usize == 100 {
            ENUM_KIND.with(Cell::get)
        } else if GENERIC_REFERENCE.with(Cell::get) && ty as usize == 2 {
            0x15
        } else if ty as usize == 1 {
            8
        } else {
            0xe
        }
    }
    unsafe extern "C" fn zero(_: Object) -> u32 {
        0
    }
    unsafe extern "C" fn is_enum(_: Object) -> i32 {
        i32::from(ENUM_KIND.with(Cell::get) != 0)
    }
    unsafe extern "C" fn is_value(_: Object) -> i32 {
        i32::from(GENERIC_VALUE.with(Cell::get))
    }
    unsafe extern "C" fn size(_: Object, alignment: *mut u32) -> i32 {
        let width = value_layout::enum_width(ENUM_KIND.with(Cell::get)).unwrap_or(0) as u32;
        unsafe { alignment.write(width) };
        if WRONG_ENUM_SIZE.with(Cell::get) {
            4
        } else {
            width as i32
        }
    }
    unsafe extern "C" fn attrs(_: Object, field: Object, status: *mut u16) -> Object {
        INFOS.with(|c| c.set(c.get() + 1));
        unsafe {
            status.write(if ATTRIBUTE_FAILURE.with(Cell::get) {
                7
            } else {
                0
            });
        }
        field
    }
    unsafe extern "C" fn has(info: Object, attribute: Object) -> i32 {
        i32::from(
            info as usize == 2
                && attribute as usize
                    == if REFERENCE_ATTRIBUTE.with(Cell::get) {
                        301
                    } else {
                        300
                    },
        )
    }
    unsafe extern "C" fn free(_: Object) {
        INFOS.with(|c| c.set(c.get() - 1));
    }
    fn api() -> Api {
        UNSAFE_SHAPE.with(|c| c.set(false));
        ATTRIBUTE_FAILURE.with(|c| c.set(false));
        ENUM_KIND.with(|c| c.set(0));
        WRONG_ENUM_SIZE.with(|c| c.set(false));
        GENERIC_REFERENCE.with(|c| c.set(false));
        GENERIC_VALUE.with(|c| c.set(false));
        INFOS.with(|c| c.set(0));
        REFERENCE_ATTRIBUTE.with(|c| c.set(false));
        Api {
            library: ptr::null_mut(),
            fields: enumerate,
            parent,
            image,
            field_parent: owner,
            name,
            flags,
            ty,
            kind,
            byref: zero,
            type_class: owner,
            is_enum,
            is_value,
            enum_base: owner,
            size,
            attrs,
            has,
            free,
            serialize: 300usize as Object,
            serialize_reference: 301usize as Object,
        }
    }
    #[test]
    fn previously_unknown_fields_are_selected_without_constructor_execution() {
        let specs = api()
            .specs(
                &[200usize as Object],
                400usize as Object,
                100usize as Object,
            )
            .unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, c"PreviouslyUnknownScalar");
        assert_eq!(specs[0].kind, 8);
        assert_eq!(specs[1].name, c"PrivateSerializedReference");
        assert_eq!(specs[1].kind, 0xe);
        assert_eq!(INFOS.with(Cell::get), 0);
    }
    #[test]
    fn serialize_reference_selects_private_reference_and_nonserialized_still_wins() {
        let api = api();
        REFERENCE_ATTRIBUTE.with(|c| c.set(true));
        let specs = api
            .specs(
                &[200usize as Object],
                400usize as Object,
                100usize as Object,
            )
            .unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[1].name, c"PrivateSerializedReference");
        assert_eq!(INFOS.with(Cell::get), 0);
        assert!(!serialized(1 | 0x80, true));
    }
    #[test]
    fn unsupported_late_shape_and_attribute_error_reject_whole_descriptor() {
        let api = api();
        UNSAFE_SHAPE.with(|c| c.set(true));
        assert!(api
            .specs(
                &[200usize as Object],
                400usize as Object,
                100usize as Object
            )
            .is_err());
        assert_eq!(INFOS.with(Cell::get), 0);
        UNSAFE_SHAPE.with(|c| c.set(false));
        ATTRIBUTE_FAILURE.with(|c| c.set(true));
        assert!(api
            .specs(
                &[200usize as Object],
                400usize as Object,
                100usize as Object
            )
            .is_err());
        assert_eq!(INFOS.with(Cell::get), 0);
    }
    #[test]
    fn generic_reference_identity_is_admitted_but_generic_value_is_not() {
        let api = api();
        GENERIC_REFERENCE.with(|c| c.set(true));
        let describe = || {
            api.specs(
                &[200usize as Object],
                400usize as Object,
                100usize as Object,
            )
        };
        let fields = describe().unwrap();
        assert_eq!(fields[1].kind, 0x15);
        assert_eq!(fields[1].type_class, Some(100usize as Object));
        assert!(fields[1].inline.is_none());
        GENERIC_VALUE.with(|c| c.set(true));
        assert!(describe().is_err());
        assert_eq!(INFOS.with(Cell::get), 0);
    }
    #[test]
    fn wide_enum_cells_require_matching_native_size_and_integer_base() {
        let api = api();
        ENUM_KIND.with(|c| c.set(0xa));
        let describe = || {
            api.specs(
                &[200usize as Object],
                400usize as Object,
                100usize as Object,
            )
        };
        assert_eq!(describe().unwrap()[0].inline, Some((8, 8)));
        WRONG_ENUM_SIZE.with(|c| c.set(true));
        assert!(describe().is_err());
        WRONG_ENUM_SIZE.with(|c| c.set(false));
        ENUM_KIND.with(|c| c.set(2));
        assert!(describe().is_err());
        assert_eq!(INFOS.with(Cell::get), 0);
    }
    #[test]
    fn storage_fields_include_private_and_nonserialized_without_attributes() {
        let api = api();
        ATTRIBUTE_FAILURE.with(|c| c.set(true));
        let fields = api
            .describe(
                &[200usize as Object],
                400usize as Object,
                100usize as Object,
                true,
            )
            .unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(INFOS.with(Cell::get), 0);
        UNSAFE_SHAPE.with(|c| c.set(true));
        assert!(api
            .describe(
                &[200usize as Object],
                400usize as Object,
                100usize as Object,
                true
            )
            .is_err());
    }
    #[test]
    fn metadata_visibility_and_attributes_select_fields() {
        assert!(serialized(6, false));
        assert!(serialized(1, true));
        assert!(serialized(3, true));
        assert!(!serialized(1, false));
        for flag in [0x10, 0x20, 0x40, 0x80] {
            assert!(!serialized(6 | flag, true));
        }
    }
}
