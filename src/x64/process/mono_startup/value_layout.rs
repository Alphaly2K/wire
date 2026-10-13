//! Bounded metadata-derived value layouts. Only declared scalar/enum/reference
//! slots are copied; padding is owned zero bytes, never native heap contents.
use super::{instances, Object};
use std::{ffi::CStr, ptr};
pub(super) const MAX_VALUE_BYTES: usize = 4096;
mod numeric;
pub(super) use numeric::NumericLayout;
#[derive(Clone, Copy)]
pub(super) enum Shape {
    Scalar { width: usize, boolean: bool },
    Reference(Object),
    Inline { width: usize, alignment: usize },
    EngineAddress { condition: bool },
}
impl Shape {
    pub(super) fn width(self) -> usize {
        match self {
            Self::Scalar { width, .. } | Self::Inline { width, .. } => width,
            Self::Reference(_) | Self::EngineAddress { .. } => 8,
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
    /// Explicit scalar unions have several field views of the same bytes.
    /// Collapse those views only when every byte is ordinary numeric data;
    /// reference/Boolean/pointer overlaps never enter this raw-byte protocol.
    fn canonicalize_scalar_union(&mut self, explicit: bool) -> Result<(), String> {
        if !explicit || !self.value || !(1..=MAX_VALUE_BYTES).contains(&self.width) {
            return Err("overlapping value requires explicit scalar layout".into());
        }
        let mut covered = vec![false; self.width];
        for slot in &self.slots {
            let width = slot.shape.width();
            let alignment = slot.shape.alignment();
            let end = slot
                .offset
                .checked_add(width)
                .ok_or("union field range overflow")?;
            if !matches!(
                slot.shape,
                Shape::Scalar { boolean: false, .. } | Shape::Inline { .. }
            ) || width == 0
                || !matches!(alignment, 1 | 2 | 4 | 8)
                || alignment > self.alignment
                || !width.is_multiple_of(alignment)
                || !slot.offset.is_multiple_of(alignment)
                || end > self.width
            {
                return Err("union field is not bounded aligned numeric data".into());
            }
            covered[slot.offset..end].fill(true);
        }
        if covered.iter().any(|&byte| !byte) {
            return Err("scalar union has uncovered bytes".into());
        }
        // This synthetic leaf describes the payload, not an embedding field.
        self.slots = vec![Slot {
            nested: true,
            offset: 0,
            field: ptr::null_mut(),
            shape: Shape::Inline {
                width: self.width,
                alignment: self.alignment,
            },
        }];
        self.check()
    }
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
            || !(1..=MAX_VALUE_BYTES).contains(&self.width)
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
                || width > MAX_VALUE_BYTES
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
    /// Snapshot a verified native IL value for a semantic engine protocol.
    /// The caller owns readable native storage; reference conversion must root
    /// and type-check each actual object. Native padding and pointers never leak.
    pub(super) unsafe fn encode_native(
        &self,
        address: usize,
        mut reference: impl FnMut(Object, Object) -> Result<u64, String>,
    ) -> Result<Vec<u8>, String> {
        self.check()?;
        if !self.value
            || address == 0
            || !address.is_multiple_of(self.alignment)
            || address.checked_add(self.width).is_none()
            || self
                .slots
                .iter()
                .any(|slot| matches!(slot.shape, Shape::EngineAddress { .. }))
        {
            return Err("native typed snapshot storage/layout invalid".into());
        }
        let mut bytes = vec![0; self.width];
        for slot in &self.slots {
            let source = unsafe { (address as *const u8).add(slot.offset) };
            let output = &mut bytes[slot.offset..slot.offset + slot.shape.width()];
            if let Shape::Reference(class) = slot.shape {
                let native = unsafe { source.cast::<Object>().read_unaligned() };
                output.copy_from_slice(&reference(native, class)?.to_le_bytes());
            } else {
                if matches!(slot.shape, Shape::Scalar { boolean: true, .. })
                    && unsafe { source.read() } > 1
                {
                    return Err("native typed snapshot Boolean is not canonical".into());
                }
                unsafe { ptr::copy_nonoverlapping(source, output.as_mut_ptr(), output.len()) };
            }
        }
        Ok(bytes)
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
                Shape::EngineAddress { condition } => {
                    super::player_loop::check_address(
                        u64::from_le_bytes(data.try_into().unwrap()),
                        condition,
                    )?;
                    None
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
    closed: super::closed_types::Api,
    image: unsafe extern "C" fn(Object) -> Object,
    value: unsafe extern "C" fn(Object) -> i32,
    size: unsafe extern "C" fn(Object, *mut u32) -> i32,
    fields: unsafe extern "C" fn(Object, *mut Object) -> Object,
    flags: unsafe extern "C" fn(Object) -> u32,
    class_flags: unsafe extern "C" fn(Object) -> u32,
    parent: unsafe extern "C" fn(Object) -> Object,
    offset: unsafe extern "C" fn(Object) -> i32,
    ty: unsafe extern "C" fn(Object) -> Object,
    kind: unsafe extern "C" fn(Object) -> u32,
    byref: unsafe extern "C" fn(Object) -> u32,
    type_class: unsafe extern "C" fn(Object) -> Object,
    is_enum: unsafe extern "C" fn(Object) -> i32,
    enum_base: unsafe extern "C" fn(Object) -> Object,
    class_name: unsafe extern "C" fn(Object) -> *const libc::c_char,
    field_name: unsafe extern "C" fn(Object) -> *const libc::c_char,
    fixed_buffer: unsafe extern "C" fn(u32, Object, Object, *mut Object, *mut u32) -> i32,
    unsafe_value: unsafe extern "C" fn(u32, Object) -> i32,
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
            closed: super::closed_types::Api::load(library)?,
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
            class_flags: sym!("mono_class_get_flags", unsafe extern "C" fn(Object) -> u32),
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
            class_name: sym!(
                "mono_class_get_name",
                unsafe extern "C" fn(Object) -> *const libc::c_char
            ),
            field_name: sym!(
                "mono_field_get_name",
                unsafe extern "C" fn(Object) -> *const libc::c_char
            ),
            fixed_buffer: sym!(
                "wire_mono_field_fixed_buffer",
                unsafe extern "C" fn(u32, Object, Object, *mut Object, *mut u32) -> i32
            ),
            unsafe_value: sym!(
                "wire_mono_class_is_unsafe_value",
                unsafe extern "C" fn(u32, Object) -> i32
            ),
            owned_image,
        })
    }
    pub(super) fn owns(self, class: Object) -> bool {
        !class.is_null() && unsafe { (self.image)(class) } == self.owned_image
    }
    pub(super) fn layout(self, class: Object) -> Result<Layout, String> {
        self.layout_inner(class, &mut Vec::new(), &[])
    }
    pub(super) fn layout_in(self, class: Object, images: &[Object]) -> Result<Layout, String> {
        if !self.owns(class)
            || images.is_empty()
            || images.len() > 512
            || images.iter().any(|image| image.is_null())
            || !images.contains(&self.owned_image)
        {
            return Err("value layout image family must contain the owned root image".into());
        }
        self.layout_inner(class, &mut Vec::new(), images)
    }
    fn layout_inner(
        self,
        class: Object,
        path: &mut Vec<Object>,
        images: &[Object],
    ) -> Result<Layout, String> {
        if path.len() >= 8 || path.contains(&class) {
            return Err("nested value layout exceeds acyclic depth bound".into());
        }
        if class.is_null()
            || !(self.owns(class) || images.contains(&unsafe { (self.image)(class) }))
            || unsafe { (self.value)(class) } == 0
        {
            return Err("value layout requires an owned metadata value class".into());
        }
        self.closed.concrete(class)?;
        if unsafe { (self.unsafe_value)(1, class) } != 0 {
            return Err("unsafe value storage requires its declaring fixed-buffer protocol".into());
        }
        path.push(class);
        let mut alignment = 0;
        let width = unsafe { (self.size)(class, &mut alignment) };
        if !(1..=MAX_VALUE_BYTES as i32).contains(&width) {
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
        let mut overlap = false;
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
            let mut fixed_type = ptr::null_mut();
            let mut fixed_count = 0;
            match unsafe { (self.fixed_buffer)(1, class, field, &mut fixed_type, &mut fixed_count) }
            {
                0 if fixed_type.is_null() && fixed_count == 0 => {}
                1 => {
                    if !matches!(kind, 0x11 | 0x15)
                        || fixed_type.is_null()
                        || unsafe { (self.byref)(fixed_type) } != 0
                    {
                        return Err(
                            "fixed-buffer attribute must describe inline value storage".into()
                        );
                    }
                    let storage = unsafe { (self.type_class)(ty) };
                    let child =
                        self.fixed_storage(storage, fixed_type, fixed_count as usize, images)?;
                    let end = offset
                        .checked_add(child.width)
                        .ok_or("fixed-buffer field range overflow")?;
                    overlap |= ranges
                        .iter()
                        .any(|&(start, stop)| offset < stop && start < end);
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
                _ => return Err("fixed-buffer metadata query rejected".into()),
            }
            if matches!(kind, 0x11 | 0x15) {
                let target = unsafe { (self.type_class)(ty) };
                if target.is_null() {
                    return Err("nested value class absent".into());
                }
                self.closed.concrete(target)?;
                if unsafe { (self.value)(target) } != 0 && unsafe { (self.is_enum)(target) } == 0 {
                    let child = self.layout_inner(target, path, images)?;
                    let end = offset
                        .checked_add(child.width)
                        .ok_or("nested field range overflow")?;
                    overlap |= ranges
                        .iter()
                        .any(|&(start, stop)| offset < stop && start < end);
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
            } else if matches!(kind, 0xe | 0x12 | 0x15 | 0x1c | 0x1d) {
                let target = unsafe { (self.type_class)(ty) };
                if target.is_null() || unsafe { (self.value)(target) } != 0 {
                    return Err("reference value field has absent/value class".into());
                }
                self.closed.concrete(target)?;
                Shape::Reference(target)
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
            } else if let Some(condition) =
                super::player_loop::address_slot(class, field, kind, offset)
            {
                Shape::EngineAddress { condition }
            } else {
                let label = |name: *const libc::c_char| {
                    if name.is_null() {
                        "<absent>".into()
                    } else {
                        unsafe { CStr::from_ptr(name) }
                            .to_string_lossy()
                            .into_owned()
                    }
                };
                return Err(format!(
                    "raw pointer/generic/complex value field is not admitted: class={} field={} kind={kind:#x} offset={offset}",
                    label(unsafe { (self.class_name)(class) }),
                    label(unsafe { (self.field_name)(field) }),
                ));
            };
            let end = offset
                .checked_add(shape.width())
                .ok_or("value field range overflow")?;
            overlap |= ranges
                .iter()
                .any(|&(start, stop)| offset < stop && start < end);
            ranges.push((offset, end));
            layout.slots.push(Slot {
                nested: false,
                offset,
                field,
                shape,
            });
        }
        if overlap {
            layout
                .canonicalize_scalar_union(unsafe { (self.class_flags)(class) } & 0x18 == 0x10)?;
        } else {
            layout.check()?;
        }
        path.pop();
        Ok(layout)
    }
    fn fixed_storage(
        self,
        class: Object,
        element: Object,
        count: usize,
        images: &[Object],
    ) -> Result<Layout, String> {
        if class.is_null()
            || !(self.owns(class) || images.contains(&unsafe { (self.image)(class) }))
            || unsafe { (self.value)(class) } == 0
            || unsafe { (self.unsafe_value)(1, class) } != 1
            || !(1..=4096).contains(&count)
        {
            return Err("fixed-buffer storage class/lifetime/count invalid".into());
        }
        self.closed.concrete(class)?;
        let shape = scalar(unsafe { (self.kind)(element) })
            .ok_or("fixed-buffer element is not a numeric primitive")?;
        let width = count
            .checked_mul(shape.width())
            .filter(|n| *n <= MAX_VALUE_BYTES)
            .ok_or("fixed-buffer byte length outside bound")?;
        let mut alignment = 0;
        if unsafe { (self.size)(class, &mut alignment) } != width as i32
            || alignment as usize != shape.alignment()
        {
            return Err(
                "fixed-buffer actual storage size/alignment differs from declaration".into(),
            );
        }
        let mut iterator = ptr::null_mut();
        let mut leaf = None;
        loop {
            let field = unsafe { (self.fields)(class, &mut iterator) };
            if field.is_null() {
                break;
            }
            if unsafe { (self.flags)(field) } & (0x10 | 0x40) != 0 {
                continue;
            }
            let ty = unsafe { (self.ty)(field) };
            if leaf.is_some()
                || unsafe { (self.parent)(field) } != class
                || unsafe { (self.offset)(field) } != 16
                || ty.is_null()
                || unsafe { (self.byref)(ty) } != 0
                || unsafe { (self.kind)(ty) } != unsafe { (self.kind)(element) }
            {
                return Err("fixed-buffer actual element field differs from declaration".into());
            }
            leaf = Some(field);
        }
        let field = leaf.ok_or("fixed-buffer storage has no declared element field")?;
        let slots = if matches!(shape, Shape::Scalar { boolean: true, .. }) {
            if count > 64 {
                return Err("fixed Boolean buffer exceeds typed slot bound".into());
            }
            (0..count)
                .map(|offset| Slot {
                    nested: true,
                    offset,
                    field,
                    shape,
                })
                .collect()
        } else {
            vec![Slot {
                nested: true,
                offset: 0,
                field,
                shape: Shape::Inline {
                    width,
                    alignment: alignment as usize,
                },
            }]
        };
        let layout = Layout {
            class,
            width,
            alignment: alignment as usize,
            slots,
            value: true,
        };
        layout.check()?;
        Ok(layout)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn native_snapshot_replaces_references_zeroes_padding_and_checks_boolean_before_export() {
        let layout = Layout {
            class: 1usize as Object,
            width: 24,
            alignment: 8,
            value: true,
            slots: vec![
                Slot {
                    nested: false,
                    offset: 0,
                    field: ptr::null_mut(),
                    shape: Shape::Reference(2usize as Object),
                },
                Slot {
                    nested: true,
                    offset: 8,
                    field: ptr::null_mut(),
                    shape: Shape::Scalar {
                        width: 4,
                        boolean: false,
                    },
                },
                Slot {
                    nested: false,
                    offset: 12,
                    field: ptr::null_mut(),
                    shape: Shape::Scalar {
                        width: 1,
                        boolean: true,
                    },
                },
            ],
        };
        let mut native = [0xababababababababu64; 3];
        native[0] = 0x12340000;
        native[1] = 0xeeeeee01deadbeef;
        let mut seen = Vec::new();
        let bytes = unsafe {
            layout.encode_native(native.as_ptr() as usize, |object, class| {
                seen.push((object as usize, class as usize));
                Ok(0x6fff00010000)
            })
        }
        .unwrap();
        assert_eq!(seen, [(0x12340000, 2)]);
        assert_eq!(&bytes[..8], &0x6fff00010000u64.to_le_bytes());
        assert_eq!(&bytes[8..13], &[0xef, 0xbe, 0xad, 0xde, 1]);
        assert!(bytes[13..].iter().all(|&b| b == 0));
        native[1] = 0xeeeeee02deadbeef;
        assert!(unsafe { layout.encode_native(native.as_ptr() as usize, |_, _| Ok(0)) }.is_err());
        assert!(unsafe { layout.encode_native(0, |_, _| Ok(0)) }.is_err());
        assert!(
            unsafe { layout.encode_native(native.as_ptr() as usize + 1, |_, _| Ok(0)) }.is_err()
        );
        assert!(unsafe {
            layout.encode_native(native.as_ptr() as usize, |_, _| {
                Err("incompatible class".into())
            })
        }
        .is_err());
    }
    thread_local! { static FIELD_KIND: Cell<u32> = const { Cell::new(0x15) }; }
    unsafe extern "C" fn test_image(_: Object) -> Object {
        1usize as Object
    }
    unsafe extern "C" fn test_value(class: Object) -> i32 {
        i32::from(class == 10usize as Object)
    }
    unsafe extern "C" fn test_all_value(_: Object) -> i32 {
        1
    }
    unsafe extern "C" fn test_size(_: Object, alignment: *mut u32) -> i32 {
        unsafe { *alignment = 8 };
        8
    }
    unsafe extern "C" fn test_fields(_: Object, iterator: *mut Object) -> Object {
        if unsafe { *iterator }.is_null() {
            unsafe { *iterator = 1usize as Object };
            1usize as Object
        } else {
            ptr::null_mut()
        }
    }
    unsafe extern "C" fn test_zero(_: Object) -> u32 {
        0
    }
    unsafe extern "C" fn test_parent(_: Object) -> Object {
        10usize as Object
    }
    unsafe extern "C" fn test_offset(_: Object) -> i32 {
        16
    }
    unsafe extern "C" fn test_type(_: Object) -> Object {
        3usize as Object
    }
    unsafe extern "C" fn test_kind(_: Object) -> u32 {
        FIELD_KIND.with(Cell::get)
    }
    unsafe extern "C" fn test_class(_: Object) -> Object {
        2usize as Object
    }
    unsafe extern "C" fn test_enum(_: Object) -> i32 {
        0
    }
    unsafe extern "C" fn test_name(_: Object) -> *const libc::c_char {
        c"test".as_ptr()
    }
    unsafe extern "C" fn no_fixed(
        _: u32,
        _: Object,
        _: Object,
        element: *mut Object,
        count: *mut u32,
    ) -> i32 {
        unsafe {
            *element = ptr::null_mut();
            *count = 0;
        }
        0
    }
    unsafe extern "C" fn safe_value(_: u32, _: Object) -> i32 {
        0
    }
    fn test_api() -> Api {
        Api {
            closed: super::super::closed_types::tests::api(),
            image: test_image,
            value: test_value,
            size: test_size,
            fields: test_fields,
            flags: test_zero,
            class_flags: test_zero,
            parent: test_parent,
            offset: test_offset,
            ty: test_type,
            kind: test_kind,
            byref: test_zero,
            type_class: test_class,
            is_enum: test_enum,
            enum_base: test_type,
            class_name: test_name,
            field_name: test_name,
            fixed_buffer: no_fixed,
            unsafe_value: safe_value,
            owned_image: 1usize as Object,
        }
    }
    #[test]
    fn fixed_storage_expands_all_declared_elements_and_rejects_standalone_or_mismatched_storage() {
        unsafe extern "C" fn size(class: Object, alignment: *mut u32) -> i32 {
            unsafe {
                *alignment = 4;
            }
            if class as usize == 10 {
                100
            } else {
                96
            }
        }
        unsafe extern "C" fn unsafe_value(_: u32, class: Object) -> i32 {
            i32::from(class as usize == 20)
        }
        unsafe extern "C" fn fields(class: Object, iterator: *mut Object) -> Object {
            let next = unsafe { *iterator } as usize + 1;
            unsafe {
                *iterator = next as Object;
            }
            match (class as usize, next) {
                (10, 1) => 1usize as Object,
                (10, 2) => 3usize as Object,
                (20, 1) => 2usize as Object,
                _ => ptr::null_mut(),
            }
        }
        unsafe extern "C" fn parent(field: Object) -> Object {
            (if field as usize == 2 { 20 } else { 10 }) as Object
        }
        unsafe extern "C" fn ty(field: Object) -> Object {
            field
        }
        unsafe extern "C" fn kind(ty: Object) -> u32 {
            match ty as usize {
                1 => 0x11,
                3 => 2,
                _ => 0xc,
            }
        }
        unsafe extern "C" fn target(_: Object) -> Object {
            20usize as Object
        }
        unsafe extern "C" fn offset(field: Object) -> i32 {
            if field as usize == 3 {
                112
            } else {
                16
            }
        }
        unsafe extern "C" fn fixed(
            _: u32,
            _: Object,
            field: Object,
            element: *mut Object,
            count: *mut u32,
        ) -> i32 {
            unsafe {
                *element = if field as usize == 1 {
                    4usize as Object
                } else {
                    ptr::null_mut()
                };
                *count = if field as usize == 1 { 24 } else { 0 };
            }
            i32::from(field as usize == 1)
        }
        let api = Api {
            value: test_all_value,
            size,
            unsafe_value,
            fields,
            parent,
            ty,
            kind,
            type_class: target,
            offset,
            fixed_buffer: fixed,
            ..test_api()
        };
        let layout = api.layout(10usize as Object).unwrap();
        assert_eq!(layout.slots[0].shape.width(), 96);
        let codec = layout.numeric().unwrap();
        let mut source = vec![0xa5; 100];
        source[96] = 1;
        let bytes = codec.snapshot(&source).unwrap();
        assert_eq!(&bytes[..96], &source[..96]);
        assert_eq!(&bytes[96..], &[1, 0, 0, 0]);
        source[96] = 2;
        assert!(codec.snapshot(&source).is_err());
        assert!(api.layout(20usize as Object).is_err());
        assert!(api
            .fixed_storage(20usize as Object, 4usize as Object, 23, &[])
            .is_err());
        assert!(api
            .fixed_storage(20usize as Object, 3usize as Object, 96, &[])
            .is_err());
        assert!(api
            .fixed_storage(20usize as Object, 4usize as Object, 4097, &[])
            .is_err());
        assert!(Api {
            unsafe_value: safe_value,
            ..api
        }
        .fixed_storage(20usize as Object, 4usize as Object, 24, &[])
        .is_err());
    }
    unsafe extern "C" fn union_size(_: Object, alignment: *mut u32) -> i32 {
        unsafe { *alignment = 4 };
        4
    }
    unsafe extern "C" fn union_fields(_: Object, iterator: *mut Object) -> Object {
        let next = unsafe { *iterator } as usize + 1;
        unsafe { *iterator = next as Object };
        if next <= 5 {
            next as Object
        } else {
            ptr::null_mut()
        }
    }
    unsafe extern "C" fn union_offset(field: Object) -> i32 {
        16 + (field as usize).saturating_sub(2) as i32
    }
    unsafe extern "C" fn union_kind(ty: Object) -> u32 {
        if ty as usize == 1 {
            8
        } else {
            5
        }
    }
    unsafe extern "C" fn union_flags(_: Object) -> u32 {
        0x10
    }
    unsafe extern "C" fn union_type(field: Object) -> Object {
        field
    }
    #[test]
    fn actual_explicit_numeric_union_becomes_one_payload_with_identical_bytes() {
        let api = Api {
            size: union_size,
            fields: union_fields,
            offset: union_offset,
            kind: union_kind,
            ty: union_type,
            class_flags: union_flags,
            ..test_api()
        };
        let layout = api.layout(10usize as Object).unwrap();
        assert_eq!(layout.scalar_inline().unwrap(), (4, 4));
        assert_eq!(layout.slots.len(), 1);
        assert!(layout.slots[0].nested);
        let native = 0xa1b2c3d4u32;
        assert_eq!(
            unsafe {
                layout
                    .encode_native(&native as *const u32 as usize, |_, _| {
                        panic!("numeric union must not contain a reference")
                    })
                    .unwrap()
            },
            native.to_le_bytes()
        );
        let ordinary = Api {
            class_flags: test_zero,
            ..api
        };
        assert!(ordinary.layout(10usize as Object).is_err());
        let mut original = layout.clone();
        original.slots = vec![
            Slot {
                nested: false,
                offset: 0,
                field: 1usize as Object,
                shape: Shape::Scalar {
                    width: 4,
                    boolean: false,
                },
            },
            Slot {
                nested: false,
                offset: 0,
                field: 2usize as Object,
                shape: Shape::Scalar {
                    width: 1,
                    boolean: true,
                },
            },
        ];
        assert!(original.clone().canonicalize_scalar_union(true).is_err());
        original.slots[1].shape = Shape::Reference(2usize as Object);
        assert!(original.clone().canonicalize_scalar_union(true).is_err());
        original.slots[1].shape = Shape::EngineAddress { condition: false };
        assert!(original.clone().canonicalize_scalar_union(true).is_err());
        original.slots[1].shape = Shape::Inline {
            width: 2,
            alignment: 2,
        };
        original.slots[0].shape = Shape::Scalar {
            width: 2,
            boolean: false,
        };
        assert!(original.clone().canonicalize_scalar_union(true).is_err());
        original.slots[0].shape = Shape::Scalar {
            width: 4,
            boolean: false,
        };
        original.slots[1].offset = 3;
        assert!(original.canonicalize_scalar_union(true).is_err());
    }
    #[test]
    fn value_fields_admit_closed_generic_and_object_references_but_reject_pointers() {
        let api = test_api();
        for kind in [0x15, 0x1c] {
            FIELD_KIND.with(|value| value.set(kind));
            let layout = api.layout(10usize as Object).unwrap();
            assert!(matches!(layout.slots[0].shape, Shape::Reference(_)));
            assert!(layout.scalar_inline().is_err());
            layout
                .validate(
                    &[0; 8],
                    api_for_empty_refs(),
                    &instances::Registry::default(),
                )
                .unwrap();
            assert!(layout
                .validate(
                    &99u64.to_le_bytes(),
                    api_for_empty_refs(),
                    &instances::Registry::default()
                )
                .is_err());
        }
        for kind in [0xf, 0x13, 0x16, 0x18, 0x19, 0x1b, 0x1e] {
            FIELD_KIND.with(|value| value.set(kind));
            assert!(api.layout(10usize as Object).is_err());
        }
        FIELD_KIND.with(|value| value.set(0x15));
        let value_class = Api {
            value: test_all_value,
            ..api
        };
        assert!(value_class.layout(10usize as Object).is_err());
    }
    thread_local! { static CROSS_CYCLE: Cell<bool> = const { Cell::new(false) }; }
    unsafe extern "C" fn cross_image(class: Object) -> Object {
        (if class == 10usize as Object { 1 } else { 2 }) as Object
    }
    unsafe extern "C" fn cross_fields(class: Object, iterator: *mut Object) -> Object {
        if unsafe { *iterator }.is_null() {
            unsafe { *iterator = 1usize as Object };
            class
        } else {
            ptr::null_mut()
        }
    }
    unsafe extern "C" fn cross_parent(field: Object) -> Object {
        field
    }
    unsafe extern "C" fn cross_type(field: Object) -> Object {
        (if field == 10usize as Object { 3 } else { 4 }) as Object
    }
    unsafe extern "C" fn cross_kind(ty: Object) -> u32 {
        if ty == 3usize as Object || CROSS_CYCLE.with(Cell::get) {
            0x11
        } else {
            0xb
        }
    }
    unsafe extern "C" fn cross_class(_: Object) -> Object {
        20usize as Object
    }
    #[test]
    fn nested_values_only_cross_the_explicit_owned_image_family() {
        let api = Api {
            image: cross_image,
            value: test_all_value,
            fields: cross_fields,
            parent: cross_parent,
            ty: cross_type,
            kind: cross_kind,
            type_class: cross_class,
            ..test_api()
        };
        let root = 10usize as Object;
        let images = [1usize as Object, 2usize as Object];
        assert!(api.layout(root).is_err());
        assert!(api.layout_in(root, &images[..1]).is_err());
        let layout = api.layout_in(root, &images).unwrap();
        assert_eq!(layout.scalar_inline().unwrap(), (8, 8));
        assert!(api.layout_in(20usize as Object, &images).is_err());
        assert!(api.layout_in(root, &[]).is_err());
        assert!(api.layout_in(root, &[ptr::null_mut(), images[0]]).is_err());
        assert!(api.layout_in(root, &[images[0]; 513]).is_err());
        CROSS_CYCLE.with(|cycle| cycle.set(true));
        let error = api.layout_in(root, &images).err().unwrap();
        assert!(error.contains("acyclic"));
        CROSS_CYCLE.with(|cycle| cycle.set(false));
    }
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
        let engine = Layout {
            class: 2usize as Object,
            width: 8,
            alignment: 8,
            value: true,
            slots: vec![Slot {
                nested: false,
                offset: 0,
                field: 1usize as Object,
                shape: Shape::EngineAddress { condition: false },
            }],
        };
        engine.check().unwrap();
        assert!(engine.scalar_inline().is_err());
        assert!(engine
            .validate(
                &99u64.to_le_bytes(),
                api_for_empty_refs(),
                &instances::Registry::default()
            )
            .is_err());
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
