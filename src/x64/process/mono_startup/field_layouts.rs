//! Generate serialization cells from actual declarations and attributes.
//! Neither class names nor serialized field names appear in the rules. This
//! first shape set rejects serialized structs with references and raw pointers.
use super::{c_char, fields::Spec, Object};
use std::{ffi::CStr, ptr};
#[derive(Clone, Copy)]
struct Api {
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
    enum_base: unsafe extern "C" fn(Object) -> Object,
    size: unsafe extern "C" fn(Object, *mut u32) -> i32,
    attrs: unsafe extern "C" fn(Object, Object, *mut u16) -> Object,
    has: unsafe extern "C" fn(Object, Object) -> i32,
    free: unsafe extern "C" fn(Object),
    serialize: Object,
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
        if serialize.is_null() {
            return Err("owned SerializeField declaration absent".into());
        }
        Ok(Self {
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
        })
    }
    fn specs(self, images: &[Object], object: Object, class: Object) -> Result<Vec<Spec>, String> {
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
                if flags & (0x10 | 0x20 | 0x40 | 0x80) != 0 {
                    continue;
                }
                let mut status = 0;
                let info = unsafe { (self.attrs)(declaring, field, &mut status) };
                let attribute = !info.is_null()
                    && status == 0
                    && unsafe { (self.has)(info, self.serialize) } != 0;
                if !info.is_null() {
                    unsafe {
                        (self.free)(info);
                    }
                }
                if status != 0 {
                    return Err(format!("checked serialization attributes failed: {status}"));
                }
                if !serialized(flags, attribute) {
                    continue;
                }
                let ty = unsafe { (self.ty)(field) };
                if ty.is_null() || unsafe { (self.byref)(ty) } != 0 {
                    return Err("serialized field is unknown/byref".into());
                }
                let kind = unsafe { (self.kind)(ty) };
                let target = matches!(kind, 0xe | 0x11 | 0x12 | 0x1d)
                    .then(|| unsafe { (self.type_class)(ty) });
                if target.is_some_and(|c| c.is_null()) {
                    return Err("serialized field type class absent".into());
                }
                let inline = if kind == 0x11 {
                    let target = target.unwrap();
                    if unsafe { (self.is_enum)(target) } == 0 {
                        return Err(
                            "serialized struct requires a typed reference-aware layout".into()
                        );
                    }
                    let base = unsafe { (self.enum_base)(target) };
                    let mut alignment = 0;
                    if base.is_null()
                        || !matches!(unsafe { (self.kind)(base) }, 8 | 9)
                        || unsafe { (self.size)(target, &mut alignment) } != 4
                        || alignment != 4
                    {
                        return Err("serialized enum requires exact four-byte layout".into());
                    }
                    Some((4, 4))
                } else if matches!(kind, 2..=0xe | 0x12 | 0x1d) {
                    None
                } else {
                    return Err("serialized pointer/complex shape requires a typed adapter".into());
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
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! {
        static UNSAFE_SHAPE: Cell<bool> = const { Cell::new(false) };
        static ATTRIBUTE_FAILURE: Cell<bool> = const { Cell::new(false) };
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
        0
    }
    unsafe extern "C" fn size(_: Object, _: *mut u32) -> i32 {
        0
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
    unsafe extern "C" fn has(info: Object, _: Object) -> i32 {
        i32::from(info as usize == 2)
    }
    unsafe extern "C" fn free(_: Object) {
        INFOS.with(|c| c.set(c.get() - 1));
    }
    fn api() -> Api {
        UNSAFE_SHAPE.with(|c| c.set(false));
        ATTRIBUTE_FAILURE.with(|c| c.set(false));
        INFOS.with(|c| c.set(0));
        Api {
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
            enum_base: owner,
            size,
            attrs,
            has,
            free,
            serialize: 300usize as Object,
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
