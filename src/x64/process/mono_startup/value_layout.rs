//! Bounded metadata-derived value layouts. Only declared scalar/enum/reference
//! slots are copied; padding is owned zero bytes, never native heap contents.
use super::{instances, Object};
use std::ptr;
#[derive(Clone, Copy)]
pub(super) enum Shape {
    Scalar { width: usize, boolean: bool },
    Reference(Object),
    Inline { width: usize, alignment: usize },
}
impl Shape {
    pub(super) fn width(self) -> usize {
        match self {
            Self::Scalar { width, .. } | Self::Inline { width, .. } => width,
            Self::Reference(_) => 8,
        }
    }
    fn alignment(self) -> usize {
        match self {
            Self::Inline { alignment, .. } => alignment,
            _ => self.width(),
        }
    }
}
#[derive(Clone)]
pub(super) struct Slot {
    pub(super) nested: bool,
    pub(super) offset: usize,
    pub(super) field: Object,
    pub(super) shape: Shape,
}
#[derive(Clone)]
pub(super) struct Layout {
    pub(super) class: Object,
    pub(super) width: usize,
    pub(super) alignment: usize,
    pub(super) slots: Vec<Slot>,
    pub(super) value: bool,
}
impl Layout {
    pub(super) fn reference(class: Object) -> Self {
        Self {
            class,
            width: 8,
            alignment: 8,
            slots: vec![Slot {
                nested: false,
                offset: 0,
                field: ptr::null_mut(),
                shape: Shape::Reference(class),
            }],
            value: false,
        }
    }
    fn append_nested(&mut self, offset: usize, child: Self) -> Result<(), String> {
        child.check()?;
        let end = offset
            .checked_add(child.width)
            .ok_or("nested value offset overflow")?;
        if end > self.width
            || !offset.is_multiple_of(child.alignment)
            || child.alignment > self.alignment
        {
            return Err("nested value exceeds parent size/alignment".into());
        }
        for mut slot in child.slots {
            slot.offset = slot
                .offset
                .checked_add(offset)
                .ok_or("nested leaf offset overflow")?;
            slot.nested = true;
            self.slots.push(slot);
        }
        Ok(())
    }
    pub(super) fn check(&self) -> Result<(), String> {
        if self.class.is_null()
            || !(1..=256).contains(&self.width)
            || !matches!(self.alignment, 1 | 2 | 4 | 8)
            || !self.width.is_multiple_of(self.alignment)
            || self.slots.is_empty()
            || self.slots.len() > 64
        {
            return Err("value layout size/alignment/capacity mismatch".into());
        }
        let mut covered = vec![false; self.width];
        for slot in &self.slots {
            let width = slot.shape.width();
            let alignment = slot.shape.alignment();
            if width == 0
                || width > 256
                || !matches!(alignment, 1 | 2 | 4 | 8)
                || alignment > self.alignment
                || !width.is_multiple_of(alignment)
                || slot.offset + width > self.width
                || !slot.offset.is_multiple_of(alignment)
                || matches!(slot.shape,Shape::Reference(class) if class.is_null() || self.alignment < 8)
                || covered[slot.offset..slot.offset + width].iter().any(|&b| b)
            {
                return Err("value layout overlapping/misaligned/out-of-bounds slot".into());
            }
            covered[slot.offset..slot.offset + width].fill(true);
        }
        Ok(())
    }
    /// A plain inline cell is allowed only when every byte is a declared
    /// non-Boolean scalar. No reference, unchecked Boolean or padding can be
    /// hidden in the field's raw get/set ABI.
    pub(super) fn scalar_inline(&self) -> Result<(usize, usize), String> {
        self.check()?;
        if self.slots.iter().any(|s| {
            !matches!(
                s.shape,
                Shape::Scalar { boolean: false, .. } | Shape::Inline { .. }
            )
        }) || self.slots.iter().map(|s| s.shape.width()).sum::<usize>() != self.width
        {
            return Err("inline value requires complete non-Boolean scalar coverage".into());
        }
        Ok((self.width, self.alignment))
    }
    pub(super) fn validate(
        &self,
        bytes: &[u8],
        api: instances::Api,
        roots: &instances::Registry,
    ) -> Result<Vec<Option<Object>>, String> {
        self.check()?;
        if bytes.len() != self.width {
            return Err("typed value payload length mismatch".into());
        }
        let mut covered = vec![false; self.width];
        let mut references = Vec::new();
        for slot in &self.slots {
            let data = &bytes[slot.offset..slot.offset + slot.shape.width()];
            covered[slot.offset..slot.offset + slot.shape.width()].fill(true);
            references.push(match slot.shape {
                Shape::Reference(class) => Some({
                    let token = u64::from_le_bytes(data.try_into().unwrap());
                    if token == 0 {
                        ptr::null_mut()
                    } else {
                        roots.compatible(api, token, class)?
                    }
                }),
                Shape::Scalar { boolean: true, .. } if data[0] > 1 => {
                    return Err("typed value Boolean is not canonical".into())
                }
                _ => None,
            });
        }
        if bytes
            .iter()
            .zip(covered)
            .any(|(&byte, covered)| !covered && byte != 0)
        {
            return Err("typed value padding must remain owned zero bytes".into());
        }
        Ok(references)
    }
}
#[derive(Clone, Copy)]
pub(super) struct Api {
    image: unsafe extern "C" fn(Object) -> Object,
    value: unsafe extern "C" fn(Object) -> i32,
    size: unsafe extern "C" fn(Object, *mut u32) -> i32,
    fields: unsafe extern "C" fn(Object, *mut Object) -> Object,
    flags: unsafe extern "C" fn(Object) -> u32,
    parent: unsafe extern "C" fn(Object) -> Object,
    offset: unsafe extern "C" fn(Object) -> i32,
    ty: unsafe extern "C" fn(Object) -> Object,
    kind: unsafe extern "C" fn(Object) -> u32,
    byref: unsafe extern "C" fn(Object) -> u32,
    type_class: unsafe extern "C" fn(Object) -> Object,
    is_enum: unsafe extern "C" fn(Object) -> i32,
    enum_base: unsafe extern "C" fn(Object) -> Object,
    owned_image: Object,
}
pub(super) fn enum_width(kind: u32) -> Option<usize> {
    match kind {
        4 | 5 => Some(1),
        6 | 7 => Some(2),
        8 | 9 => Some(4),
        0xa | 0xb => Some(8),
        _ => None,
    }
}
fn scalar(kind: u32) -> Option<Shape> {
    Some(Shape::Scalar {
        width: match kind {
            2 | 4 | 5 => 1,
            3 | 6 | 7 => 2,
            8 | 9 | 0xc => 4,
            0xa | 0xb | 0xd => 8,
            _ => return None,
        },
        boolean: kind == 2,
    })
}
impl Api {
    pub(super) fn load(library: Object, owned_image: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing value layout API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            image: sym!(
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            ),
            value: sym!(
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            ),
            size: sym!(
                "mono_class_value_size",
                unsafe extern "C" fn(Object, *mut u32) -> i32
            ),
            fields: sym!(
                "mono_class_get_fields",
                unsafe extern "C" fn(Object, *mut Object) -> Object
            ),
            flags: sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32),
            parent: sym!(
                "mono_field_get_parent",
                unsafe extern "C" fn(Object) -> Object
            ),
            offset: sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> i32),
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
            owned_image,
        })
    }
    pub(super) fn owns(self, class: Object) -> bool {
        !class.is_null() && unsafe { (self.image)(class) } == self.owned_image
    }
    pub(super) fn layout(self, class: Object) -> Result<Layout, String> {
        self.layout_inner(class, &mut Vec::new())
    }
    fn layout_inner(self, class: Object, path: &mut Vec<Object>) -> Result<Layout, String> {
        if path.len() >= 8 || path.contains(&class) {
            return Err("nested value layout exceeds acyclic depth bound".into());
        }
        if !self.owns(class) || unsafe { (self.value)(class) } == 0 {
            return Err("value layout requires an owned metadata value class".into());
        }
        path.push(class);
        let mut alignment = 0;
        let width = unsafe { (self.size)(class, &mut alignment) };
        if !(1..=256).contains(&width) {
            return Err("value layout exceeds bounded payload".into());
        }
        let mut layout = Layout {
            class,
            width: width as usize,
            alignment: alignment as usize,
            slots: Vec::new(),
            value: true,
        };
        let mut iterator = ptr::null_mut();
        let mut count = 0;
        let mut ranges = Vec::new();
        loop {
            let field = unsafe { (self.fields)(class, &mut iterator) };
            if field.is_null() {
                break;
            }
            count += 1;
            if count > 128 {
                return Err("value field count exceeded".into());
            }
            if unsafe { (self.parent)(field) } != class {
                return Err("value field declaring class mismatch".into());
            }
            if unsafe { (self.flags)(field) } & (0x10 | 0x40) != 0 {
                continue;
            }
            let ty = unsafe { (self.ty)(field) };
            if ty.is_null() || unsafe { (self.byref)(ty) } != 0 {
                return Err("value field unknown/byref".into());
            }
            let kind = unsafe { (self.kind)(ty) };
            let offset = unsafe { (self.offset)(field) };
            if offset < 16 {
                return Err("value field points into native object header".into());
            }
            let offset = (offset - 16) as usize;
            if kind == 0x11 {
                let target = unsafe { (self.type_class)(ty) };
                if target.is_null() {
                    return Err("nested value class absent".into());
                }
                if unsafe { (self.is_enum)(target) } == 0 {
                    let child = self.layout_inner(target, path)?;
                    let end = offset
                        .checked_add(child.width)
                        .ok_or("nested field range overflow")?;
                    if ranges
                        .iter()
                        .any(|&(start, stop)| offset < stop && start < end)
                    {
                        return Err("nested value field envelopes overlap".into());
                    }
                    ranges.push((offset, end));
                    if let Ok((width, alignment)) = child.scalar_inline() {
                        layout.slots.push(Slot {
                            nested: false,
                            offset,
                            field,
                            shape: Shape::Inline { width, alignment },
                        });
                    } else {
                        layout.append_nested(offset, child)?;
                    }
                    continue;
                }
            }
            let shape = if let Some(shape) = scalar(kind) {
                shape
            } else if matches!(kind, 0xe | 0x12 | 0x1d) {
                Shape::Reference(unsafe { (self.type_class)(ty) })
            } else if kind == 0x11 {
                let target = unsafe { (self.type_class)(ty) };
                if target.is_null() {
                    return Err("nested value class absent".into());
                }
                {
                    let base = unsafe { (self.enum_base)(target) };
                    if base.is_null() {
                        return Err("value enum base absent".into());
                    }
                    let kind = unsafe { (self.kind)(base) };
                    enum_width(kind).ok_or("value enum base is not an integer")?;
                    let shape = scalar(kind).unwrap();
                    let mut align = 0;
                    if unsafe { (self.size)(target, &mut align) } != shape.width() as i32
                        || align as usize != shape.width()
                    {
                        return Err("value enum size/alignment mismatch".into());
                    }
                    shape
                }
            } else {
                return Err("raw pointer/generic/complex value field is not admitted".into());
            };
            let end = offset
                .checked_add(shape.width())
                .ok_or("value field range overflow")?;
            if ranges
                .iter()
                .any(|&(start, stop)| offset < stop && start < end)
            {
                return Err("value field envelopes overlap".into());
            }
            ranges.push((offset, end));
            layout.slots.push(Slot {
                nested: false,
                offset,
                field,
                shape,
            });
        }
        layout.check()?;
        path.pop();
        Ok(layout)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn api_for_empty_refs() -> instances::Api {
        instances::tests::api()
    }
    fn layout() -> Layout {
        Layout {
            class: 2usize as Object,
            width: 24,
            alignment: 8,
            value: true,
            slots: vec![
                Slot {
                    nested: false,
                    offset: 0,
                    field: 1usize as Object,
                    shape: Shape::Reference(2usize as Object),
                },
                Slot {
                    nested: false,
                    offset: 8,
                    field: 2usize as Object,
                    shape: Shape::Scalar {
                        width: 4,
                        boolean: false,
                    },
                },
                Slot {
                    nested: false,
                    offset: 16,
                    field: 3usize as Object,
                    shape: Shape::Reference(2usize as Object),
                },
            ],
        }
    }
    #[test]
    fn inline_scalar_structs_reject_hidden_reference_boolean_and_padding() {
        let mut scalar = Layout {
            class: 2usize as Object,
            width: 12,
            alignment: 4,
            value: true,
            slots: (0..3)
                .map(|i| Slot {
                    nested: false,
                    offset: i * 4,
                    field: (i + 1) as Object,
                    shape: Shape::Scalar {
                        width: 4,
                        boolean: false,
                    },
                })
                .collect(),
        };
        assert_eq!(scalar.scalar_inline().unwrap(), (12, 4));
        scalar.slots.pop();
        assert!(scalar.scalar_inline().is_err());
        scalar.width = 8;
        scalar.slots[0].shape = Shape::Scalar {
            width: 1,
            boolean: true,
        };
        assert!(scalar.scalar_inline().is_err());
        assert!(layout().scalar_inline().is_err());
        let mut nested = Layout {
            class: 2usize as Object,
            width: 24,
            alignment: 4,
            value: true,
            slots: vec![
                Slot {
                    nested: false,
                    offset: 0,
                    field: 1usize as Object,
                    shape: Shape::Inline {
                        width: 12,
                        alignment: 4,
                    },
                },
                Slot {
                    nested: false,
                    offset: 12,
                    field: 2usize as Object,
                    shape: Shape::Inline {
                        width: 12,
                        alignment: 4,
                    },
                },
            ],
        };
        assert_eq!(nested.scalar_inline().unwrap(), (24, 4));
        nested
            .validate(
                &[0; 24],
                api_for_empty_refs(),
                &instances::Registry::default(),
            )
            .unwrap();
        nested.slots[1].offset = 8;
        assert!(nested.check().is_err());
    }
    #[test]
    fn nested_reference_boolean_leaves_use_parent_offsets_and_keep_padding_checked() {
        let mut child = layout();
        child.slots[1].shape = Shape::Scalar {
            width: 1,
            boolean: true,
        };
        let mut parent = Layout {
            class: 2usize as Object,
            width: 40,
            alignment: 8,
            value: true,
            slots: Vec::new(),
        };
        parent.append_nested(8, child.clone()).unwrap();
        parent.check().unwrap();
        assert_eq!(
            parent.slots.iter().map(|s| s.offset).collect::<Vec<_>>(),
            [8, 16, 24]
        );
        assert!(parent.slots.iter().all(|s| s.nested));
        assert!(parent.scalar_inline().is_err());
        let api = api_for_empty_refs();
        let roots = instances::Registry::default();
        let mut bytes = [0; 40];
        bytes[16] = 1;
        parent.validate(&bytes, api, &roots).unwrap();
        bytes[16] = 2;
        assert!(parent.validate(&bytes, api, &roots).is_err());
        bytes[16] = 1;
        bytes[17] = 1;
        assert!(parent.validate(&bytes, api, &roots).is_err());
        assert!(parent.append_nested(20, child.clone()).is_err());
        assert!(parent.append_nested(24, child).is_err());
    }
    #[test]
    fn late_forged_reference_padding_and_overlap_are_rejected() {
        let api = instances::tests::api();
        let mut roots = instances::Registry::default();
        let token = roots.borrow(api, 4usize as Object).unwrap();
        let mut bytes = vec![0; 24];
        bytes[..8].copy_from_slice(&token.to_le_bytes());
        layout().validate(&bytes, api, &roots).unwrap();
        bytes[16..].copy_from_slice(&99u64.to_le_bytes());
        assert!(layout().validate(&bytes, api, &roots).is_err());
        bytes[16..].fill(0);
        bytes[12] = 1;
        assert!(layout().validate(&bytes, api, &roots).is_err());
        let mut underaligned = layout();
        underaligned.alignment = 4;
        assert!(underaligned.check().is_err());
        let mut overlap = layout();
        overlap.slots[2].offset = 8;
        assert!(overlap.check().is_err());
        let mut misaligned = layout();
        misaligned.slots[2].offset = 12;
        assert!(misaligned.check().is_err());
    }
}
